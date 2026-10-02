//! Writes finished packets into a file, on a thread of its own so a slow
//! disk never holds up capture. Files are written as `<name>.part` and only
//! get their real name once they're complete, so anything watching the
//! folder (the sync service) never picks up half a file.

use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

use anyhow::{bail, Context, Result};
use ffmpeg_sys_next as ff;

use crate::audio::RATE;
use crate::encoder::Codec;

pub fn check(ret: i32, what: &str) -> Result<i32> {
    if ret < 0 {
        let mut buf = [0 as std::ffi::c_char; 256];
        unsafe { ff::av_strerror(ret, buf.as_mut_ptr(), buf.len()) };
        let msg = unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy();
        bail!("{what}: {msg}");
    }
    Ok(ret)
}

/// One encoded packet. Cheap to clone, the data is shared.
#[derive(Clone)]
pub struct Packet {
    /// 0 is the video, audio tracks follow.
    pub stream: usize,
    /// In the stream's own units: frames for video, samples for audio.
    pub pts: i64,
    pub duration: i64,
    pub key: bool,
    pub data: Payload,
}

/// A packet's bytes: in memory, or on disk (the replay buffer keeps them
/// there, see replay.rs).
#[derive(Clone)]
pub enum Payload {
    Mem(Arc<[u8]>),
    Disk { file: Arc<std::fs::File>, offset: u64, len: usize },
}

impl Payload {
    pub fn len(&self) -> usize {
        match self {
            Payload::Mem(b) => b.len(),
            Payload::Disk { len, .. } => *len,
        }
    }

    /// Copies the bytes into `out`, which is `len()` long. Ones read from
    /// disk get dropped from memory again right after.
    pub fn read_into(&self, out: &mut [u8]) -> std::io::Result<()> {
        match self {
            Payload::Mem(b) => {
                out.copy_from_slice(b);
                Ok(())
            }
            Payload::Disk { file, offset, len } => {
                use std::os::fd::AsRawFd;
                use std::os::unix::fs::FileExt;
                file.read_exact_at(out, *offset)?;
                unsafe { libc::posix_fadvise(file.as_raw_fd(), *offset as i64, *len as i64, libc::POSIX_FADV_DONTNEED) };
                Ok(())
            }
        }
    }
}

impl From<Vec<u8>> for Payload {
    fn from(v: Vec<u8>) -> Self {
        Payload::Mem(Arc::from(v))
    }
}

/// An audio encoder's stream parameters, copied so files can be set up from
/// any thread.
pub struct Params(*mut ff::AVCodecParameters);

// Written once when copied, only read after that.
unsafe impl Send for Params {}
unsafe impl Sync for Params {}

impl Params {
    pub fn from_encoder(ctx: *const ff::AVCodecContext) -> Result<Self> {
        unsafe {
            let par = ff::avcodec_parameters_alloc();
            if par.is_null() {
                bail!("out of memory");
            }
            let p = Self(par);
            check(ff::avcodec_parameters_from_context(par, ctx), "copying audio parameters")?;
            Ok(p)
        }
    }
}

impl Drop for Params {
    fn drop(&mut self) {
        unsafe { ff::avcodec_parameters_free(&mut self.0) };
    }
}

/// Everything a file needs before its first packet.
pub struct Streams {
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// VPS/SPS/PPS (or SPS/PPS), Annex B.
    pub video_params: Vec<u8>,
    pub audio: Vec<(&'static str, Params)>,
}

pub struct Saved {
    pub path: PathBuf,
    pub frames: u64,
    pub bytes: u64,
}

impl Saved {
    pub fn seconds(&self, fps: u32) -> f64 {
        self.frames as f64 / fps.max(1) as f64
    }
}

/// How much gets written before it's pushed to disk and dropped from memory.
/// A clip is a few hundred MB written in about a second: left alone it all
/// sits in memory waiting to be written, and the kernel making room for it
/// (into compressed swap, on the Frame) once stalled PipeWire's realtime
/// thread long enough to get it killed, taking the audio with it.
const FLUSH_EVERY: i64 = 8 * 1024 * 1024;

struct Writer {
    fmt: *mut ff::AVFormatContext,
    packet: *mut ff::AVPacket,
    part: PathBuf,
    path: PathBuf,
    fps: u32,
    frames: u64,
    bytes: u64,
    /// The file again, for pushing what's written out of memory.
    file: Option<std::fs::File>,
    /// How far into the file is on disk and out of memory.
    flushed: i64,
}

// Owned by exactly one thread at a time.
unsafe impl Send for Writer {}

fn part_path(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(".part");
    PathBuf::from(p)
}

impl Writer {
    fn create(path: &Path, streams: &Streams) -> Result<Self> {
        let part = part_path(path);
        let name = CString::new(path.to_string_lossy().as_bytes())?;
        let part_c = CString::new(part.to_string_lossy().as_bytes())?;
        let mut w = Self {
            fmt: ptr::null_mut(),
            packet: unsafe { ff::av_packet_alloc() },
            part,
            path: path.to_owned(),
            fps: streams.fps,
            frames: 0,
            bytes: 0,
            file: None,
            flushed: 0,
        };
        unsafe {
            // The container comes from the real name, the .part one means nothing to FFmpeg.
            let format = ff::av_guess_format(ptr::null(), name.as_ptr(), ptr::null());
            if format.is_null() {
                bail!("picking a container for {} (use .mp4 or .mkv)", path.display());
            }
            check(
                ff::avformat_alloc_output_context2(&mut w.fmt, format, ptr::null(), part_c.as_ptr()),
                "setting up the output file",
            )?;
            w.add_video(streams)?;
            for (i, (title, params)) in streams.audio.iter().enumerate() {
                w.add_audio(title, params, i == 0)?;
            }
            check(ff::avio_open(&mut (*w.fmt).pb, part_c.as_ptr(), ff::AVIO_FLAG_WRITE), "creating the output file")?;
            w.file = std::fs::File::open(&w.part).ok();
            // No faststart: moving the index to the front means rewriting the
            // whole file when the recording stops, which is a lot of I/O on a
            // long recording.
            check(ff::avformat_write_header(w.fmt, ptr::null_mut()), "writing the file header")?;
        }
        Ok(w)
    }

    fn is_mp4(&self) -> bool {
        let name = unsafe { CStr::from_ptr((*(*self.fmt).oformat).name) };
        name.to_bytes().starts_with(b"mp4") || name.to_bytes().starts_with(b"mov")
    }

    unsafe fn add_video(&mut self, s: &Streams) -> Result<()> {
        let st = ff::avformat_new_stream(self.fmt, ptr::null());
        if st.is_null() {
            bail!("couldn't add the video stream");
        }
        let par = &mut *(*st).codecpar;
        par.codec_type = ff::AVMediaType::AVMEDIA_TYPE_VIDEO;
        par.codec_id = match s.codec {
            Codec::Hevc => ff::AVCodecID::AV_CODEC_ID_HEVC,
            Codec::H264 => ff::AVCodecID::AV_CODEC_ID_H264,
        };
        if s.codec == Codec::Hevc && self.is_mp4() {
            // hvc1 rather than hev1, or Apple devices refuse to play it.
            par.codec_tag = u32::from_le_bytes(*b"hvc1");
        }
        par.width = s.width as i32;
        par.height = s.height as i32;
        par.format = ff::AVPixelFormat::AV_PIX_FMT_YUV420P as i32;
        par.color_range = ff::AVColorRange::AVCOL_RANGE_MPEG;
        par.color_primaries = ff::AVColorPrimaries::AVCOL_PRI_BT709;
        par.color_trc = ff::AVColorTransferCharacteristic::AVCOL_TRC_BT709;
        par.color_space = ff::AVColorSpace::AVCOL_SPC_BT709;
        par.framerate = ff::AVRational { num: s.fps as i32, den: 1 };
        let pad = ff::AV_INPUT_BUFFER_PADDING_SIZE as usize;
        par.extradata = ff::av_mallocz(s.video_params.len() + pad).cast();
        ptr::copy_nonoverlapping(s.video_params.as_ptr(), par.extradata, s.video_params.len());
        par.extradata_size = s.video_params.len() as i32;
        (*st).time_base = ff::AVRational { num: 1, den: s.fps as i32 };
        (*st).avg_frame_rate = par.framerate;
        Ok(())
    }

    unsafe fn add_audio(&mut self, title: &str, params: &Params, default: bool) -> Result<()> {
        let st = ff::avformat_new_stream(self.fmt, ptr::null());
        if st.is_null() {
            bail!("couldn't add an audio stream");
        }
        check(ff::avcodec_parameters_copy((*st).codecpar, params.0), "setting up audio stream")?;
        (*st).time_base = ff::AVRational { num: 1, den: RATE as i32 };
        let title = CString::new(title)?;
        ff::av_dict_set(&mut (*st).metadata, c"title".as_ptr(), title.as_ptr(), 0);
        // The first track plays by default, others are there for editing.
        (*st).disposition = if default { ff::AV_DISPOSITION_DEFAULT } else { 0 };
        Ok(())
    }

    fn write(&mut self, p: &Packet) -> Result<()> {
        unsafe {
            let pkt = self.packet;
            check(ff::av_new_packet(pkt, p.data.len() as i32), "allocating a packet")?;
            let out = std::slice::from_raw_parts_mut((*pkt).data, p.data.len());
            p.data.read_into(out).context("reading the replay buffer")?;
            (*pkt).pts = p.pts;
            (*pkt).dts = p.pts;
            (*pkt).duration = p.duration;
            (*pkt).stream_index = p.stream as i32;
            if p.key {
                (*pkt).flags |= ff::AV_PKT_FLAG_KEY;
            }
            let from = if p.stream == 0 {
                ff::AVRational { num: 1, den: self.fps as i32 }
            } else {
                ff::AVRational { num: 1, den: RATE as i32 }
            };
            let st = *(*self.fmt).streams.add(p.stream);
            ff::av_packet_rescale_ts(pkt, from, (*st).time_base);
            if p.stream == 0 {
                self.frames += 1;
            }
            self.bytes += p.data.len() as u64;
            check(ff::av_interleaved_write_frame(self.fmt, pkt), "writing")?;
        }
        self.flush_some();
        Ok(())
    }

    /// Every FLUSH_EVERY bytes: waits for them to be on disk, then lets the
    /// kernel forget them. Paces the writing to the disk instead of to
    /// memory.
    fn flush_some(&mut self) {
        let Some(file) = &self.file else { return };
        use std::os::fd::AsRawFd;
        unsafe {
            let pb = (*self.fmt).pb;
            let at = ff::avio_seek(pb, 0, libc::SEEK_CUR);
            if at - self.flushed < FLUSH_EVERY {
                return;
            }
            ff::avio_flush(pb);
            let (fd, from, len) = (file.as_raw_fd(), self.flushed, at - self.flushed);
            let how = libc::SYNC_FILE_RANGE_WAIT_BEFORE | libc::SYNC_FILE_RANGE_WRITE | libc::SYNC_FILE_RANGE_WAIT_AFTER;
            libc::sync_file_range(fd, from, len, how);
            libc::posix_fadvise(fd, from, len, libc::POSIX_FADV_DONTNEED);
            self.flushed = at;
        }
    }

    fn finish(mut self) -> Result<Saved> {
        unsafe {
            check(ff::av_write_trailer(self.fmt), "finalizing the file")?;
            ff::avio_closep(&mut (*self.fmt).pb);
        }
        std::fs::rename(&self.part, &self.path).with_context(|| format!("renaming {}", self.part.display()))?;
        Ok(Saved { path: std::mem::take(&mut self.path), frames: self.frames, bytes: self.bytes })
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        unsafe {
            ff::av_packet_free(&mut self.packet);
            if !self.fmt.is_null() {
                if !(*self.fmt).pb.is_null() {
                    ff::avio_closep(&mut (*self.fmt).pb);
                }
                ff::avformat_free_context(self.fmt);
            }
        }
    }
}

/// A file being written on its own thread.
pub struct Handle {
    tx: Sender<Packet>,
    thread: JoinHandle<Result<Saved>>,
}

impl Handle {
    /// Creates the file right away, so a bad path fails here and not later.
    pub fn spawn(path: &Path, streams: &Streams) -> Result<Self> {
        let writer = Writer::create(path, streams)?;
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new().name("writer".into()).spawn(move || run(writer, rx))?;
        Ok(Self { tx, thread })
    }

    pub fn send(&self, p: Packet) {
        // A writer that failed has stopped listening; its error shows up in finish.
        let _ = self.tx.send(p);
    }

    /// Stops taking packets. The file gets finished in the background.
    pub fn close(self) -> JoinHandle<Result<Saved>> {
        drop(self.tx);
        self.thread
    }
}

fn run(mut writer: Writer, rx: Receiver<Packet>) -> Result<Saved> {
    // The game comes first, for the CPU and the disk.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 19);
        const IOPRIO_WHO_PROCESS: libc::c_long = 1;
        const BEST_EFFORT_LOWEST: libc::c_long = (2 << 13) | 7;
        libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, 0 as libc::c_long, BEST_EFFORT_LOWEST);
    }
    let mut failure = None;
    for p in rx {
        if let Err(e) = writer.write(&p) {
            failure = Some(e);
            break;
        }
    }
    // Even after an error, finish the file so everything up to it plays.
    let saved = writer.finish();
    match failure {
        Some(e) => {
            if saved.is_ok() {
                log::warn!("writing failed partway, the file has everything up to that point");
            }
            Err(e)
        }
        None => saved,
    }
}

//! Drives the Snapdragon's hardware video encoder (qcom iris) through the
//! V4L2 stateful encoder interface. Frames go in as dmabufs straight from the
//! GPU, compressed packets come out of mmapped buffers.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use v4l2r::bindings as v4l2;

use crate::gpu::Nv12Layout;

const BUF_TYPE_CAPTURE: u32 = 9; // VIDEO_CAPTURE_MPLANE
const BUF_TYPE_OUTPUT: u32 = 10; // VIDEO_OUTPUT_MPLANE
const MEMORY_MMAP: u32 = 1;
const MEMORY_DMABUF: u32 = 4;
const FIELD_NONE: u32 = 1;
const BUF_FLAG_KEYFRAME: u32 = 0x8;
const BUF_FLAG_ERROR: u32 = 0x40;
const BUF_FLAG_LAST: u32 = 0x0010_0000;
const ENC_CMD_STOP: u32 = 1;

const COLORSPACE_REC709: u32 = 3;
const YCBCR_ENC_709: u8 = 2;
const QUANTIZATION_LIM_RANGE: u8 = 2;
const XFER_FUNC_709: u8 = 1;

const CID_CODEC_BASE: u32 = 0x0099_0900;
const CID_B_FRAMES: u32 = CID_CODEC_BASE + 202;
const CID_GOP_SIZE: u32 = CID_CODEC_BASE + 203;
const CID_BITRATE_MODE: u32 = CID_CODEC_BASE + 206;
const CID_BITRATE: u32 = CID_CODEC_BASE + 207;
const CID_BITRATE_PEAK: u32 = CID_CODEC_BASE + 208;
const CID_FRAME_RC_ENABLE: u32 = CID_CODEC_BASE + 215;
const CID_HEADER_MODE: u32 = CID_CODEC_BASE + 216;
const CID_FORCE_KEY_FRAME: u32 = CID_CODEC_BASE + 229;
const CID_H264_I_FRAME_QP: u32 = CID_CODEC_BASE + 350;
const CID_H264_P_FRAME_QP: u32 = CID_CODEC_BASE + 351;
const CID_H264_LEVEL: u32 = CID_CODEC_BASE + 359;
const CID_H264_PROFILE: u32 = CID_CODEC_BASE + 363;
const CID_HEVC_I_FRAME_QP: u32 = CID_CODEC_BASE + 602;
const CID_HEVC_P_FRAME_QP: u32 = CID_CODEC_BASE + 603;
const CID_HEVC_PROFILE: u32 = CID_CODEC_BASE + 615;
const CID_HEVC_LEVEL: u32 = CID_CODEC_BASE + 616;
const CID_HEVC_TIER: u32 = CID_CODEC_BASE + 618;

const CAPTURE_BUFFERS: u32 = 4;
const STOP_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Codec {
    Hevc,
    H264,
}

impl Codec {
    fn fourcc(self) -> u32 {
        u32::from_le_bytes(match self {
            Codec::Hevc => *b"HEVC",
            Codec::H264 => *b"H264",
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
    pub qp: Option<u32>,
}

pub struct Packet<'a> {
    pub data: &'a [u8],
    pub pts_us: u64,
    pub key: bool,
}

struct Mapping {
    ptr: *mut u8,
    len: usize,
}

impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ptr.cast(), self.len) };
    }
}

pub struct Encoder {
    file: File,
    layout: Nv12Layout,
    outputs: Vec<bool>,
    captures: Vec<Mapping>,
    streaming: bool,
}

// The mmapped capture buffers are only touched from the thread that owns the encoder.
unsafe impl Send for Encoder {}

// asm-generic ioctl encoding, which is what arm64 uses.
const fn ioc(dir: u32, nr: u32, size: usize) -> libc::c_ulong {
    ((dir << 30) | ((size as u32) << 16) | ((b'V' as u32) << 8) | nr) as libc::c_ulong
}
const RW: u32 = 3;
const W: u32 = 1;
const VIDIOC_S_FMT: libc::c_ulong = ioc(RW, 5, std::mem::size_of::<v4l2::v4l2_format>());
const VIDIOC_REQBUFS: libc::c_ulong = ioc(RW, 8, std::mem::size_of::<v4l2::v4l2_requestbuffers>());
const VIDIOC_QUERYBUF: libc::c_ulong = ioc(RW, 9, std::mem::size_of::<v4l2::v4l2_buffer>());
const VIDIOC_QBUF: libc::c_ulong = ioc(RW, 15, std::mem::size_of::<v4l2::v4l2_buffer>());
const VIDIOC_DQBUF: libc::c_ulong = ioc(RW, 17, std::mem::size_of::<v4l2::v4l2_buffer>());
const VIDIOC_STREAMON: libc::c_ulong = ioc(W, 18, std::mem::size_of::<libc::c_int>());
const VIDIOC_STREAMOFF: libc::c_ulong = ioc(W, 19, std::mem::size_of::<libc::c_int>());
const VIDIOC_S_PARM: libc::c_ulong = ioc(RW, 22, std::mem::size_of::<v4l2::v4l2_streamparm>());
const VIDIOC_S_CTRL: libc::c_ulong = ioc(RW, 28, std::mem::size_of::<v4l2::v4l2_control>());
const VIDIOC_ENCODER_CMD: libc::c_ulong = ioc(RW, 77, std::mem::size_of::<EncoderCmd>());
const VIDIOC_S_SELECTION: libc::c_ulong = ioc(RW, 95, std::mem::size_of::<v4l2::v4l2_selection>());
const BUF_TYPE_OUTPUT_SINGLE: u32 = 2;
const SEL_TGT_CROP: u32 = 0;

#[repr(C)]
#[derive(Default)]
struct EncoderCmd {
    cmd: u32,
    flags: u32,
    data: [u32; 8],
}

unsafe fn xioctl<T>(fd: RawFd, req: libc::c_ulong, arg: *mut T) -> io::Result<()> {
    loop {
        if libc::ioctl(fd, req, arg) == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

fn set_crop(fd: RawFd, width: u32, height: u32) -> Result<()> {
    // Some drivers want the single planar type here even on mplane queues.
    let mut last = None;
    for ty in [BUF_TYPE_OUTPUT, BUF_TYPE_OUTPUT_SINGLE] {
        let mut sel: v4l2::v4l2_selection = unsafe { std::mem::zeroed() };
        sel.type_ = ty;
        sel.target = SEL_TGT_CROP;
        sel.r.width = width;
        sel.r.height = height;
        match unsafe { xioctl(fd, VIDIOC_S_SELECTION, &mut sel) } {
            Ok(()) if sel.r.width == width && sel.r.height == height => return Ok(()),
            Ok(()) => bail!("the encoder cropped to {}x{} instead of {width}x{height}", sel.r.width, sel.r.height),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap()).context("setting the encoder crop")
}

fn peak_bitrate(bitrate: u32) -> u32 {
    (bitrate as u64 * 3 / 2).min(245_000_000) as u32
}

/// Lowest HEVC level (V4L2 menu index) and tier that fit the stream.
/// Limits from H.265 Annex A: luma picture size, luma samples per second,
/// and Main / High tier bitrate in kbit/s.
fn hevc_level(width: u32, height: u32, fps: u32, peak_kbps: u32) -> (i32, bool) {
    const LEVELS: [(i32, u64, u64, u32, u32); 7] = [
        (4, 2_228_224, 66_846_720, 12_000, 30_000),        // 4
        (6, 2_228_224, 133_693_440, 20_000, 50_000),       // 4.1
        (7, 8_912_896, 267_386_880, 25_000, 100_000),      // 5
        (8, 8_912_896, 534_773_760, 40_000, 160_000),      // 5.1
        (9, 8_912_896, 1_069_547_520, 60_000, 240_000),    // 5.2
        (10, 35_651_584, 1_069_547_520, 60_000, 240_000),  // 6
        (11, 35_651_584, 2_139_095_040, 120_000, 480_000), // 6.1
    ];
    let size = width as u64 * height as u64;
    let rate = size * fps as u64;
    for &(level, max_size, max_rate, main, high) in &LEVELS {
        if size <= max_size && rate <= max_rate {
            if peak_kbps <= main {
                return (level, false);
            }
            if peak_kbps <= high {
                return (level, true);
            }
        }
    }
    (11, true)
}

/// Lowest H.264 level (V4L2 menu index) that fits the stream. Limits from
/// H.264 Annex A in macroblocks, with High profile's 1.25x bitrate allowance.
fn h264_level(width: u32, height: u32, fps: u32, peak_kbps: u32) -> i32 {
    const LEVELS: [(i32, u64, u64, u32); 5] = [
        (12, 8_192, 245_760, 62_500),     // 4.1
        (13, 8_704, 522_240, 62_500),     // 4.2
        (14, 22_080, 589_824, 168_750),   // 5
        (15, 36_864, 983_040, 300_000),   // 5.1
        (16, 36_864, 2_073_600, 300_000), // 5.2
    ];
    let mbs = width.div_ceil(16) as u64 * height.div_ceil(16) as u64;
    let rate = mbs * fps as u64;
    LEVELS
        .iter()
        .find(|&&(_, max_fs, max_rate, max_kbps)| mbs <= max_fs && rate <= max_rate && peak_kbps <= max_kbps)
        .map_or(16, |l| l.0)
}

/// Finds the hardware encoder node.
pub fn find_device() -> Result<PathBuf> {
    let link = Path::new("/dev/video-enc0");
    if link.exists() {
        return Ok(link.into());
    }
    for entry in std::fs::read_dir("/sys/class/video4linux")? {
        let entry = entry?;
        let name = std::fs::read_to_string(entry.path().join("name")).unwrap_or_default();
        if name.contains("encoder") {
            return Ok(Path::new("/dev").join(entry.file_name()));
        }
    }
    bail!("no hardware video encoder found")
}

impl Encoder {
    pub fn open(path: &Path, cfg: Config, output_buffers: u32) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .with_context(|| format!("opening encoder {}", path.display()))?;
        let fd = file.as_raw_fd();

        unsafe {
            // Coded format first, the raw side's constraints depend on it.
            let mut fmt: v4l2::v4l2_format = std::mem::zeroed();
            fmt.type_ = BUF_TYPE_CAPTURE;
            fmt.fmt.pix_mp.width = cfg.width;
            fmt.fmt.pix_mp.height = cfg.height;
            fmt.fmt.pix_mp.pixelformat = cfg.codec.fourcc();
            fmt.fmt.pix_mp.field = FIELD_NONE;
            fmt.fmt.pix_mp.num_planes = 1;
            xioctl(fd, VIDIOC_S_FMT, &mut fmt).context("setting coded format")?;
            if fmt.fmt.pix_mp.pixelformat != cfg.codec.fourcc() {
                bail!("the encoder doesn't do {:?}", cfg.codec);
            }

            let mut fmt: v4l2::v4l2_format = std::mem::zeroed();
            fmt.type_ = BUF_TYPE_OUTPUT;
            let pix = &mut fmt.fmt.pix_mp;
            pix.width = cfg.width;
            pix.height = cfg.height;
            pix.pixelformat = u32::from_le_bytes(*b"NV12");
            pix.field = FIELD_NONE;
            pix.num_planes = 1;
            pix.colorspace = COLORSPACE_REC709;
            pix.__bindgen_anon_1.ycbcr_enc = YCBCR_ENC_709;
            pix.quantization = QUANTIZATION_LIM_RANGE;
            pix.xfer_func = XFER_FUNC_709;
            xioctl(fd, VIDIOC_S_FMT, &mut fmt).context("setting raw format")?;
            let pix = fmt.fmt.pix_mp;
            let (got_w, got_h) = (pix.width, pix.height);
            if got_w < cfg.width || got_h < cfg.height {
                bail!("the encoder wants {got_w}x{got_h} instead of {}x{}", cfg.width, cfg.height);
            }
            if (got_w, got_h) != (cfg.width, cfg.height) {
                // Buffers are padded to the macroblock size, the crop says what's real.
                set_crop(fd, cfg.width, cfg.height)?;
            }

            let plane = pix.plane_fmt[0];
            let (y_stride, size) = (plane.bytesperline, plane.sizeimage);
            // Venus/iris NV12: luma rows padded to 32, chroma follows right after.
            let uv_offset = y_stride * got_h.next_multiple_of(32);
            let expected = uv_offset + y_stride * (got_h / 2).next_multiple_of(16);
            if y_stride % 4 != 0 || uv_offset % 4 != 0 || expected > size {
                bail!("unexpected NV12 layout from the encoder: stride {y_stride}, size {size}");
            }
            log::info!("encoder input: NV12 stride {y_stride}, chroma at {uv_offset}, {size} bytes");

            let layout = Nv12Layout { width: cfg.width, height: cfg.height, y_stride, uv_offset, size };

            for ty in [BUF_TYPE_OUTPUT, BUF_TYPE_CAPTURE] {
                let mut parm: v4l2::v4l2_streamparm = std::mem::zeroed();
                parm.type_ = ty;
                // Same offset for capture and output.
                parm.parm.output.timeperframe.numerator = 1;
                parm.parm.output.timeperframe.denominator = cfg.fps;
                if let Err(e) = xioctl(fd, VIDIOC_S_PARM, &mut parm) {
                    log::warn!("couldn't set frame rate on queue {ty}: {e}");
                }
            }

            let mut enc = Self { file, layout, outputs: Vec::new(), captures: Vec::new(), streaming: false };
            enc.set_controls(&cfg);
            enc.setup_buffers(output_buffers)?;
            Ok(enc)
        }
    }

    fn set_ctrl(&self, name: &str, id: u32, value: i32) {
        let mut ctrl = v4l2::v4l2_control { id, value };
        if let Err(e) = unsafe { xioctl(self.file.as_raw_fd(), VIDIOC_S_CTRL, &mut ctrl) } {
            log::warn!("encoder ignored {name}={value}: {e}");
        }
    }

    fn set_controls(&self, cfg: &Config) {
        let gop = (cfg.fps * 2) as i32;
        self.set_ctrl("b_frames", CID_B_FRAMES, 0);
        self.set_ctrl("gop_size", CID_GOP_SIZE, gop);
        self.set_ctrl("header_mode", CID_HEADER_MODE, 0);

        // With a fixed quantizer the bitrate is whatever it turns out to be,
        // so leave room for the top of the range.
        let peak_kbps = if cfg.qp.is_some() { 160_000 } else { peak_bitrate(cfg.bitrate) / 1000 };
        match cfg.codec {
            Codec::Hevc => {
                let (level, high_tier) = hevc_level(cfg.width, cfg.height, cfg.fps, peak_kbps);
                self.set_ctrl("hevc_profile", CID_HEVC_PROFILE, 0); // Main
                self.set_ctrl("hevc_tier", CID_HEVC_TIER, high_tier as i32);
                self.set_ctrl("hevc_level", CID_HEVC_LEVEL, level);
            }
            Codec::H264 => {
                self.set_ctrl("h264_profile", CID_H264_PROFILE, 4); // High
                self.set_ctrl("h264_level", CID_H264_LEVEL, h264_level(cfg.width, cfg.height, cfg.fps, peak_kbps));
            }
        }

        match cfg.qp {
            Some(qp) => {
                self.set_ctrl("frame_rc", CID_FRAME_RC_ENABLE, 0);
                let (i, p) = match cfg.codec {
                    Codec::Hevc => (CID_HEVC_I_FRAME_QP, CID_HEVC_P_FRAME_QP),
                    Codec::H264 => (CID_H264_I_FRAME_QP, CID_H264_P_FRAME_QP),
                };
                self.set_ctrl("i_qp", i, qp as i32);
                self.set_ctrl("p_qp", p, qp as i32);
            }
            None => {
                self.set_ctrl("frame_rc", CID_FRAME_RC_ENABLE, 1);
                self.set_ctrl("bitrate_mode", CID_BITRATE_MODE, 0); // VBR
                self.set_ctrl("bitrate", CID_BITRATE, cfg.bitrate as i32);
                self.set_ctrl("bitrate_peak", CID_BITRATE_PEAK, peak_bitrate(cfg.bitrate) as i32);
            }
        }
    }

    unsafe fn setup_buffers(&mut self, output_buffers: u32) -> Result<()> {
        let fd = self.file.as_raw_fd();

        let mut req: v4l2::v4l2_requestbuffers = std::mem::zeroed();
        req.count = output_buffers;
        req.type_ = BUF_TYPE_OUTPUT;
        req.memory = MEMORY_DMABUF;
        xioctl(fd, VIDIOC_REQBUFS, &mut req).context("allocating encoder input slots")?;
        if req.count < output_buffers {
            bail!("encoder only gave us {} input slots", req.count);
        }
        self.outputs = vec![false; output_buffers as usize];

        let mut req: v4l2::v4l2_requestbuffers = std::mem::zeroed();
        req.count = CAPTURE_BUFFERS;
        req.type_ = BUF_TYPE_CAPTURE;
        req.memory = MEMORY_MMAP;
        xioctl(fd, VIDIOC_REQBUFS, &mut req).context("allocating encoder output buffers")?;

        for index in 0..req.count {
            let mut plane: v4l2::v4l2_plane = std::mem::zeroed();
            let mut buf: v4l2::v4l2_buffer = std::mem::zeroed();
            buf.index = index;
            buf.type_ = BUF_TYPE_CAPTURE;
            buf.memory = MEMORY_MMAP;
            buf.length = 1;
            buf.m.planes = &mut plane;
            xioctl(fd, VIDIOC_QUERYBUF, &mut buf).context("querying encoder output buffer")?;

            let len = plane.length as usize;
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd,
                plane.m.mem_offset as libc::off_t,
            );
            if ptr == libc::MAP_FAILED {
                return Err(io::Error::last_os_error()).context("mapping encoder output buffer");
            }
            self.captures.push(Mapping { ptr: ptr.cast(), len });
            self.queue_capture(index)?;
        }
        log::debug!("encoder: {} input slots, {} output buffers", self.outputs.len(), self.captures.len());

        for ty in [BUF_TYPE_OUTPUT, BUF_TYPE_CAPTURE] {
            let mut t = ty as libc::c_int;
            xioctl(fd, VIDIOC_STREAMON, &mut t).context("starting the encoder")?;
        }
        self.streaming = true;
        Ok(())
    }

    fn queue_capture(&self, index: u32) -> Result<()> {
        unsafe {
            let mut plane: v4l2::v4l2_plane = std::mem::zeroed();
            plane.length = self.captures[index as usize].len as u32;
            let mut buf: v4l2::v4l2_buffer = std::mem::zeroed();
            buf.index = index;
            buf.type_ = BUF_TYPE_CAPTURE;
            buf.memory = MEMORY_MMAP;
            buf.length = 1;
            buf.m.planes = &mut plane;
            xioctl(self.file.as_raw_fd(), VIDIOC_QBUF, &mut buf).context("queueing encoder output buffer")
        }
    }

    /// Makes the next frame a keyframe, so a recording started mid-session
    /// doesn't wait for the next one.
    pub fn force_keyframe(&self) {
        self.set_ctrl("force_key_frame", CID_FORCE_KEY_FRAME, 1);
    }

    pub fn layout(&self) -> Nv12Layout {
        self.layout
    }

    /// An input slot the GPU can write into, or None if the encoder is
    /// still chewing on all of them.
    pub fn free_slot(&mut self) -> Result<Option<usize>> {
        self.reclaim_outputs()?;
        Ok(self.outputs.iter().position(|busy| !busy))
    }

    fn reclaim_outputs(&mut self) -> Result<()> {
        loop {
            let mut plane: v4l2::v4l2_plane = unsafe { std::mem::zeroed() };
            let mut buf: v4l2::v4l2_buffer = unsafe { std::mem::zeroed() };
            buf.type_ = BUF_TYPE_OUTPUT;
            buf.memory = MEMORY_DMABUF;
            buf.length = 1;
            buf.m.planes = &mut plane;
            match unsafe { xioctl(self.file.as_raw_fd(), VIDIOC_DQBUF, &mut buf) } {
                Ok(()) => self.outputs[buf.index as usize] = false,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.raw_os_error() == Some(libc::EPIPE) => return Ok(()),
                Err(e) => return Err(e).context("reclaiming encoder input"),
            }
        }
    }

    pub fn queue_frame(&mut self, slot: usize, dmabuf: RawFd, pts_us: u64) -> Result<()> {
        unsafe {
            let mut plane: v4l2::v4l2_plane = std::mem::zeroed();
            plane.bytesused = self.layout.size;
            plane.length = self.layout.size;
            plane.m.fd = dmabuf;
            let mut buf: v4l2::v4l2_buffer = std::mem::zeroed();
            buf.index = slot as u32;
            buf.type_ = BUF_TYPE_OUTPUT;
            buf.memory = MEMORY_DMABUF;
            buf.field = FIELD_NONE;
            buf.length = 1;
            buf.m.planes = &mut plane;
            buf.timestamp.tv_sec = (pts_us / 1_000_000) as _;
            buf.timestamp.tv_usec = (pts_us % 1_000_000) as _;
            xioctl(self.file.as_raw_fd(), VIDIOC_QBUF, &mut buf).context("feeding the encoder")?;
        }
        self.outputs[slot] = true;
        Ok(())
    }

    /// Hands every finished packet to `sink`. Returns true once the encoder
    /// has sent its last packet after `finish`.
    pub fn poll_packets(&mut self, mut sink: impl FnMut(Packet)) -> Result<bool> {
        loop {
            let mut plane: v4l2::v4l2_plane = unsafe { std::mem::zeroed() };
            let mut buf: v4l2::v4l2_buffer = unsafe { std::mem::zeroed() };
            buf.type_ = BUF_TYPE_CAPTURE;
            buf.memory = MEMORY_MMAP;
            buf.length = 1;
            buf.m.planes = &mut plane;
            match unsafe { xioctl(self.file.as_raw_fd(), VIDIOC_DQBUF, &mut buf) } {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(e) if e.raw_os_error() == Some(libc::EPIPE) => return Ok(true),
                Err(e) => return Err(e).context("reading from the encoder"),
            }

            let map = &self.captures[buf.index as usize];
            let start = (plane.data_offset as usize).min(map.len);
            let end = (plane.bytesused as usize).min(map.len);
            if buf.flags & BUF_FLAG_ERROR != 0 {
                log::warn!("encoder flagged a corrupt packet, dropping it");
            } else if end > start {
                let data = unsafe { std::slice::from_raw_parts(map.ptr.add(start), end - start) };
                sink(Packet {
                    data,
                    pts_us: buf.timestamp.tv_sec as u64 * 1_000_000 + buf.timestamp.tv_usec as u64,
                    key: buf.flags & BUF_FLAG_KEYFRAME != 0,
                });
            }

            if buf.flags & BUF_FLAG_LAST != 0 {
                return Ok(true);
            }
            self.queue_capture(buf.index)?;
        }
    }

    /// Flushes everything still inside the encoder.
    pub fn finish(&mut self, mut sink: impl FnMut(Packet)) -> Result<()> {
        let mut cmd = EncoderCmd { cmd: ENC_CMD_STOP, ..Default::default() };
        if let Err(e) = unsafe { xioctl(self.file.as_raw_fd(), VIDIOC_ENCODER_CMD, &mut cmd) } {
            log::warn!("encoder refused to drain: {e}");
            return Ok(());
        }
        let deadline = Instant::now() + STOP_TIMEOUT;
        while Instant::now() < deadline {
            if self.poll_packets(&mut sink)? {
                return Ok(());
            }
            let mut pfd = libc::pollfd { fd: self.file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            unsafe { libc::poll(&mut pfd, 1, 50) };
        }
        log::warn!("encoder didn't finish draining in time, the last frames may be missing");
        Ok(())
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        if self.streaming {
            for ty in [BUF_TYPE_OUTPUT, BUF_TYPE_CAPTURE] {
                let mut t = ty as libc::c_int;
                let _ = unsafe { xioctl(self.file.as_raw_fd(), VIDIOC_STREAMOFF, &mut t) };
            }
        }
        self.captures.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hevc_levels() {
        // A 60 Mbit/s peak is past 4.1 High's 50, so level 5 High. 30 fits 4.1 High.
        assert_eq!(hevc_level(1920, 1080, 60, 60_000), (7, true));
        assert_eq!(hevc_level(1920, 1080, 60, 30_000), (6, true));
        assert_eq!(hevc_level(1920, 1080, 60, 15_000), (6, false));
        // 1080p72 needs level 5 for the sample rate alone.
        assert_eq!(hevc_level(1920, 1080, 72, 20_000), (7, false));
        // Square 1440 at 30 fps is light enough for level 4.
        assert_eq!(hevc_level(1440, 1440, 30, 10_000), (4, false));
    }

    #[test]
    fn h264_levels() {
        assert_eq!(h264_level(1920, 1080, 60, 60_000), 13);
        assert_eq!(h264_level(1920, 1080, 72, 60_000), 14);
        assert_eq!(h264_level(1920, 1080, 30, 120_000), 14);
    }
}

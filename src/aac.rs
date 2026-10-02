//! AAC encoding for one audio track. Samples come in at timeline positions,
//! small timing wobbles get bridged, real gaps become silence, and encoded
//! packets come back out for whoever is writing files.

use std::ptr;
use std::sync::Arc;

use anyhow::{bail, Result};
use ffmpeg_sys_next as ff;

use crate::audio::{CHANNELS, RATE};
use crate::writer::{check, Packet, Params};

/// Audio further than this from where we expect it gets resynced by padding
/// silence or dropping samples, instead of drifting out of sync.
const RESYNC_FRAMES: i64 = RATE as i64 / 20;

pub struct Track {
    /// Stream index in the files: 0 is video, audio tracks follow.
    stream: usize,
    ctx: *mut ff::AVCodecContext,
    frame: *mut ff::AVFrame,
    packet: *mut ff::AVPacket,
    /// Interleaved samples waiting to fill an encoder frame.
    pending: Vec<f32>,
    /// Sample position of `pending[0]`.
    next_sample: i64,
    started: bool,
}

impl Track {
    pub fn new(stream: usize, bitrate: u32) -> Result<Self> {
        unsafe {
            let codec = ff::avcodec_find_encoder(ff::AVCodecID::AV_CODEC_ID_AAC);
            if codec.is_null() {
                bail!("this FFmpeg has no AAC encoder");
            }
            let t = Self {
                stream,
                ctx: ff::avcodec_alloc_context3(codec),
                frame: ff::av_frame_alloc(),
                packet: ff::av_packet_alloc(),
                pending: Vec::new(),
                next_sample: 0,
                started: false,
            };
            let ctx = t.ctx;
            (*ctx).sample_fmt = ff::AVSampleFormat::AV_SAMPLE_FMT_FLTP;
            (*ctx).sample_rate = RATE as i32;
            ff::av_channel_layout_default(&mut (*ctx).ch_layout, CHANNELS as i32);
            (*ctx).bit_rate = bitrate as i64;
            (*ctx).time_base = ff::AVRational { num: 1, den: RATE as i32 };
            // Every file we write is MP4, which wants the config up front.
            (*ctx).flags |= ff::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
            // The fast coder costs a fraction of the default one and sounds the same at these bitrates.
            let mut opts = ptr::null_mut();
            ff::av_dict_set(&mut opts, c"aac_coder".as_ptr(), c"fast".as_ptr(), 0);
            let ret = ff::avcodec_open2(ctx, codec, &mut opts);
            ff::av_dict_free(&mut opts);
            check(ret, "opening the AAC encoder")?;

            let frame = t.frame;
            (*frame).format = ff::AVSampleFormat::AV_SAMPLE_FMT_FLTP as i32;
            (*frame).sample_rate = RATE as i32;
            (*frame).nb_samples = (*ctx).frame_size;
            check(ff::av_channel_layout_copy(&mut (*frame).ch_layout, &(*ctx).ch_layout), "audio frame setup")?;
            check(ff::av_frame_get_buffer(frame, 0), "allocating audio frame")?;
            Ok(t)
        }
    }

    /// What a file needs to know about this track's stream.
    pub fn params(&self) -> Result<Params> {
        Params::from_encoder(self.ctx)
    }

    /// Timeline position right after the last sample handed in.
    pub fn end(&self) -> i64 {
        self.next_sample + (self.pending.len() / CHANNELS as usize) as i64
    }

    /// Queues samples that start at timeline frame `pos`, bridging small
    /// timing wobbles and filling real gaps.
    pub fn place(&mut self, pos: i64, samples: &[f32], out: &mut Vec<Packet>) -> Result<()> {
        let ch = CHANNELS as usize;
        let mut samples = samples;
        let frames = (samples.len() / ch) as i64;
        let mut gap = 0usize;

        if !self.started {
            if pos + frames <= 0 {
                return Ok(());
            }
            let skip = (-pos).max(0);
            samples = &samples[skip as usize * ch..];
            // Tracks always start at zero; if nothing played at first, that
            // stretch is silence.
            self.next_sample = 0;
            self.started = true;
            gap = pos.max(0) as usize;
        } else {
            let drift = pos - self.end();
            if drift > RESYNC_FRAMES {
                log::debug!("audio stream {}: gap of {drift} samples, filling with silence", self.stream);
                gap = drift as usize;
            } else if drift < -RESYNC_FRAMES {
                let drop = (-drift).min(frames);
                log::debug!("audio stream {}: ran ahead by {} samples, dropping", self.stream, -drift);
                samples = &samples[drop as usize * ch..];
            }
        }

        self.silence(gap, out)?;
        self.pending.extend_from_slice(samples);
        self.encode(false, out)
    }

    /// Pads silence up to timeline frame `pos`, for a source that went quiet
    /// (an idle speaker sink sends nothing at all).
    pub fn fill_to(&mut self, pos: i64, out: &mut Vec<Packet>) -> Result<()> {
        self.started = true;
        let gap = (pos - self.end()).max(0) as usize;
        self.silence(gap, out)
    }

    /// Encodes whatever is left. The track can't take more samples after this.
    pub fn flush(&mut self, out: &mut Vec<Packet>) -> Result<()> {
        self.encode(true, out)
    }

    fn silence(&mut self, mut frames: usize, out: &mut Vec<Packet>) -> Result<()> {
        // A sink that sat idle for an hour is a big gap; fill it a second at
        // a time so it never piles up in memory.
        while frames > 0 {
            let n = frames.min(RATE as usize);
            self.pending.resize(self.pending.len() + n * CHANNELS as usize, 0.0);
            frames -= n;
            self.encode(false, out)?;
        }
        Ok(())
    }

    fn encode(&mut self, flush: bool, out: &mut Vec<Packet>) -> Result<()> {
        let ch = CHANNELS as usize;
        unsafe {
            let frame_size = (*self.ctx).frame_size as usize;
            loop {
                let available = self.pending.len() / ch;
                let n = if available >= frame_size {
                    frame_size
                } else if flush && available > 0 {
                    available
                } else {
                    break;
                };
                check(ff::av_frame_make_writable(self.frame), "audio frame")?;
                (*self.frame).nb_samples = n as i32;
                for c in 0..ch {
                    let plane = std::slice::from_raw_parts_mut((*self.frame).data[c] as *mut f32, n);
                    for (i, s) in plane.iter_mut().enumerate() {
                        *s = self.pending[i * ch + c];
                    }
                }
                (*self.frame).pts = self.next_sample;
                self.next_sample += n as i64;
                self.pending.drain(..n * ch);
                check(ff::avcodec_send_frame(self.ctx, self.frame), "encoding audio")?;
                self.drain(out)?;
            }
            if flush {
                check(ff::avcodec_send_frame(self.ctx, ptr::null()), "finishing audio")?;
                self.drain(out)?;
            }
        }
        Ok(())
    }

    unsafe fn drain(&mut self, out: &mut Vec<Packet>) -> Result<()> {
        loop {
            let ret = ff::avcodec_receive_packet(self.ctx, self.packet);
            if ret == ff::AVERROR(libc::EAGAIN) || ret == ff::AVERROR_EOF {
                return Ok(());
            }
            check(ret, "encoding audio")?;
            let p = &*self.packet;
            out.push(Packet {
                stream: self.stream,
                pts: p.pts,
                duration: p.duration,
                key: true,
                data: crate::writer::Payload::Mem(Arc::from(std::slice::from_raw_parts(p.data, p.size as usize))),
            });
            ff::av_packet_unref(self.packet);
        }
    }
}

impl Drop for Track {
    fn drop(&mut self) {
        unsafe {
            ff::av_packet_free(&mut self.packet);
            ff::av_frame_free(&mut self.frame);
            ff::avcodec_free_context(&mut self.ctx);
        }
    }
}

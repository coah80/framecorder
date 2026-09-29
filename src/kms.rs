//! Finds the plane the VR compositor scans out and hands its buffers over as
//! dmabufs. The compositor keeps DRM master; we only look.
//!
//! Getting buffer handles for someone else's framebuffer needs CAP_SYS_ADMIN,
//! which is the one privilege this tool needs.

use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use drm::control::{crtc, framebuffer, plane, Device as ControlDevice};
use drm::{ClientCapability, Device, VblankWaitFlags, VblankWaitTarget};

struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl Device for Card {}
impl ControlDevice for Card {}

/// One scanout buffer, exported as a dmabuf.
pub struct ScanoutBuffer {
    pub fb_id: u32,
    pub fd: OwnedFd,
    pub width: u32,
    pub height: u32,
    pub fourcc: u32,
    pub modifier: u64,
    pub pitch: u32,
    pub offset: u32,
}

pub struct Kms {
    card: Card,
    pipe: u32,
    plane: plane::Handle,
    pub refresh_hz: f64,
    pub mode_size: (u32, u32),
}

impl Kms {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        let card = Card(file);
        card.set_client_capability(ClientCapability::UniversalPlanes, true)
            .context("enabling universal planes")?;

        let res = card.resource_handles().context("reading DRM resources")?;
        let (pipe, crtc, mode) = res
            .crtcs()
            .iter()
            .enumerate()
            .find_map(|(i, &h)| {
                let info = card.get_crtc(h).ok()?;
                Some((i as u32, h, info.mode()?))
            })
            .context("no active display found, is the headset on?")?;

        let plane = find_scanout_plane(&card, crtc)?;
        let (w, h) = mode.size();
        let refresh_hz = mode_refresh(&mode);
        log::info!(
            "display: {}x{} @ {:.2} Hz, crtc {:?}, plane {:?}",
            w,
            h,
            refresh_hz,
            crtc,
            plane
        );

        Ok(Self {
            card,
            pipe,
            plane,
            refresh_hz,
            mode_size: (w as u32, h as u32),
        })
    }

    /// Blocks until the next vblank and returns its CLOCK_MONOTONIC timestamp.
    pub fn wait_vblank(&self) -> Result<Duration> {
        let reply = loop {
            match self.card.wait_vblank(VblankWaitTarget::Relative(1), VblankWaitFlags::empty(), self.pipe, 0) {
                // A pause or stop signal landed mid-wait; just wait again.
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                other => break other.context("waiting for vblank")?,
            }
        };
        Ok(reply.time().unwrap_or_else(crate::clock::now))
    }

    /// Id of the framebuffer being scanned out right now, if any.
    pub fn current_fb(&self) -> Result<Option<u32>> {
        let info = self.card.get_plane(self.plane).context("reading scanout plane")?;
        Ok(info.framebuffer().map(u32::from))
    }

    pub fn export(&self, fb_id: u32) -> Result<ScanoutBuffer> {
        let handle: framebuffer::Handle =
            drm::control::from_u32(fb_id).context("bad framebuffer id")?;
        let info = self
            .card
            .get_planar_framebuffer(handle)
            .with_context(|| format!("reading framebuffer {fb_id}"))?;

        let buffers = info.buffers();
        let Some(bo) = buffers[0] else {
            bail!(
                "the kernel hid the scanout buffer from us. framecorder needs CAP_SYS_ADMIN, run:\n  \
                 sudo setcap cap_sys_admin+ep {}",
                std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()
            );
        };
        if buffers[1..].iter().any(Option::is_some) {
            bail!("multi-plane scanout buffers aren't supported yet");
        }

        let fd = self
            .card
            .buffer_to_prime_fd(bo, libc::O_CLOEXEC as u32)
            .context("exporting scanout buffer as dmabuf");
        // The export holds its own reference, the GEM handle is ours to drop.
        let _ = self.card.close_buffer(bo);
        let (width, height) = info.size();

        Ok(ScanoutBuffer {
            fb_id,
            fd: fd?,
            width,
            height,
            fourcc: info.pixel_format() as u32,
            modifier: info.modifier().map(u64::from).unwrap_or(0),
            pitch: info.pitches()[0],
            offset: info.offsets()[0],
        })
    }
}

fn find_scanout_plane(card: &Card, crtc: crtc::Handle) -> Result<plane::Handle> {
    let mut best: Option<(plane::Handle, u64)> = None;
    for handle in card.plane_handles().context("listing planes")? {
        let Ok(info) = card.get_plane(handle) else { continue };
        if info.crtc() != Some(crtc) {
            continue;
        }
        let Some(fb) = info.framebuffer() else { continue };
        // Biggest buffer wins, that's the compositor's output rather than a cursor.
        let area = card
            .get_framebuffer(fb)
            .map(|f| f.size().0 as u64 * f.size().1 as u64)
            .unwrap_or(0);
        if best.is_none_or(|(_, a)| area > a) {
            best = Some((handle, area));
        }
    }
    best.map(|(h, _)| h).context("no plane is scanning out, is the VR compositor running?")
}

fn mode_refresh(mode: &drm::control::Mode) -> f64 {
    let (htotal, vtotal) = (mode.hsync().2 as f64, mode.vsync().2 as f64);
    if htotal > 0.0 && vtotal > 0.0 {
        mode.clock() as f64 * 1000.0 / (htotal * vtotal)
    } else {
        mode.vrefresh() as f64
    }
}

/// Identifies the buffer behind a dmabuf fd. Every export of the same buffer
/// shares one dma_buf file, so its inode is stable for the buffer's lifetime.
pub fn buffer_id(fd: &OwnedFd) -> Result<u64> {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut st) } != 0 {
        return Err(std::io::Error::last_os_error()).context("looking at a scanout buffer");
    }
    Ok(st.st_ino)
}

/// Waits (briefly) until nobody is still drawing into the buffer.
pub fn wait_idle(fd: &OwnedFd, timeout: Duration) {
    let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut pfd, 1, timeout.as_millis() as libc::c_int) };
}

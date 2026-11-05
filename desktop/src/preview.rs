//! the small picture on the live page. gpui can't decode video, so ffmpeg
//! (the same one the thumbnails use) reads the local stream and hands over a
//! few small frames a second, only while the page is up.

use std::io::Read;
use std::process::{Child, Stdio};
use std::sync::Arc;

use gpui::RenderImage;
use image::{Frame, RgbaImage};
use smallvec::SmallVec;

use crate::thumbs;

/// the picture's size, 16:9. a tall or square stream sits in the middle of it,
/// the rest is see-through
pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 270;
/// frames a second. the decoder still decodes every frame the stream has, this
/// is only how many get scaled and handed over
pub const FPS: u32 = 5;

/// what the picture is up to, in the page's words
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Note {
    /// nothing's been decoded yet
    Waiting,
    Showing,
    /// ffmpeg gave up or wasn't there
    Gone,
}

/// the biggest fullscreen picture, so a 4k screen doesn't mean copying 4k
/// frames: casting goes out at 1080p anyway
pub const FULL_MAX: (u32, u32) = (1920, 1080);

/// how big and how often the pictures come
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Size {
    pub width: u32,
    pub height: u32,
    /// None is every frame the stream has
    pub fps: Option<u32>,
}

/// the small picture on the page
pub const SMALL: Size = Size { width: WIDTH, height: HEIGHT, fps: Some(FPS) };

/// a screen's pixels, shrunk to fit `FULL_MAX` (turned to match a portrait
/// screen), even sized for the scaler
pub fn full(width: u32, height: u32) -> Size {
    let (max_w, max_h) = if height > width { (FULL_MAX.1, FULL_MAX.0) } else { FULL_MAX };
    let scale = (max_w as f32 / width.max(1) as f32).min(max_h as f32 / height.max(1) as f32).min(1.0);
    let even = |n: f32| ((n as u32) & !1).max(2);
    Size { width: even(width as f32 * scale), height: even(height as f32 * scale), fps: None }
}

/// one ffmpeg, reading the stream until it's dropped
pub struct Decoder {
    child: Child,
    pub size: Size,
}

impl Decoder {
    /// starts decoding `url` into pictures of `size`, handing each to
    /// `deliver` on a thread of its own. `deliver` returning false stops it.
    /// the first frame takes a moment: ffmpeg waits for a keyframe
    pub fn start(url: &str, size: Size, deliver: impl Fn(Arc<RenderImage>) -> bool + Send + 'static) -> std::io::Result<Self> {
        let Size { width, height, fps } = size;
        let rate = fps.map(|f| format!("fps={f},")).unwrap_or_default();
        let filter = format!(
            "{rate}scale={width}:{height}:force_original_aspect_ratio=decrease,format=bgra,pad={width}:{height}:-1:-1:color=black@0"
        );
        let mut child = thumbs::ffmpeg()
            .args(["-loglevel", "error", "-nostdin", "-fflags", "nobuffer", "-flags", "low_delay", "-an", "-i", url])
            .args(["-vf", &filter, "-f", "rawvideo", "-pix_fmt", "bgra", "-"])
            .stdout(Stdio::piped())
            .spawn()?;
        let Some(mut out) = child.stdout.take() else {
            let _ = child.kill();
            return Err(std::io::Error::other("ffmpeg has no output"));
        };
        std::thread::Builder::new().name("live preview".into()).spawn(move || {
            let mut buf = vec![0u8; (width * height * 4) as usize];
            while out.read_exact(&mut buf).is_ok() {
                let Some(frame) = frame(&buf, width, height) else { break };
                if !deliver(frame) {
                    break;
                }
            }
        })?;
        Ok(Self { child, size })
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// one decoded picture, bgra as gpui keeps them
fn frame(bgra: &[u8], width: u32, height: u32) -> Option<Arc<RenderImage>> {
    let image = RgbaImage::from_raw(width, height, bgra.to_vec())?;
    Some(Arc::new(RenderImage::new(SmallVec::from_elem(Frame::new(image), 1))))
}

/// a made up frame for the demo: a dusk sky over a dark floor, with a window
/// of light in it, so the page can be looked at without a headset
pub fn demo_frame() -> Arc<RenderImage> {
    let mut image = RgbaImage::new(WIDTH, HEIGHT);
    for (x, y, p) in image.enumerate_pixels_mut() {
        let fy = y as f32 / HEIGHT as f32;
        let fx = x as f32 / WIDTH as f32;
        let horizon = 0.62;
        let (b, g, r) = if fy < horizon {
            let t = fy / horizon;
            (0x3a as f32 + 60. * t, 0x1e as f32 + 20. * t, 0x24 as f32 + 70. * t)
        } else {
            let t = (fy - horizon) / (1. - horizon);
            (0x22 as f32 - 10. * t, 0x1c as f32 - 6. * t, 0x1c as f32 - 6. * t)
        };
        let lit = (0.56..0.74).contains(&fx) && (0.3..0.62).contains(&fy);
        let (b, g, r) = if lit { (b + 90., g + 70., r + 110.) } else { (b, g, r) };
        *p = image::Rgba([b.min(255.) as u8, g.min(255.) as u8, r.min(255.) as u8, 255]);
    }
    Arc::new(RenderImage::new(SmallVec::from_elem(Frame::new(image), 1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// the ffmpeg line hands over whole frames of the size the page draws, and
    /// a tall stream comes centered with see-through sides, not stretched
    #[test]
    fn frames_arrive_at_the_pages_size_whatever_the_streams_shape() {
        let dir = std::env::temp_dir().join(format!("framecorder-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tall = dir.join("tall.ts");
        let made = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i", "color=c=white:size=90x160:rate=10", "-t", "1", "-c:v", "libx264"])
            .args(["-preset", "ultrafast", "-pix_fmt", "yuv420p", "-f", "mpegts"])
            .arg(&tall)
            .status()
            .expect("ffmpeg makes the fixture");
        assert!(made.success());
        let (tx, rx) = mpsc::channel();
        let decoder = Decoder::start(tall.to_str().unwrap(), SMALL, move |f| tx.send(f).is_ok()).unwrap();
        let frame = rx.recv_timeout(Duration::from_secs(20)).expect("a frame within 20 s");
        let bytes = frame.as_bytes(0).unwrap();
        assert_eq!(bytes.len(), (WIDTH * HEIGHT * 4) as usize);
        let px = |x: u32, y: u32| &bytes[((y * WIDTH + x) * 4) as usize..][..4];
        // the middle shows the white picture, the sides are see-through padding
        assert_eq!(px(WIDTH / 2, HEIGHT / 2)[3], 255);
        assert!(px(WIDTH / 2, HEIGHT / 2)[0] > 200);
        assert_eq!(px(10, HEIGHT / 2)[3], 0);
        assert_eq!(px(WIDTH - 10, HEIGHT / 2)[3], 0);
        drop(decoder);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// fullscreen pictures fit the screen but never go past 1080p, either way up
    #[test]
    fn fullscreen_size_fits_the_screen_up_to_1080p() {
        let size = |w, h| (full(w, h).width, full(w, h).height);
        assert_eq!(size(3840, 2160), (1920, 1080));
        assert_eq!(size(2880, 1800), (1728, 1080));
        assert_eq!(size(1366, 768), (1366, 768));
        assert_eq!(size(1080, 2400), (864, 1920));
        assert_eq!(full(1280, 720).fps, None);
    }
}

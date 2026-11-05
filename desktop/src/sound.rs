//! the live view's sound, while it's fullscreen: ffmpeg decodes the stream's
//! audio to plain samples at the speakers' own rate, and cpal plays them. the
//! small picture on the page stays quiet, obs may be listening already.

use std::collections::VecDeque;
use std::io::Read;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};

use crate::thumbs;

/// most sound held back, in seconds. anything older gets dropped, so a hiccup
/// never leaves the sound running behind the picture
const MOST: f32 = 0.25;
/// held back before it starts playing, so the first moments don't crackle
const FIRST: f32 = 0.06;

type Samples = Arc<Mutex<Buffer>>;

#[derive(Default)]
struct Buffer {
    samples: VecDeque<f32>,
    playing: bool,
}

pub struct Sound {
    child: Child,
    _stream: Stream,
}

impl Sound {
    pub fn start(url: &str) -> Result<Self, String> {
        let device = cpal::default_host().default_output_device().ok_or("there are no speakers to play it on")?;
        let config = device.default_output_config().map_err(|e| format!("couldn't open the speakers: {e}"))?;
        let rate = config.sample_rate();
        let channels = config.channels();
        let held = |secs: f32| (rate as f32 * secs) as usize * channels as usize;
        let (most, first) = (held(MOST), held(FIRST));

        let samples: Samples = Arc::default();
        let stream = match config.sample_format() {
            SampleFormat::F32 => play::<f32>(&device, &config.config(), samples.clone(), first, |s| s),
            SampleFormat::I16 => play::<i16>(&device, &config.config(), samples.clone(), first, |s| (s * i16::MAX as f32) as i16),
            other => return Err(format!("the speakers want {other} samples, which this can't play")),
        }?;
        stream.play().map_err(|e| format!("couldn't start the speakers: {e}"))?;

        let mut child = thumbs::ffmpeg()
            .args(["-loglevel", "error", "-nostdin", "-fflags", "nobuffer", "-flags", "low_delay", "-vn", "-i", url])
            .args(["-ac", &channels.to_string(), "-ar", &rate.to_string(), "-c:a", "pcm_f32le", "-f", "f32le", "-"])
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("couldn't start ffmpeg for the sound: {e}"))?;
        let mut out = child.stdout.take().ok_or("ffmpeg has no output")?;
        std::thread::Builder::new()
            .name("live sound".into())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                while out.read_exact(&mut buf).is_ok() {
                    let mut b = samples.lock().unwrap();
                    b.samples.extend(buf.as_chunks::<4>().0.iter().map(|s| f32::from_le_bytes(*s)));
                    let over = b.samples.len().saturating_sub(most);
                    b.samples.drain(..over);
                }
            })
            .map_err(|e| format!("couldn't start the sound: {e}"))?;
        Ok(Self { child, _stream: stream })
    }
}

impl Drop for Sound {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// a speaker stream that plays what's buffered, and silence when it runs dry
fn play<T: cpal::SizedSample + Default + Send + 'static>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Samples,
    first: usize,
    convert: fn(f32) -> T,
) -> Result<Stream, String> {
    device
        .build_output_stream(
            *config,
            move |out: &mut [T], _| {
                let mut b = samples.lock().unwrap();
                if !b.playing && b.samples.len() >= first {
                    b.playing = true;
                }
                let playing = b.playing;
                for o in out.iter_mut() {
                    *o = if playing { b.samples.pop_front().map(convert).unwrap_or_default() } else { T::default() };
                }
                if b.samples.is_empty() {
                    b.playing = false;
                }
            },
            |e| log::warn!("live sound: {e}"),
            None,
        )
        .map_err(|e| format!("couldn't open the speakers: {e}"))
}

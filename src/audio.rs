//! Captures game audio (the default output's monitor) and optionally the
//! microphone through PipeWire. Samples are handed to the muxer with a
//! CLOCK_MONOTONIC timestamp so they line up with the video.

use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

use anyhow::{Context, Result};
use pipewire as pw;
use pw::spa;
use pw::spa::pod::Pod;

use crate::mux::Msg;

pub const RATE: u32 = 48_000;
pub const CHANNELS: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Game,
    Mic,
}

pub struct AudioChunk {
    pub source: Source,
    /// CLOCK_MONOTONIC time of the first sample, in nanoseconds.
    pub time_ns: u64,
    /// Interleaved stereo f32.
    pub samples: Vec<f32>,
}

struct Terminate;

pub struct Capture {
    stop: pw::channel::Sender<Terminate>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    pub fn start(sources: Vec<Source>, tx: Sender<Msg>) -> Result<Self> {
        let (stop, stop_rx) = pw::channel::channel::<Terminate>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<()>>();

        let thread = std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || {
                let result = run(sources, tx, stop_rx, &ready_tx);
                if let Err(e) = result {
                    let _ = ready_tx.send(Err(e));
                }
            })?;

        ready_rx
            .recv()
            .context("audio thread died during startup")??;
        Ok(Self { stop, thread: Some(thread) })
    }

    pub fn stop(mut self) {
        let _ = self.stop.send(Terminate);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// How long to wait before connecting again after losing PipeWire.
const RECONNECT_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

fn run(
    sources: Vec<Source>,
    tx: Sender<Msg>,
    stop_rx: pw::channel::Receiver<Terminate>,
    ready: &std::sync::mpsc::Sender<Result<()>>,
) -> Result<()> {
    use std::cell::Cell;
    use std::rc::Rc;

    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).context("creating PipeWire loop")?;
    let stopping = Rc::new(Cell::new(false));
    let _stop = stop_rx.attach(mainloop.loop_(), {
        let (mainloop, stopping) = (mainloop.clone(), stopping.clone());
        move |_| {
            stopping.set(true);
            mainloop.quit()
        }
    });

    // PipeWire restarts now and then (on the Frame, starting a game can do
    // it), and streams on the old connection just go quiet. So: connect,
    // capture until the connection's lost, and connect again, until stopped.
    let mut first = true;
    loop {
        match capture(&mainloop, &sources, &tx, first.then_some(ready)) {
            Ok(()) => {}
            Err(e) if first => return Err(e),
            Err(e) => log::warn!("audio: {e:#}, trying again"),
        }
        first = false;
        if stopping.get() {
            return Ok(());
        }
        // a second's pause, while still hearing about being stopped
        let timer = mainloop.loop_().add_timer({
            let mainloop = mainloop.clone();
            move |_| mainloop.quit()
        });
        let _ = timer.update_timer(Some(RECONNECT_AFTER), None);
        mainloop.run();
        if stopping.get() {
            return Ok(());
        }
    }
}

/// One connection to PipeWire, capturing until it's lost or we're stopped.
fn capture(
    mainloop: &pw::main_loop::MainLoopRc,
    sources: &[Source],
    tx: &Sender<Msg>,
    ready: Option<&std::sync::mpsc::Sender<Result<()>>>,
) -> Result<()> {
    let context = pw::context::ContextRc::new(mainloop, None).context("creating PipeWire context")?;
    let core = context.connect_rc(None).context("connecting to PipeWire")?;
    let _lost = core
        .add_listener_local()
        .error({
            let mainloop = mainloop.clone();
            move |id, _seq, _res, message| {
                if id == pw::core::PW_ID_CORE {
                    log::warn!("audio: lost PipeWire ({message}), connecting again");
                    mainloop.quit();
                }
            }
        })
        .register();

    let mut streams = Vec::new();
    for &source in sources {
        streams.push(open_stream(&core, source, tx.clone(), mainloop.clone())?);
        log::info!("audio: capturing {source:?}");
    }
    if let Some(ready) = ready {
        let _ = ready.send(Ok(()));
    }
    mainloop.run();
    drop(streams);
    Ok(())
}

type Stream<'c> = (pw::stream::StreamBox<'c>, pw::stream::StreamListener<()>);

fn open_stream(core: &pw::core::CoreRc, source: Source, tx: Sender<Msg>, mainloop: pw::main_loop::MainLoopRc) -> Result<Stream<'_>> {
    let mut props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Production",
        *pw::keys::APP_NAME => "framecorder",
        *pw::keys::NODE_NAME => match source {
            Source::Game => "framecorder-game",
            Source::Mic => "framecorder-mic",
        },
    };
    if source == Source::Game {
        props.insert("stream.capture.sink", "true");
        // Listening in shouldn't keep the speakers awake when nothing plays.
        props.insert("node.passive", "true");
    }

    let stream = pw::stream::StreamBox::new(core, "framecorder", props).context("creating audio stream")?;

    let listener = stream
        .add_local_listener_with_user_data(())
        .state_changed(move |_, _, _, new| {
            // a stream PipeWire's given up on: start over with a fresh connection
            if let pw::stream::StreamState::Error(e) = new {
                log::warn!("audio: {source:?} stream failed ({e}), connecting again");
                mainloop.quit();
            }
        })
        .process(move |stream, _| {
            let Some(mut buffer) = stream.dequeue_buffer() else { return };
            let datas = buffer.datas_mut();
            let Some(data) = datas.first_mut() else { return };
            let chunk = data.chunk();
            let (offset, size) = (chunk.offset() as usize, chunk.size() as usize);
            let Some(bytes) = data.data() else { return };
            let Some(bytes) = bytes.get(offset..offset + size) else { return };

            let samples: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
            let frames = samples.len() as u64 / CHANNELS as u64;
            let duration = frames * 1_000_000_000 / RATE as u64;
            // The graph clock says when this cycle started, which doesn't
            // wobble with how late our callback runs. The samples are that
            // cycle's worth, minus however long they took to reach us.
            let time_ns = match stream.time() {
                Ok(t) if t.now() > 0 => {
                    let rate = t.rate();
                    let delay = if rate.denom > 0 { t.delay() * rate.num as i64 * 1_000_000_000 / rate.denom as i64 } else { 0 };
                    (t.now() - delay.max(0)).max(0) as u64
                }
                _ => crate::clock::now().as_nanos() as u64,
            }
            .saturating_sub(duration);
            let _ = tx.send(Msg::Audio(AudioChunk { source, time_ns, samples }));
        })
        .register()
        .context("registering audio callback")?;

    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(RATE);
    info.set_channels(CHANNELS);
    let mut position = [0u32; spa::param::audio::MAX_CHANNELS];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    info.set_position(position);

    let obj = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    let bytes = spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &spa::pod::Value::Object(obj))
        .context("building audio format")?
        .0
        .into_inner();
    let mut params = [Pod::from_bytes(&bytes).context("building audio format")?];

    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .context("connecting audio stream")?;

    Ok((stream, listener))
}

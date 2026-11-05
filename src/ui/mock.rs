//! `framecorder-ui --demo [port]`: the tab, drawn by the real view code, in a
//! browser instead of the headset, with made up state behind it. Clicks,
//! hover and typing work; nothing records, syncs or saves. For looking at
//! the tab and trying it without a Frame.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::pairing::clients::{Transfer, Update};
use super::pairing::{Device, Pairing, Presence};
use super::paint::Canvas;
use super::settings::Settings;
use super::text::Fonts;
use super::view::{self, Action, Hit, Model, Phase, ReportView, Screen, Status, Step, SyncView};
use super::{sample_summary, CLIP_COOLDOWN};

/// The states the page's picker starts from.
const STARTS: &[&str] = &["home", "setup", "locked-out", "live", "many", "updating", "low-space", "no-sync"];
const SENDING_FOR: Duration = Duration::from_millis(1500);

enum Report {
    Writing,
    Sending(Instant),
    Sent,
}

struct Mock {
    settings: Settings,
    mics: Vec<crate::mics::Mic>,
    screen: Screen,
    recording: Option<Instant>,
    clipped_at: Option<Instant>,
    note: Option<(String, bool)>,
    toast: Option<(String, bool)>,
    pointer: Option<(f32, f32)>,
    over: Option<usize>,
    devices: Vec<Device>,
    page: usize,
    sync_available: bool,
    pairing: Option<Pairing>,
    update: Update,
    unlocked: bool,
    free: u64,
    autostart: bool,
    text: String,
    typing: bool,
    report: Report,
    hits: Vec<Hit>,
    /// Bumped on every change, so the page knows to fetch a new picture.
    version: u64,
    shown_second: u64,
}

impl Mock {
    fn new(start: &str) -> Self {
        let live = |id: &str, name: &str, presence, old_app, transfer| Device { presence, old_app, transfer, ..Device::paired(id, name) };
        let getting = Some(Transfer { name: "2026-10-15_12-27-10.mp4".into(), done: 19_200_000, total: 48_000_000 });
        let devices = match start {
            "live" | "updating" | "low-space" => vec![
                live("a", "Pixel 9", Presence::Connected, false, getting),
                live("b", "COLEPCWIN", Presence::Away, false, None),
                live("c", "Sam's MacBook Pro", Presence::Connected, true, None),
            ],
            "many" => ["Pixel 9", "COLEPCWIN", "Sam's MacBook Pro", "Living room PC", "Galaxy S25", "Work laptop"]
                .iter()
                .enumerate()
                .map(|(i, name)| live(&i.to_string(), name, if i % 2 == 0 { Presence::Connected } else { Presence::Away }, false, None))
                .collect(),
            _ => vec![live("a", "Pixel 9", Presence::Connected, false, None), live("b", "COLEPCWIN", Presence::Connected, false, None)],
        };
        Self {
            settings: Settings { onboarded: start != "setup", ..Settings::default() },
            mics: super::sample_mics("audio-mics"),
            screen: if start == "setup" { Screen::Onboarding(Step::Shape) } else { Screen::Home },
            recording: None,
            clipped_at: None,
            note: None,
            toast: None,
            pointer: None,
            over: None,
            devices,
            page: 0,
            sync_available: start != "no-sync",
            pairing: None,
            update: if start == "updating" { Update::Updating } else { Update::Idle },
            unlocked: start != "locked-out",
            free: if start == "low-space" { 1_400_000_000 } else { 128_400_000_000 },
            autostart: true,
            text: String::new(),
            typing: false,
            report: Report::Writing,
            hits: Vec::new(),
            version: 1,
            shown_second: 0,
        }
    }

    /// What time changes by itself: the recording clock, the clip button
    /// cooling down, a report finishing sending.
    fn tick(&mut self) {
        if let Some(at) = self.recording {
            let second = at.elapsed().as_secs();
            if second != self.shown_second {
                self.shown_second = second;
                self.version += 1;
            }
        }
        if self.clipped_at.is_some_and(|t| t.elapsed() >= CLIP_COOLDOWN) {
            self.clipped_at = None;
            self.version += 1;
        }
        if matches!(self.report, Report::Sending(at) if at.elapsed() >= SENDING_FOR) {
            self.report = Report::Sent;
            self.note = Some(("Sent as issue #42 · https://github.com/coah80/framecorder/issues/42".into(), true));
            self.toast = Some(("Report sent, it's issue #42".into(), true));
            self.version += 1;
        }
    }

    fn act(&mut self, action: Action) {
        self.version += 1;
        let s = &mut self.settings;
        match action {
            Action::TurnOff => self.note = Some(("Closing. framecorder starts again with SteamVR.".into(), true)),
            Action::Report => self.go(Screen::Report),
            Action::Type => {
                if matches!(self.report, Report::Writing) {
                    self.typing = true;
                }
            }
            Action::Send => self.report = Report::Sending(Instant::now()),
            Action::Autostart => self.autostart = !self.autostart,
            Action::Record if s.onboarded => match self.recording.take() {
                Some(at) => {
                    let secs = at.elapsed().as_secs();
                    self.note = Some((format!("Saved 2026-11-05_14-02-11.mp4 · {} · 41.0 MB", view::clock(Duration::from_secs(secs))), true));
                }
                None => {
                    self.recording = Some(Instant::now());
                    self.note = None;
                }
            },
            Action::Record => {}
            Action::ClipNow if s.onboarded && s.clips && self.clipped_at.is_none() => {
                self.clipped_at = Some(Instant::now());
                self.toast = Some((format!("Clipped the last {} s", s.clip_secs), true));
            }
            Action::ClipNow => {}
            Action::Open(section) => self.go(Screen::Settings(section)),
            Action::Close => self.go(Screen::Home),
            Action::Step(step) => self.go(Screen::Onboarding(step)),
            Action::FinishSetup => {
                self.settings.onboarded = true;
                self.go(Screen::Home);
            }
            Action::Pair => self.pairing = Pairing::sample().ok(),
            Action::ClosePair => self.pairing = None,
            Action::DevicePage(page) => self.page = page,
            Action::RemoveDevice(i) => {
                if i < self.devices.len() {
                    let gone = self.devices.remove(i);
                    self.note = Some((format!("Removed {}", gone.name), true));
                }
            }
            Action::Shape(shape) => s.shape = shape,
            Action::Eye(eye) => s.eye = eye,
            Action::Quality(q) => s.quality = q,
            Action::Fps(fps) => s.fps = fps,
            Action::GameAudio => s.game_audio = !s.game_audio,
            Action::Mic => s.mic = !s.mic,
            Action::SplitAudio => s.split_audio = !s.split_audio,
            Action::MicSource(i) => {
                if let Some(mic) = self.mics.get(i) {
                    s.mic_source = mic.name.clone();
                }
            }
            Action::Clips => s.clips = !s.clips,
            Action::ClipLength(secs) => s.clip_secs = secs,
        }
    }

    fn go(&mut self, screen: Screen) {
        self.screen = screen;
        self.pairing = None;
        self.typing = false;
        if screen == Screen::Report && matches!(self.report, Report::Sent) {
            self.report = Report::Writing;
            self.text.clear();
        }
    }

    /// A key from the browser, while "SteamVR's keyboard" is up.
    fn key(&mut self, key: &str) {
        if !self.typing {
            return;
        }
        match key {
            "Escape" => self.typing = false,
            "Enter" => self.text.push('\n'),
            "Backspace" => {
                self.text.pop();
            }
            k if k.chars().count() == 1 => self.text.push_str(k),
            _ => return,
        }
        self.version += 1;
    }

    fn hover_at(&mut self, x: f32, y: f32) {
        self.pointer = Some((x, y));
        let over = self.hits.iter().position(|h| h.rect.contains(x, y));
        if over != self.over {
            self.over = over;
            self.version += 1;
        }
    }

    fn click(&mut self, x: f32, y: f32) {
        if let Some(action) = self.hits.iter().find(|h| h.rect.contains(x, y)).map(|h| h.action) {
            self.act(action);
        }
    }

    fn draw(&mut self, fonts: &mut Fonts) -> Vec<u8> {
        let mut canvas = Canvas::new(view::WIDTH, view::HEIGHT);
        let summary = sample_summary();
        let phase = match self.report {
            Report::Writing => Phase::Writing,
            Report::Sending(_) => Phase::Sending,
            Report::Sent => Phase::Sent { number: 42, url: "https://github.com/coah80/framecorder/issues/42" },
        };
        let model = Model {
            settings: &self.settings,
            mics: &self.mics,
            screen: self.screen,
            sync: SyncView {
                available: self.sync_available,
                error: None,
                devices: &self.devices,
                page: self.page,
                pairing: self.pairing.as_ref().map(Ok),
                update: &self.update,
            },
            clip_cooling: self.clipped_at.is_some(),
            live_viewers: 0,
            report: ReportView { text: &self.text, typing: self.typing, summary: Some(&summary), phase },
            unlocked: self.unlocked,
            status: match self.recording {
                Some(at) => Status::Recording { recorded: at.elapsed() },
                None => Status::Idle,
            },
            pointer: self.pointer,
            note: self.note.as_ref().map(|(t, ok)| (t.as_str(), *ok)),
            relocked: false,
            free: Some(self.free),
            autostart: self.autostart,
        };
        self.hits = view::draw(&mut canvas, fonts, &model);
        canvas.pixels
    }
}

pub fn serve(port: u16) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port)).with_context(|| format!("listening on 127.0.0.1:{port}"))?;
    log::info!("the tab demo is on http://127.0.0.1:{port}");
    let mut fonts = Fonts::load()?;
    let mut mock = Mock::new("home");
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if let Err(e) = answer(stream, &mut mock, &mut fonts) {
            log::warn!("a demo request failed: {e:#}");
        }
    }
    Ok(())
}

fn answer(mut stream: TcpStream, mock: &mut Mock, fonts: &mut Fonts) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let target = line.split_whitespace().nth(1).unwrap_or("/").to_string();
    // the headers, read and dropped; nothing here sends a body
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
    }
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let arg = |name: &str| query.split('&').find_map(|kv| kv.strip_prefix(name)?.strip_prefix('=')).map(decode);
    let xy = || Some((arg("x")?.parse::<f32>().ok()?, arg("y")?.parse::<f32>().ok()?));
    mock.tick();
    let (kind, body): (&str, Vec<u8>) = match path {
        "/" => ("text/html; charset=utf-8", page().into_bytes()),
        "/frame" => ("application/octet-stream", mock.draw(fonts)),
        "/state" => {
            let toast = mock.toast.take().map(|(t, ok)| format!(r#"{{"text":{},"ok":{ok}}}"#, json(&t))).unwrap_or_else(|| "null".into());
            ("application/json", format!(r#"{{"version":{},"typing":{},"toast":{toast}}}"#, mock.version, mock.typing).into_bytes())
        }
        "/click" | "/move" => {
            if let Some((x, y)) = xy() {
                if path == "/click" { mock.click(x, y) } else { mock.hover_at(x, y) }
            }
            ("text/plain", Vec::new())
        }
        "/key" => {
            if let Some(k) = arg("k") {
                mock.key(&k);
            }
            ("text/plain", Vec::new())
        }
        "/start" => {
            let version = mock.version + 1;
            *mock = Mock { version, ..Mock::new(arg("s").as_deref().unwrap_or("home")) };
            ("text/plain", Vec::new())
        }
        _ => ("text/plain", b"not here".to_vec()),
    };
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len())?;
    stream.write_all(&body)?;
    Ok(())
}

/// %xx and + in a query value
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(b) => {
                    out.push(b);
                    i += 2;
                }
                None => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn json(s: &str) -> String {
    let escaped: String = s.chars().flat_map(|c| match c {
        '"' => vec!['\\', '"'],
        '\\' => vec!['\\', '\\'],
        '\n' => vec!['\\', 'n'],
        c => vec![c],
    }).collect();
    format!("\"{escaped}\"")
}

fn page() -> String {
    let starts: String = STARTS.iter().map(|s| format!(r#"<option>{s}</option>"#)).collect();
    format!(
        r#"<!doctype html><meta charset="utf-8"><title>framecorder tab</title>
<style>
body{{margin:0;background:#11111b;color:#a6adc8;font:13px system-ui;display:flex;flex-direction:column;align-items:center;gap:12px;padding:16px}}
canvas{{width:min(100%,1280px);border-radius:14px;box-shadow:0 0 0 1px #313244;cursor:default}}
#bar{{display:flex;gap:10px;align-items:center}} select,button{{background:#1e1e2e;color:#cdd6f4;border:1px solid #45475a;border-radius:8px;padding:4px 8px;font:inherit}}
#toast{{position:fixed;top:24px;padding:10px 16px;border-radius:12px;background:#313244;color:#cdd6f4;opacity:0;transition:opacity .2s}}
#typing{{visibility:hidden;color:#cba6f7}}
</style>
<div id="bar">start from <select id="start">{starts}</select><button id="go">reset</button><span id="typing">keyboard up: type, esc when done</span></div>
<canvas id="c" width="{w}" height="{h}"></canvas><div id="toast"></div>
<script>
const c=document.getElementById('c'),g=c.getContext('2d');let v=-1,busy=false,lastMove=0;
const at=e=>{{const r=c.getBoundingClientRect();return `x=${{((e.clientX-r.left)*{w}/r.width).toFixed(1)}}&y=${{((e.clientY-r.top)*{h}/r.height).toFixed(1)}}`}};
async function frame(){{const b=await (await fetch('/frame')).arrayBuffer();g.putImageData(new ImageData(new Uint8ClampedArray(b),{w},{h}),0,0)}}
async function poll(){{if(busy)return;busy=true;try{{const s=await (await fetch('/state')).json();
 document.getElementById('typing').style.visibility=s.typing?'visible':'hidden';
 if(s.toast){{const t=document.getElementById('toast');t.textContent=s.toast.text;t.style.color=s.toast.ok?'#cdd6f4':'#f38ba8';t.style.opacity=1;setTimeout(()=>t.style.opacity=0,2500)}}
 if(s.version!==v){{v=s.version;await frame()}}}}finally{{busy=false}}}}
c.onclick=async e=>{{await fetch('/click?'+at(e));poll()}};
c.onmousemove=e=>{{const n=Date.now();if(n-lastMove<60)return;lastMove=n;fetch('/move?'+at(e)).then(poll)}};
c.onmouseleave=()=>fetch('/move?x=-1&y=-1').then(poll);
document.onkeydown=async e=>{{if(e.target.tagName==='SELECT')return;if(e.key.length===1||['Enter','Backspace','Escape'].includes(e.key)){{e.preventDefault();await fetch('/key?k='+encodeURIComponent(e.key));poll()}}}};
document.getElementById('go').onclick=async()=>{{await fetch('/start?s='+document.getElementById('start').value);poll()}};
setInterval(poll,250);poll();
</script>"#,
        w = view::WIDTH,
        h = view::HEIGHT,
    )
}

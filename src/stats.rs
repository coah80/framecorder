use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const URL: &str = "https://stats.coah80.com/v1/ping";
static FILE: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
pub enum Made {
    Recording(f64),
    Clip,
    Live,
    Remote,
    Report,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct State {
    last: String,
    ever: bool,
    first: u64,
    recordings: u64,
    clips: u64,
    seconds: u64,
    live: u64,
    remote: u64,
    reports: u64,
    perf: BTreeMap<String, u64>,
}

impl State {
    fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for (key, value) in text.lines().filter_map(|l| l.rsplit_once('=')) {
            match key {
                "last" => s.last = value.to_owned(),
                "ever" => s.ever = value == "true",
                "first" => s.first = value.parse().unwrap_or(0),
                "recordings" => s.recordings = value.parse().unwrap_or(0),
                "clips" => s.clips = value.parse().unwrap_or(0),
                "seconds" => s.seconds = value.parse().unwrap_or(0),
                "live" => s.live = value.parse().unwrap_or(0),
                "remote" => s.remote = value.parse().unwrap_or(0),
                "reports" => s.reports = value.parse().unwrap_or(0),
                _ => {
                    if let (Some(name), Ok(n)) = (key.strip_prefix("perf."), value.parse()) {
                        s.perf.insert(name.to_owned(), n);
                    }
                }
            }
        }
        s
    }

    fn text(&self) -> String {
        let mut text = format!(
            "last={}\never={}\nfirst={}\nrecordings={}\nclips={}\nseconds={}\nlive={}\nremote={}\nreports={}\n",
            self.last, self.ever, self.first, self.recordings, self.clips, self.seconds, self.live, self.remote, self.reports
        );
        for (name, n) in &self.perf {
            text += &format!("perf.{name}={n}\n");
        }
        text
    }
}

fn path() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(dir.join("framecorder/stats"))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn load() -> State {
    let mut state = path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| State::parse(&t)).unwrap_or_default();
    if state.first == 0 {
        state.first = now() / 86400;
    }
    state
}

fn age(days: u64) -> &'static str {
    match days {
        0 => "0",
        1..=6 => "1-6",
        7..=29 => "7-29",
        30..=89 => "30-89",
        _ => "90+",
    }
}

fn save(state: &State) {
    let Some(path) = path() else { return };
    let tmp = path.with_extension("tmp");
    let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&tmp, state.text())).and_then(|_| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        log::debug!("couldn't keep the stats count: {e}");
    }
}

fn day(secs: u64) -> String {
    let z = (secs / 86400) as i64 + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

fn today() -> String {
    day(now())
}

pub fn made(what: Made) {
    if cfg!(test) {
        return;
    }
    let _held = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load();
    match what {
        Made::Recording(secs) => {
            state.recordings += 1;
            state.seconds += secs.max(0.0) as u64;
        }
        Made::Clip => state.clips += 1,
        Made::Live => state.live += 1,
        Made::Remote => state.remote += 1,
        Made::Report => state.reports += 1,
    }
    save(&state);
}

fn body(state: &State, settings: &[(&str, String)]) -> Value {
    let settings: serde_json::Map<String, Value> = settings.iter().map(|(k, v)| ((*k).to_owned(), json!(v))).collect();
    json!({
        "app": "headset",
        "version": env!("CARGO_PKG_VERSION"),
        "os": "steamos",
        "new": !state.ever,
        "age": age((now() / 86400).saturating_sub(state.first)),
        "counts": {
            "recordings": state.recordings.min(10_000),
            "clips": state.clips.min(10_000),
            "minutes": (state.seconds / 60).min(100_000),
            "live": state.live.min(10_000),
            "remote": state.remote.min(10_000),
            "reports": state.reports.min(1_000),
        },
        "settings": settings,
        "perf": state.perf.iter().map(|(k, n)| (k.clone(), json!(n.min(&PERF_MOST)))).collect::<serde_json::Map<_, _>>(),
    })
}

pub fn due() -> bool {
    load().last != today()
}

pub fn ping(settings: Vec<(&'static str, String)>) {
    let spawned = std::thread::Builder::new().name("stats".into()).spawn(move || {
        let sent = load();
        let day = today();
        let Some(sealed) = crate::seal::seal(&crate::seal::SERVICE, body(&sent, &settings).to_string().as_bytes()) else { return };
        let child = Command::new("curl")
            .args(["--silent", "--fail", "--max-time", "15", "--connect-timeout", "5", "--proto", "=https", "--tlsv1.2"])
            .args(["-A", "framecorder", "-H", "Content-Type: application/octet-stream", "--data-binary", "@-", URL])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child.stdin.take().map(|mut s| s.write_all(&sealed)).transpose()?;
                child.wait()
            });
        if !child.is_ok_and(|s| s.success()) {
            return;
        }
        let _held = FILE.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = load();
        state.last = day;
        state.ever = true;
        state.recordings = state.recordings.saturating_sub(sent.recordings);
        state.clips = state.clips.saturating_sub(sent.clips);
        state.seconds = state.seconds.saturating_sub(sent.seconds / 60 * 60);
        state.live = state.live.saturating_sub(sent.live);
        state.remote = state.remote.saturating_sub(sent.remote);
        state.reports = state.reports.saturating_sub(sent.reports);
        for (name, n) in &sent.perf {
            if let Some(left) = state.perf.get_mut(name) {
                *left = left.saturating_sub(*n);
            }
        }
        state.perf.retain(|_, n| *n > 0);
        save(&state);
    });
    if let Err(e) = spawned {
        log::debug!("couldn't start the stats ping: {e}");
    }
}

const PERF_MOST: u64 = 10_000;
const SAMPLE_EVERY: Duration = Duration::from_secs(15 * 60);
const LOOK_EVERY: Duration = Duration::from_secs(10);

fn bucket(v: f64, edges: &[u32]) -> String {
    match edges.iter().position(|&e| v < f64::from(e)) {
        Some(0) => format!("<{}", edges[0]),
        Some(i) => format!("{}-{}", edges[i - 1], edges[i]),
        None => format!("{}+", edges[edges.len() - 1]),
    }
}

fn mb(v: f64) -> String {
    bucket(v, &[25, 50, 100, 200, 400, 800])
}

fn up(secs: f64) -> &'static str {
    match secs / 3600.0 {
        h if h < 1.0 => "<1h",
        h if h < 6.0 => "1-6h",
        h if h < 24.0 => "6-24h",
        _ => "1d+",
    }
}

fn cpu(pct: f64) -> String {
    bucket(pct, &[1, 5, 15, 40])
}

fn add(keys: Vec<String>) {
    if cfg!(test) || keys.is_empty() {
        return;
    }
    let _held = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load();
    for key in keys {
        *state.perf.entry(key).or_default() += 1;
    }
    save(&state);
}

mod proc {
    use std::time::Duration;

    fn ticks_per_sec() -> f64 {
        (unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).max(1) as f64
    }

    fn stat(pid: &str) -> Option<Vec<String>> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        Some(text.rsplit_once(')')?.1.split_whitespace().map(str::to_owned).collect())
    }

    pub fn cpu(pid: &str) -> Option<Duration> {
        let f = stat(pid)?;
        let ticks: u64 = f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?;
        Some(Duration::from_secs_f64(ticks as f64 / ticks_per_sec()))
    }

    pub fn age(pid: &str) -> Option<f64> {
        let started = f64::from(stat(pid)?.get(19)?.parse::<u32>().ok()?) / ticks_per_sec();
        let uptime: f64 = std::fs::read_to_string("/proc/uptime").ok()?.split_whitespace().next()?.parse().ok()?;
        Some((uptime - started).max(0.0))
    }

    fn kb(file: &str, field: &str) -> Option<f64> {
        let text = std::fs::read_to_string(file).ok()?;
        let line = text.lines().find(|l| l.starts_with(field))?;
        line[field.len()..].trim().trim_end_matches("kB").trim().parse().ok()
    }

    pub fn rss(pid: &str) -> Option<f64> {
        kb(&format!("/proc/{pid}/status"), "VmRSS:").map(|k| k / 1024.0)
    }

    pub fn available() -> Option<f64> {
        kb("/proc/meminfo", "MemAvailable:").map(|k| k / 1024.0)
    }

    pub fn find(program: &str) -> Option<String> {
        let uid = unsafe { libc::getuid() };
        std::fs::read_dir("/proc").ok()?.flatten().find_map(|e| {
            let pid = e.file_name().into_string().ok().filter(|p| p.bytes().all(|b| b.is_ascii_digit()))?;
            use std::os::unix::fs::MetadataExt;
            (e.metadata().ok()?.uid() == uid && std::fs::read_link(format!("/proc/{pid}/exe")).ok()?.file_name()? == program).then_some(pid)
        })
    }
}

struct Take {
    pid: String,
    cpu_from: (Instant, Duration),
    fps: Vec<f64>,
    gpu_ms: Vec<f64>,
    most_mb: f64,
    least_free: f64,
}

#[derive(Default)]
pub struct Watch {
    take: Option<Take>,
    stretch: Option<Stretch>,
    looked: Option<Instant>,
}

struct Stretch {
    from: Instant,
    game: bool,
    cpu: Vec<(&'static str, String, Duration)>,
}

impl Stretch {
    fn new(game: bool, clips: Option<u32>) -> Self {
        let cpu = processes(clips).into_iter().filter_map(|(name, pid)| Some((name, pid.clone(), proc::cpu(&pid)?))).collect();
        Self { from: Instant::now(), game, cpu }
    }
}

fn processes(clips: Option<u32>) -> Vec<(&'static str, String)> {
    let mut out = vec![("ui", "self".to_string())];
    out.extend(clips.map(|pid| ("clips", pid.to_string())));
    out.extend(proc::find("framecorder-sync").map(|pid| ("sync", pid)));
    out
}

impl Watch {
    pub fn report(&mut self, recorder: u32, fps: f64, target: u32, gpu_ms: f64, recording: bool) {
        if !recording {
            self.take = None;
            return;
        }
        let pid = recorder.to_string();
        let take = self.take.get_or_insert_with(|| Take {
            cpu_from: (Instant::now(), proc::cpu(&pid).unwrap_or_default()),
            pid,
            fps: Vec::new(),
            gpu_ms: Vec::new(),
            most_mb: 0.0,
            least_free: f64::MAX,
        });
        take.fps.push(100.0 * fps / f64::from(target.max(1)));
        take.gpu_ms.push(gpu_ms);
        take.most_mb = take.most_mb.max(proc::rss(&take.pid).unwrap_or(0.0));
        take.least_free = take.least_free.min(proc::available().unwrap_or(f64::MAX));
    }

    pub fn saved(&mut self) {
        let Some(t) = self.take.take() else { return };
        if t.fps.is_empty() {
            return;
        }
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let secs = t.cpu_from.0.elapsed().as_secs_f64().max(1.0);
        let used = proc::cpu(&t.pid).map_or(0.0, |c| c.saturating_sub(t.cpu_from.1).as_secs_f64());
        let mut keys = vec![
            format!("rec_fps={}", bucket(mean(&t.fps), &[50, 80, 95])),
            format!("rec_gpu_ms={}", bucket(mean(&t.gpu_ms), &[3, 5, 8, 12])),
            format!("rec_cpu={}", cpu(100.0 * used / secs)),
            format!("rec_mb={}", mb(t.most_mb)),
        ];
        if t.least_free < f64::MAX {
            keys.push(format!("rec_free_mb={}", bucket(t.least_free, &[500, 1000, 2000, 4000])));
        }
        add(keys);
    }

    pub fn forget(&mut self) {
        self.take = None;
    }

    pub fn tick(&mut self, clips: Option<u32>, recording: bool, game: impl FnOnce() -> bool) {
        if recording {
            self.stretch = None;
            return;
        }
        if self.looked.is_some_and(|t| t.elapsed() < LOOK_EVERY) {
            return;
        }
        self.looked = Some(Instant::now());
        let game = game();
        let Some(stretch) = self.stretch.as_ref().filter(|s| s.game == game) else {
            self.stretch = Some(Stretch::new(game, clips));
            return;
        };
        if stretch.from.elapsed() < SAMPLE_EVERY {
            return;
        }
        add(readings(stretch, clips));
        self.stretch = Some(Stretch::new(game, clips));
    }
}

fn readings(stretch: &Stretch, clips: Option<u32>) -> Vec<String> {
    let when = if stretch.game { "game" } else { "idle" };
    let secs = stretch.from.elapsed().as_secs_f64().max(1.0);
    let mut keys = Vec::new();
    for (name, pid) in processes(clips) {
        if let (Some(rss), Some(age)) = (proc::rss(&pid), proc::age(&pid)) {
            keys.push(format!("{when}_{name}_mb@{}={}", up(age), mb(rss)));
        }
        let before = stretch.cpu.iter().find(|(n, p, _)| *n == name && *p == pid).map(|c| c.2);
        if let (Some(before), Some(now)) = (before, proc::cpu(&pid)) {
            keys.push(format!("{when}_{name}_cpu={}", cpu(100.0 * now.saturating_sub(before).as_secs_f64() / secs)));
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ping_says_only_what_the_service_takes() {
        let perf = [("rec_fps=95+", 2), ("idle_ui_mb@6-24h=25-50", 40_000)].map(|(k, n)| (k.to_string(), n)).into();
        let state = State { last: "2026-11-13".into(), ever: false, first: now() / 86400 - 12, recordings: 3, clips: 20_000, seconds: 185, live: 2, remote: 1, reports: 0, perf };
        let settings = [("shape", "wide".to_string()), ("paired", "3+".to_string())];
        assert_eq!(
            body(&state, &settings).to_string(),
            format!(
                r#"{{"age":"7-29","app":"headset","counts":{{"clips":10000,"live":2,"minutes":3,"recordings":3,"remote":1,"reports":0}},"new":true,"os":"steamos","perf":{{"idle_ui_mb@6-24h=25-50":10000,"rec_fps=95+":2}},"settings":{{"paired":"3+","shape":"wide"}},"version":"{}"}}"#,
                env!("CARGO_PKG_VERSION")
            )
        );
        assert_eq!(State::parse(&state.text()), state);
        assert_eq!([age(0), age(6), age(7), age(29), age(30), age(89), age(90)], ["0", "1-6", "7-29", "7-29", "30-89", "30-89", "90+"]);
        assert_eq!(day(1_794_700_800 - 1), "2026-11-14");
        assert_eq!(day(951_782_400), "2000-02-29");
    }

    #[test]
    fn readings_go_in_the_buckets_the_service_knows() {
        assert_eq!([mb(3.0), mb(25.0), mb(99.9), mb(799.0), mb(800.0)], ["<25", "25-50", "50-100", "400-800", "800+"]);
        assert_eq!([cpu(0.4), cpu(12.0), cpu(400.0)], ["<1", "5-15", "40+"]);
        assert_eq!([up(60.0), up(3600.0), up(7.0 * 3600.0), up(30.0 * 3600.0)], ["<1h", "1-6h", "6-24h", "1d+"]);
        assert!(proc::rss("self").is_some_and(|m| m > 0.0) && proc::age("self").is_some() && proc::cpu("self").is_some());
    }
}

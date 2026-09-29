//! Samples what the recording costs, once a second, into a CSV next to the
//! video: the recorder's own CPU, GPU and memory, the whole headset's, and
//! how the capture itself is keeping up. Cheap enough to leave on.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

const INTERVAL: Duration = Duration::from_secs(1);
/// How often (in samples) to look for processes that opened the GPU since
/// last time. Walking every process's fds is the priciest thing in here.
const RESCAN_EVERY: u32 = 30;

/// Bumped by the capture loop.
#[derive(Default)]
pub struct Counters {
    pub frames: AtomicU64,
    pub dropped: AtomicU64,
    /// Shader time from GPU timestamps, in nanoseconds.
    pub shader_ns: AtomicU64,
}

#[derive(Default, Clone, Copy)]
struct Stat {
    sum: f64,
    max: f64,
}

impl Stat {
    fn add(&mut self, v: f64) {
        self.sum += v;
        self.max = self.max.max(v);
    }
}

#[derive(Default)]
pub struct Summary {
    samples: u32,
    rec_cpu: Stat,
    rec_gpu: Stat,
    rec_mem: Stat,
    sys_cpu: Stat,
    sys_gpu: Stat,
    shader_ms: Stat,
}

impl Summary {
    /// One line for the log, which the dashboard tab also reads back.
    pub fn line(&self) -> String {
        let n = self.samples.max(1) as f64;
        format!(
            "perf summary: recorder cpu {:.1}% of a core (max {:.1}), gpu {:.1}% (max {:.1}), {:.0} MB ram, shader {:.2} ms/frame; whole headset cpu {:.0}%, gpu {:.0}%",
            self.rec_cpu.sum / n,
            self.rec_cpu.max,
            self.rec_gpu.sum / n,
            self.rec_gpu.max,
            self.rec_mem.max,
            self.shader_ms.sum / n,
            self.sys_cpu.sum / n,
            self.sys_gpu.sum / n,
        )
    }
}

pub struct PerfLog {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Summary>,
    pub path: PathBuf,
}

/// Where the CSV for a recording goes: `clip.mp4` -> `clip.perf.csv`.
pub fn csv_path(video: &Path) -> PathBuf {
    video.with_extension("perf.csv")
}

pub fn start(video: &Path, counters: Arc<Counters>) -> Result<PerfLog> {
    let path = csv_path(video);
    let file = File::create(&path).with_context(|| format!("creating {}", path.display()))?;
    let stop = Arc::new(AtomicBool::new(false));
    let thread = std::thread::Builder::new().name("perf".into()).spawn({
        let stop = stop.clone();
        move || sample(BufWriter::new(file), counters, &stop)
    })?;
    Ok(PerfLog { stop, thread, path })
}

impl PerfLog {
    pub fn finish(self) -> Summary {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.join().unwrap_or_default()
    }
}

struct Sample {
    at: Instant,
    frames: u64,
    dropped: u64,
    shader_ns: u64,
    rec_ticks: u64,
    rec_gpu_ns: u64,
    sys_busy: u64,
    sys_total: u64,
    sys_gpu_ns: u64,
}

fn sample(mut out: BufWriter<File>, counters: Arc<Counters>, stop: &AtomicBool) -> Summary {
    let _ = writeln!(
        out,
        "time_s,fps,dropped,rec_cpu_pct,rec_gpu_pct,shader_ms_per_frame,rec_rss_mb,rec_anon_mb,sys_cpu_pct,sys_gpu_pct,gpu_mhz,sys_mem_used_mb"
    );
    let tick_hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64;
    let gpu_freq = gpu_freq_path();
    let mut gpu_clients = drm_clients();
    let start = Instant::now();
    let mut summary = Summary::default();
    let mut last = read(&counters, &gpu_clients);
    let mut rounds = 0u32;

    while !stop.load(Ordering::Relaxed) {
        // Sleep in small steps so stopping doesn't wait a whole interval.
        let wake = Instant::now() + INTERVAL;
        while Instant::now() < wake && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
        rounds += 1;
        if rounds.is_multiple_of(RESCAN_EVERY) {
            gpu_clients = drm_clients();
        }

        let now = read(&counters, &gpu_clients);
        let dt = now.at.duration_since(last.at).as_secs_f64().max(1e-3);
        let frames = now.frames.saturating_sub(last.frames);
        let fps = frames as f64 / dt;
        let dropped = now.dropped.saturating_sub(last.dropped);
        let rec_cpu = now.rec_ticks.saturating_sub(last.rec_ticks) as f64 / tick_hz / dt * 100.0;
        let rec_gpu = now.rec_gpu_ns.saturating_sub(last.rec_gpu_ns) as f64 / 1e9 / dt * 100.0;
        let shader_ms = now.shader_ns.saturating_sub(last.shader_ns) as f64 / 1e6 / frames.max(1) as f64;
        let sys_total = now.sys_total.saturating_sub(last.sys_total).max(1) as f64;
        let sys_cpu = now.sys_busy.saturating_sub(last.sys_busy) as f64 / sys_total * 100.0;
        // Clients coming and going can make the sum step backwards; clamp.
        let sys_gpu = (now.sys_gpu_ns.saturating_sub(last.sys_gpu_ns) as f64 / 1e9 / dt * 100.0).min(100.0);
        let (rss, anon) = own_memory();
        let mhz = gpu_freq.as_ref().and_then(|p| read_u64(p)).map_or(0, |hz| hz / 1_000_000);
        let mem_used = system_memory_used();

        let _ = writeln!(
            out,
            "{:.1},{:.1},{},{:.2},{:.2},{:.3},{:.1},{:.1},{:.1},{:.1},{},{:.0}",
            start.elapsed().as_secs_f64(),
            fps,
            dropped,
            rec_cpu,
            rec_gpu,
            shader_ms,
            rss,
            anon,
            sys_cpu,
            sys_gpu,
            mhz,
            mem_used
        );
        let _ = out.flush();

        summary.samples += 1;
        summary.rec_cpu.add(rec_cpu);
        summary.rec_gpu.add(rec_gpu);
        summary.rec_mem.add(rss);
        summary.sys_cpu.add(sys_cpu);
        summary.sys_gpu.add(sys_gpu);
        summary.shader_ms.add(shader_ms);
        last = now;
    }
    summary
}

fn read(counters: &Counters, gpu_clients: &[PathBuf]) -> Sample {
    let (sys_busy, sys_total) = system_cpu();
    Sample {
        at: Instant::now(),
        frames: counters.frames.load(Ordering::Relaxed),
        dropped: counters.dropped.load(Ordering::Relaxed),
        shader_ns: counters.shader_ns.load(Ordering::Relaxed),
        rec_ticks: own_cpu_ticks(),
        rec_gpu_ns: own_drm_clients().iter().filter_map(|p| gpu_ns(p)).sum(),
        sys_busy,
        sys_total,
        sys_gpu_ns: gpu_clients.iter().filter_map(|p| gpu_ns(p)).sum(),
    }
}

fn read_u64(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// utime + stime of every thread in this process.
fn own_cpu_ticks() -> u64 {
    let Ok(stat) = fs::read_to_string("/proc/self/stat") else { return 0 };
    // Fields after the ")" that closes the command name; utime and stime are 14 and 15.
    let rest = stat.rsplit_once(')').map_or("", |(_, r)| r);
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let get = |i: usize| fields.get(i).and_then(|f| f.parse::<u64>().ok()).unwrap_or(0);
    get(11) + get(12)
}

/// Resident and anonymous (really ours, not shared libraries) memory in MB.
fn own_memory() -> (f64, f64) {
    let Ok(status) = fs::read_to_string("/proc/self/status") else { return (0.0, 0.0) };
    let field = |name: &str| {
        status
            .lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<f64>().ok())
            .map_or(0.0, |kb| kb / 1024.0)
    };
    (field("VmRSS:"), field("RssAnon:"))
}

/// (busy, total) jiffies across all CPUs.
fn system_cpu() -> (u64, u64) {
    let Ok(stat) = fs::read_to_string("/proc/stat") else { return (0, 0) };
    let Some(line) = stat.lines().next() else { return (0, 0) };
    let v: Vec<u64> = line.split_whitespace().skip(1).filter_map(|f| f.parse().ok()).collect();
    let total: u64 = v.iter().take(8).sum();
    let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
    (total.saturating_sub(idle), total)
}

fn system_memory_used() -> f64 {
    let Ok(info) = fs::read_to_string("/proc/meminfo") else { return 0.0 };
    let field = |name: &str| {
        info.lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    (field("MemTotal:") - field("MemAvailable:")) / 1024.0
}

fn gpu_freq_path() -> Option<PathBuf> {
    fs::read_dir("/sys/class/devfreq").ok()?.flatten().find_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        name.contains("gpu").then(|| e.path().join("cur_freq"))
    })
}

/// GPU time a DRM client has used so far, from its fdinfo.
fn gpu_ns(fdinfo: &Path) -> Option<u64> {
    let text = fs::read_to_string(fdinfo).ok()?;
    text.lines()
        .find(|l| l.starts_with("drm-engine-gpu:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
}

fn is_drm(fd_link: &Path) -> bool {
    fs::read_link(fd_link).is_ok_and(|t| t.starts_with("/dev/dri/"))
}

fn drm_fdinfos(pid_dir: &Path) -> Vec<PathBuf> {
    let Ok(fds) = fs::read_dir(pid_dir.join("fd")) else { return Vec::new() };
    fds.flatten()
        .filter(|e| is_drm(&e.path()))
        .map(|e| pid_dir.join("fdinfo").join(e.file_name()))
        .collect()
}

fn own_drm_clients() -> Vec<PathBuf> {
    drm_fdinfos(Path::new("/proc/self"))
}

/// One fdinfo per GPU client on the system. Several fds can share a client,
/// so they're told apart by client id to avoid counting anything twice.
fn drm_clients() -> Vec<PathBuf> {
    let Ok(procs) = fs::read_dir("/proc") else { return Vec::new() };
    let mut by_id: HashMap<String, PathBuf> = HashMap::new();
    for p in procs.flatten() {
        if !p.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        for info in drm_fdinfos(&p.path()) {
            let Ok(text) = fs::read_to_string(&info) else { continue };
            if let Some(id) = text.lines().find(|l| l.starts_with("drm-client-id:")) {
                by_id.entry(id.to_string()).or_insert(info);
            }
        }
    }
    by_id.into_values().collect()
}

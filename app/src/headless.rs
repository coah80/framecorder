//! `framecorder-app --headless-sync <dir>`: the same sync engine with no
//! window, for servers, testing and scripts.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::core::discover;
use crate::core::engine::{DirSink, Engine, Listener, Progress, State, Status};
use crate::core::pairlink;
use crate::core::store::Entry;

pub const IDENTIFIER: &str = "com.framecorder.app";

const USAGE: &str = "\
usage:
  framecorder-app                              start the app
  framecorder-app --headless-sync <dir> [options]
      --state <dir>        where pairings and the index live (default: the app's)
      --pair <link>        pair using the framecorder://pair?... link from the QR code
      --host <ip:port>     pair with the frame at this address...
      --code <123456>      ...using this code from \"pair a device\" on the frame
      --fp <sha256>        ...and only if its certificate has this fingerprint
      --exit-after <secs>  stop after this long
  framecorder-app --discover [--seconds <n>]  list frames on the network";

struct LogListener {
    last: Mutex<(String, u64)>,
}

impl Listener for LogListener {
    fn status(&self, s: &Status) {
        let msg = s.message.as_deref().unwrap_or("");
        match s.state {
            State::Connected => println!("connected to {} at {}", s.name, s.addr),
            State::Connecting => {}
            State::Unreachable => println!(
                "can't reach {}: make sure it's on, on the same wi-fi, and framecorder is running ({msg})",
                s.name
            ),
            State::Full => println!("out of space for clips from {}: {msg}", s.name),
            State::Unpaired | State::WrongFingerprint => println!("{}: {msg}", s.name),
        }
    }

    fn progress(&self, p: &Progress) {
        // a line per 10%, not per chunk
        let pct = if p.total > 0 { p.done * 100 / p.total } else { 100 };
        let mut last = self.last.lock().unwrap();
        if last.0 != p.id || pct / 10 != last.1 / 10 || p.done == p.total {
            println!("  {} {pct}% ({} / {} bytes)", p.name, p.done, p.total);
            *last = (p.id.clone(), pct);
        }
    }

    fn synced(&self, e: &Entry) {
        println!("synced {} -> {}", e.name, e.location);
    }

    fn removed(&self, _host: &str, id: &str) {
        println!("removed on the frame: {id}");
    }
}

pub fn default_state_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join(IDENTIFIER)
}

pub fn main(args: Vec<String>) -> i32 {
    #[cfg(windows)]
    unsafe {
        // we're a windowed app on Windows, borrow the console we were run from
        windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS);
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("couldn't start: {e}");
            return 1;
        }
    };
    match rt.block_on(run(args)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

async fn run(args: Vec<String>) -> Result<(), String> {
    let mut it = args.into_iter().skip(1);
    let (mut dir, mut state, mut link, mut host, mut code, mut fp) = (None, None, None, None, None, None);
    let (mut exit_after, mut discover_secs, mut discover) = (None, 5u64, false);
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{arg} needs a value\n{USAGE}"));
        match arg.as_str() {
            "--headless-sync" => dir = Some(PathBuf::from(value()?)),
            "--state" => state = Some(PathBuf::from(value()?)),
            "--pair" => link = Some(value()?),
            "--host" => host = Some(value()?),
            "--code" => code = Some(value()?),
            "--fp" => fp = Some(value()?),
            "--exit-after" => exit_after = Some(value()?.parse::<u64>().map_err(|_| "--exit-after wants seconds")?),
            "--discover" => discover = true,
            "--seconds" => discover_secs = value()?.parse().map_err(|_| "--seconds wants a number")?,
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }

    if discover {
        println!("looking for frames for {discover_secs}s...");
        discover::browse(Duration::from_secs(discover_secs), |f| {
            println!("{}  {}  fp={}", f.name, f.addr, f.fingerprint);
            true
        })
        .await?;
        return Ok(());
    }

    let dir = dir.ok_or(USAGE)?;
    let state = state.unwrap_or_else(default_state_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("can't use {}: {e}", dir.display()))?;
    let engine = Engine::new(
        tokio::runtime::Handle::current(),
        &state,
        Arc::new(DirSink { root: dir.clone() }),
        Arc::new(LogListener { last: Mutex::new((String::new(), 0)) }),
        &crate::core::device_name(),
    );

    if let Some(link) = link {
        let l = pairlink::parse(&link)?;
        let h = engine.pair(&l.addr, Some(&l.fingerprint), &l.code).await?;
        println!("paired with {} ({})", h.name, h.fingerprint);
    } else if let Some(addr) = host {
        let code = code.ok_or("--host needs --code too")?;
        if !pairlink::is_code(&code) {
            return Err("the code is the 6 digits shown on the frame".into());
        }
        let fp = match fp {
            Some(f) => Some(crate::core::tls::normalize_fingerprint(&f).ok_or("that fingerprint doesn't look right")?),
            None => None,
        };
        let h = engine.pair(&addr, fp.as_deref(), &code).await?;
        println!("paired with {} ({})", h.name, h.fingerprint);
    }

    if engine.hosts().is_empty() {
        return Err("not paired with any frame yet, use --pair or --host/--code".into());
    }
    println!("syncing into {} (state in {})", dir.display(), state.display());
    println!("note: this only works while the frame is on, on the same wi-fi, and framecorder is running on it");
    engine.start_all();
    match exit_after {
        Some(secs) => tokio::time::sleep(Duration::from_secs(secs)).await,
        None => std::future::pending::<()>().await,
    }
    Ok(())
}

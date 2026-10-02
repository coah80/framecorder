//! framecorder-sync: serves finished recordings and clips to paired devices
//! on the same Wi-Fi. See README.md for the protocol.

mod config;
mod devices;
mod events;
mod frame;
mod http;
mod library;
mod mdns;
mod openvr;
mod recording;
mod remote;
mod server;
mod throttle;
mod watch;

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use config::{Info, Paths, Settings, DEFAULT_GAME_RATE_MB, DEFAULT_PORT, PROTOCOL_VERSION};

struct Args {
    videos: Option<PathBuf>,
    state: Option<PathBuf>,
    port: Option<u16>,
    mdns: bool,
}

const USAGE: &str = "usage: framecorder-sync [--videos DIR] [--state DIR] [--port N] [--no-mdns]";

fn parse_args() -> Result<Args, String> {
    let mut args = Args { videos: None, state: None, port: None, mdns: true };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--videos" => args.videos = Some(value()?.into()),
            "--state" => args.state = Some(value()?.into()),
            "--port" => args.port = Some(value()?.parse().map_err(|_| "--port wants a number".to_string())?),
            "--no-mdns" => args.mdns = false,
            "-h" | "--help" => return Err(USAGE.into()),
            _ => return Err(format!("unknown argument {arg}\n{USAGE}")),
        }
    }
    Ok(args)
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(args) {
        log::error!("{e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> std::io::Result<()> {
    let defaults = Paths::from_env()?;
    let paths = Paths {
        power: defaults.power.clone(),
        ..Paths::new(args.videos.as_deref().unwrap_or(&defaults.videos), args.state.as_deref().unwrap_or(&defaults.state))
    };
    watch::ensure_dirs(&paths)?;
    let settings = Settings::load(&paths);
    let name = config::device_name(&settings);
    let identity = config::identity(&paths.state)?;
    let tls = config::tls_config(&identity)?;

    let library = Arc::new(library::Library::new(&paths));
    let hub = Arc::new(events::Hub::default());
    let devices = Arc::new(devices::Devices::load(&paths.devices()));
    let watcher = watch::Watcher::new(&paths, library.clone(), hub.clone(), devices.clone())?;

    let port = args.port.or(settings.port).unwrap_or(DEFAULT_PORT);
    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            log::warn!("port {port} is taken, picking another one");
            TcpListener::bind(("0.0.0.0", 0))?
        }
        Err(e) => return Err(e),
    };
    let port = listener.local_addr()?.port();

    let info = Info { name: &name, port, fingerprint: &identity.fingerprint, version: PROTOCOL_VERSION };
    config::write_atomic(&paths.info(), &serde_json::to_vec_pretty(&info).unwrap_or_default())?;

    let _mdns = if args.mdns {
        match mdns::advertise(&name, &identity.fingerprint, port) {
            Ok(d) => Some(d),
            Err(e) => {
                log::warn!("couldn't advertise over mDNS, devices will need the address: {e}");
                None
            }
        }
    } else {
        None
    };

    let throttle = throttle::Throttle::new(settings.game_rate_mb.unwrap_or(DEFAULT_GAME_RATE_MB));
    let lib_path = openvr::library_path();
    let checker = throttle.clone();
    std::thread::Builder::new()
        .name("game-check".into())
        .stack_size(256 * 1024)
        .spawn(move || checker.run_checker(|| openvr::scene_running(&lib_path)))?;
    std::thread::Builder::new().name("watch".into()).spawn(move || watcher.run())?;

    log::info!(
        "serving {} clips as {name:?} on port {port}, {} device(s) paired, fingerprint {}",
        library.list().len(),
        devices.count(),
        identity.fingerprint
    );
    let state = Arc::new(server::State {
        paths: paths.clone(),
        name,
        fingerprint: identity.fingerprint,
        tls,
        library,
        hub,
        devices,
        pairing: devices::Pairing::new(&paths.pairing()),
        throttle,
        remote: remote::Remote::default(),
        connections: AtomicUsize::new(0),
    });
    server::serve(listener, state);
    Ok(())
}

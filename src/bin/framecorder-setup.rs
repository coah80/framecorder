//! What the installer runs once the release is downloaded:
//! puts framecorder where it lives on the headset and starts its services.
//! Safe to run again, it's how updates get installed too.

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_target(false)
        .init();
    let result = match std::env::args().nth(1).as_deref() {
        // What the update timer runs, every few hours.
        Some("--update") => framecorder::setup::update(),
        // The new release's installer, run by the old one's --update.
        Some("--update-install") => framecorder::setup::run(false).map(|_| ()),
        // The installer's unlock, once it has the password.
        Some("--unlock") => framecorder::setup::unlock(),
        _ => framecorder::setup::run(true).map(|report| {
            for line in report.lines() {
                log::info!("{line}");
            }
        }),
    };
    if let Err(e) = result {
        log::error!("{e:#}");
        std::process::exit(1);
    }
}

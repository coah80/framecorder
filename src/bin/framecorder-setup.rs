//! What site/install (and the flatpak) run once the release is downloaded:
//! puts framecorder where it lives on the headset and starts its services.
//! Safe to run again, it's how updates get installed too.

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_target(false)
        .init();
    match framecorder::setup::run() {
        Ok(report) => {
            for line in report.lines() {
                log::info!("{line}");
            }
        }
        Err(e) => {
            log::error!("{e:#}");
            std::process::exit(1);
        }
    }
}

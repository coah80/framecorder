//! The installer Frame Drop launches from the Steam library: puts
//! framecorder where it lives on the headset, starts its services, and
//! opens the dashboard on the new tab. Safe to run again, it's how updates
//! get installed too.

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

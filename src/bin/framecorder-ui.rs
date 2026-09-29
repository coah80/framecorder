//! Dashboard tab for starting and stopping recordings from inside the headset.

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_target(false)
        .init();
    let args: Vec<String> = std::env::args().collect();
    let result = match args.iter().position(|a| a == "--preview") {
        Some(i) if args.len() > i + 2 => framecorder::ui::preview(args[i + 1].as_ref(), &args[i + 2]),
        _ => framecorder::ui::run(),
    };
    if let Err(e) = result {
        log::error!("{e:#}");
        std::process::exit(1);
    }
}

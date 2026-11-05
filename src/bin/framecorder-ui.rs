//! Dashboard tab for starting and stopping recordings from inside the headset.

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_target(false)
        .init();
    let args: Vec<String> = std::env::args().collect();
    let result = match args.iter().position(|a| a == "--preview") {
        Some(i) if args.len() > i + 2 => {
            framecorder::ui::preview(args[i + 1].as_ref(), &args[i + 2])
        }
        Some(_) => Err(anyhow::anyhow!("--preview needs a file and a state")),
        _ if args.iter().any(|a| a == "--demo") => {
            let port = args.iter().position(|a| a == "--demo").and_then(|i| args.get(i + 1)).and_then(|p| p.parse().ok());
            framecorder::ui::demo(port.unwrap_or(4796))
        }
        _ if args.iter().any(|a| a == "--diagnostics") => {
            println!("{}", framecorder::report::collect());
            Ok(())
        }
        _ if args.iter().any(|a| a == "--report") => {
            let description = args
                .iter()
                .position(|a| a == "--report")
                .and_then(|i| args.get(i + 1));
            match description {
                Some(description) => {
                    framecorder::report::send(description, &framecorder::report::collect())
                        .map(|sent| println!("{}", sent.url))
                }
                None => Err(anyhow::anyhow!("describe what went wrong after --report")),
            }
        }
        _ if args.iter().any(|a| a == "--uninstall") => framecorder::setup::uninstall(),
        _ => framecorder::ui::run(),
    };
    if let Err(e) = result {
        log::error!("{e:#}");
        std::process::exit(1);
    }
}

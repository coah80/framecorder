// no console window on Windows (headless mode attaches to its parent's)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let headless = args.iter().any(|a| matches!(a.as_str(), "--headless-sync" | "--discover" | "-h" | "--help"));
    if headless {
        std::process::exit(framecorder_app_lib::headless::main(args));
    }
    #[cfg(feature = "gui")]
    framecorder_app_lib::run();
    #[cfg(not(feature = "gui"))]
    {
        eprintln!("built without the gui, use --headless-sync <dir>");
        std::process::exit(2);
    }
}

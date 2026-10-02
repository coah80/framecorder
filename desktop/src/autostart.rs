//! "start with the computer": a .desktop file in ~/.config/autostart, a
//! registry Run key on windows, a launch agent on macos.

use auto_launch::{AutoLaunch, AutoLaunchBuilder, MacOSLaunchMode};

fn launcher() -> Option<AutoLaunch> {
    let exe = std::env::current_exe().ok()?;
    AutoLaunchBuilder::new()
        .set_app_name("framecorder")
        .set_app_path(&exe.to_string_lossy())
        // started with the computer: straight to the tray
        .set_args(&["--minimized"])
        .set_macos_launch_mode(MacOSLaunchMode::LaunchAgent)
        .build()
        .ok()
}

pub fn is_enabled() -> Option<bool> {
    launcher()?.is_enabled().ok()
}

pub fn set(enabled: bool) -> Result<bool, String> {
    let l = launcher().ok_or("can't set that up on this computer")?;
    if enabled { l.enable() } else { l.disable() }.map_err(|e| e.to_string())?;
    l.is_enabled().map_err(|e| e.to_string())
}

//! framecorder's companion app: pulls clips off a Steam Frame over Wi-Fi.

pub mod core;
#[cfg(feature = "gui")]
mod gui;
pub mod headless;

#[cfg(feature = "gui")]
pub fn run() {
    gui::run()
}

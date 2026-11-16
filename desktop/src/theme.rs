//! catppuccin mocha with a mauve accent, same as the old app and the site.

use gpui::{rgb, rgba, Rgba};

pub const CRUST: u32 = 0x11111b;
pub const MANTLE: u32 = 0x181825;
pub const BASE: u32 = 0x1e1e2e;
pub const SURFACE0: u32 = 0x313244;
pub const SURFACE1: u32 = 0x45475a;
pub const SURFACE2: u32 = 0x585b70;
pub const OVERLAY2: u32 = 0x9399b2;
pub const SUBTEXT0: u32 = 0xa6adc8;
pub const SUBTEXT1: u32 = 0xbac2de;
pub const TEXT: u32 = 0xcdd6f4;
pub const MAUVE: u32 = 0xcba6f7;
pub const MAUVE_LIGHT: u32 = 0xddc4fb;
pub const RED: u32 = 0xf38ba8;
pub const RED_LIGHT: u32 = 0xf7a8bf;
pub const PEACH: u32 = 0xfab387;
pub const YELLOW: u32 = 0xf9e2af;
pub const GREEN: u32 = 0xa6e3a1;

pub const HEADING: &str = "Montserrat";
pub const BODY: &str = "Poppins";
pub const DATA: &str = "Space Grotesk";

pub fn c(hex: u32) -> Rgba {
    rgb(hex)
}

/// a color at some alpha, 0 to 1
pub fn a(hex: u32, alpha: f32) -> Rgba {
    rgba((hex << 8) | ((alpha.clamp(0.0, 1.0) * 255.0).round() as u32))
}

/// the faint hairline between rows and around cards
pub fn line() -> Rgba {
    a(TEXT, 0.06)
}

pub fn card() -> Rgba {
    a(SURFACE0, 0.4)
}

pub fn card_line() -> Rgba {
    a(SURFACE1, 0.55)
}

/// placeholder tones for clips without a thumbnail yet, so the grid isn't one flat color
pub const TONES: [u32; 8] = [0x2b2540, 0x1f2b3a, 0x23302b, 0x33282c, 0x1f2d33, 0x2e2a22, 0x272a3d, 0x22302f];

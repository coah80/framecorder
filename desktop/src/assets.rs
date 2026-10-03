//! the fonts and icons, baked into the binary so there's nothing to install.

use std::borrow::Cow;

use gpui::{App, AssetSource, Result, SharedString};

const FONTS: [&[u8]; 4] = [
    include_bytes!("../../app/ui/fonts/Montserrat-Bold.ttf"),
    include_bytes!("../../app/ui/fonts/Poppins-Regular.ttf"),
    include_bytes!("../../app/ui/fonts/Poppins-Medium.ttf"),
    include_bytes!("../../app/ui/fonts/SpaceGrotesk-Medium.ttf"),
];

pub fn load_fonts(cx: &mut App) {
    let fonts = FONTS.iter().map(|f| Cow::Borrowed(*f)).collect();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        log::warn!("couldn't load the fonts, falling back to the system's: {e}");
    }
}

/// the icons, as stroke paths in a 24x24 box. gpui paints svgs as a mask in
/// the text color, so the stroke color here doesn't matter
const ICONS: &[(&str, &str)] = &[
    ("clips", r#"<rect x="3" y="5" width="18" height="14" rx="3"/><path d="m10 9 5 3-5 3z"/>"#),
    (
        "settings",
        r#"<path d="M4 7h10M18 7h2M4 17h2M10 17h10"/><circle cx="16" cy="7" r="2"/><circle cx="8" cy="17" r="2"/>"#,
    ),
    ("folder", r#"<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#),
    ("play", r#"<path d="m9 7 9 5-9 5z"/>"#),
    (
        "grid",
        r#"<rect x="4" y="4" width="7" height="7" rx="1.5"/><rect x="13" y="4" width="7" height="7" rx="1.5"/><rect x="4" y="13" width="7" height="7" rx="1.5"/><rect x="13" y="13" width="7" height="7" rx="1.5"/>"#,
    ),
    ("list", r#"<path d="M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01"/>"#),
    ("download", r#"<path d="M12 4v11M7 11l5 5 5-5M5 20h14"/>"#),
    ("restart", r#"<path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7"/>"#),
    ("spinner", r#"<path d="M21 12a9 9 0 1 1-6.2-8.6"/>"#),
    ("wifi", r#"<path d="M2 8.8a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16.4a5 5 0 0 1 7 0M12 20h.01"/>"#),
    (
        "wifi-off",
        r#"<path d="M2 8.8a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16.4a5 5 0 0 1 7 0M12 20h.01M3 3l18 18"/>"#,
    ),
    ("power", r#"<path d="M12 3v9M6.3 6.3a8 8 0 1 0 11.4 0"/>"#),
    ("bolt", r#"<path d="M13 3 4 14h7l-1 7 9-11h-7z"/>"#),
    ("lock", r#"<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>"#),
    ("plus", r#"<path d="M12 5v14M5 12h14"/>"#),
    (
        "headset",
        r#"<path d="M3 10a3 3 0 0 1 3-3h12a3 3 0 0 1 3 3v3a3 3 0 0 1-3 3h-3l-1.5-2h-3L9 16H6a3 3 0 0 1-3-3z"/>"#,
    ),
    ("disk", r#"<rect x="3" y="6" width="18" height="12" rx="2"/><path d="M7 14h.01M11 14h6"/>"#),
    ("x", r#"<path d="M6 6l12 12M18 6 6 18"/>"#),
    ("search", r#"<circle cx="11" cy="11" r="6"/><path d="m20 20-4.5-4.5"/>"#),
    ("unlink", r#"<path d="m10 6 1-1a4 4 0 0 1 5.7 5.7l-1 1M14 18l-1 1a4 4 0 0 1-5.7-5.7l1-1M4 4l16 16"/>"#),
    ("clip", r#"<circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M20 4 8.1 15.9M14.5 14.5 20 20M8.1 8.1 12 12"/>"#),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let Some(name) = path.strip_prefix("icons/").and_then(|p| p.strip_suffix(".svg")) else {
            return Ok(None);
        };
        Ok(ICONS.iter().find(|(n, _)| *n == name).map(|(_, body)| {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#
            );
            Cow::Owned(svg.into_bytes())
        }))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(if path.starts_with("icons") {
            ICONS.iter().map(|(n, _)| SharedString::from(format!("icons/{n}.svg"))).collect()
        } else {
            Vec::new()
        })
    }
}

/// the app icon, 256 px, for the window, the tray and the linux app menu
pub const APP_ICON_PNG: &[u8] = include_bytes!("../../app/icons/128x128@2x.png");

pub fn app_icon() -> Option<image::RgbaImage> {
    image::load_from_memory_with_format(APP_ICON_PNG, image::ImageFormat::Png).ok().map(|i| i.to_rgba8())
}

pub fn icon_path(name: &str) -> SharedString {
    format!("icons/{name}.svg").into()
}

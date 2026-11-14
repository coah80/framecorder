//! the fonts and icons, baked into the binary so there's nothing to install.

use std::borrow::Cow;

use gpui::{App, AssetSource, Result, SharedString};

const FONTS: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/Montserrat-Bold.ttf"),
    include_bytes!("../../assets/fonts/Poppins-Regular.ttf"),
    include_bytes!("../../assets/fonts/Poppins-Medium.ttf"),
    include_bytes!("../../assets/fonts/SpaceGrotesk-Medium.ttf"),
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
    ("keyboard", r#"<rect x="3" y="6" width="18" height="12" rx="2"/><path d="M7 10h.01M11 10h.01M15 10h.01M7 14h10"/>"#),
    ("check", r#"<path d="m5 12 5 5 9-10"/>"#),
    ("retry", r#"<path d="M4 11a8 8 0 0 1 14-4.5M4 4v6h6M20 13a8 8 0 0 1-14 4.5M20 20v-6h-6"/>"#),
    ("back", r#"<path d="M15 5l-7 7 7 7"/>"#),
    ("chevron-down", r#"<path d="m6 9 6 6 6-6"/>"#),
    ("chevron-up", r#"<path d="m6 15 6-6 6 6"/>"#),
    ("info", r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>"#),
    ("mic", r#"<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>"#),
    ("mic-off", r#"<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3M3 3l18 18"/>"#),
    ("trash", r#"<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>"#),
    ("bug", r#"<rect x="8" y="8" width="8" height="12" rx="4"/><path d="M12 12v8M8 13H5M8 17l-2 2M16 13h3M16 17l2 2M9.5 8 8 5.5M14.5 8 16 5.5"/>"#),
    // discord's own mark (from simple icons), filled where the rest are strokes
    (
        "discord",
        r#"<path fill="black" stroke="none" d="M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189Z"/>"#,
    ),
    (
        "live",
        r#"<circle cx="12" cy="12" r="2"/><path d="M8.5 8.5a5 5 0 0 0 0 7M15.5 8.5a5 5 0 0 1 0 7M5.6 5.6a9 9 0 0 0 0 12.8M18.4 5.6a9 9 0 0 1 0 12.8"/>"#,
    ),
    ("copy", r#"<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/>"#),
    ("external", r#"<path d="M14 4h6v6M20 4l-9 9M19 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h5"/>"#),
    ("fullscreen", r#"<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>"#),
    ("exit-fullscreen", r#"<path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5"/>"#),
    ("eye", r#"<path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7-10-7-10-7z"/><circle cx="12" cy="12" r="3"/>"#),
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
pub const APP_ICON_PNG: &[u8] = include_bytes!("../../assets/icons/128x128@2x.png");

pub fn app_icon() -> Option<image::RgbaImage> {
    image::load_from_memory_with_format(APP_ICON_PNG, image::ImageFormat::Png).ok().map(|i| i.to_rgba8())
}

pub fn icon_path(name: &str) -> SharedString {
    format!("icons/{name}.svg").into()
}

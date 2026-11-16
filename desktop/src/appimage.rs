//! the linux build ships as an AppImage. it runs from a read-only mount that
//! moves every launch, so whatever points back at the app (start with the
//! computer, the app menu, the updater) points at the AppImage file instead.

use std::path::PathBuf;

/// the AppImage file, when we're running from one
pub fn file() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file())
}

/// what to start to get this app again
pub fn launch_path() -> std::io::Result<PathBuf> {
    file().map(Ok).unwrap_or_else(std::env::current_exe)
}

/// the app id, which wayland matches to the menu entry's file name
#[cfg(target_os = "linux")]
const ID: &str = "com.framecorder.desktop";

/// puts framecorder in the app menu, pointing at this AppImage, the closest an
/// AppImage gets to being installed. written again when the AppImage moved
#[cfg(target_os = "linux")]
pub fn integrate(icon_png: &[u8]) {
    let (Some(file), Some(data)) = (file(), dirs::data_dir()) else {
        return;
    };
    let Some(exec) = exec_quote(&file.to_string_lossy()) else {
        log::warn!("not adding {} to the app menu, its path has characters a menu entry can't hold", file.display());
        return;
    };
    let entry = data.join("applications").join(format!("{ID}.desktop"));
    let text = format!(
        "[Desktop Entry]\nType=Application\nName=framecorder\nComment=clips and recordings from your steam frame\n\
         Exec={exec}\nIcon=framecorder\nTerminal=false\nCategories=AudioVideo;Video;\nStartupWMClass={ID}\n"
    );
    if std::fs::read_to_string(&entry).is_ok_and(|now| now == text) {
        return;
    }
    let icon = data.join("icons/hicolor/256x256/apps/framecorder.png");
    let wrote = [(&icon, icon_png), (&entry, text.as_bytes())].into_iter().try_for_each(|(path, bytes)| {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, bytes)
    });
    match wrote {
        Ok(()) => log::info!("added framecorder to the app menu, at {}", file.display()),
        Err(e) => log::warn!("couldn't add framecorder to the app menu: {e}"),
    }
}

/// a path as a menu entry's Exec, in double quotes. None for the few
/// characters that would need escaping twice over
#[cfg(target_os = "linux")]
fn exec_quote(path: &str) -> Option<String> {
    if path.chars().any(|c| matches!(c, '"' | '`' | '$' | '\\' | '\n')) {
        return None;
    }
    Some(format!("\"{}\"", path.replace('%', "%%")))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::exec_quote;

    #[test]
    fn menu_entry_paths() {
        assert_eq!(exec_quote("/home/a b/framecorder.AppImage").as_deref(), Some("\"/home/a b/framecorder.AppImage\""));
        assert_eq!(exec_quote("/home/a/100%.AppImage").as_deref(), Some("\"/home/a/100%%.AppImage\""));
        assert_eq!(exec_quote("/home/a/$x.AppImage"), None);
    }
}

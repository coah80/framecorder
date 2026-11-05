//! putting video files themselves on the clipboard, so they paste into discord,
//! a chat, a file manager or an editor as the files. gpui's clipboard only
//! writes text and images (its `ExternalPaths` entry is dropped by every
//! platform's writer), so each os gets the small bit of native code it needs.

use std::path::PathBuf;

/// the files pasted anywhere take a copy. `Err` is plain words for a toast
pub fn copy_files(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("nothing to copy".into());
    }
    os::copy(paths)
}

/// `file://` uris, one per line, the way `text/uri-list` wants them. anything
/// outside the unreserved set gets percent-encoded, so spaces and non-ascii
/// names survive the trip
#[cfg(any(target_os = "linux", test))]
fn uri_list(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| file_uri(p) + "\r\n").collect()
}

#[cfg(any(target_os = "linux", test))]
fn file_uri(path: &std::path::Path) -> String {
    use std::fmt::Write as _;
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => uri.push(byte as char),
            _ => {
                // writing to a String can't fail
                let _ = write!(uri, "%{byte:02X}");
            }
        }
    }
    uri
}

/// a `CF_HDROP` payload: the `DROPFILES` header, then each path in utf-16
/// with a nul, then one more nul to end the list
#[cfg(any(windows, test))]
fn hdrop(paths: &[PathBuf]) -> Vec<u16> {
    // DROPFILES, as little-endian u16 pairs: pFiles = 20 (the header's own
    // size), pt.x, pt.y, fNC all 0, fWide = 1
    let mut out: Vec<u16> = vec![20, 0, 0, 0, 0, 0, 0, 0, 1, 0];
    for path in paths {
        out.extend(path.as_os_str().to_string_lossy().encode_utf16());
        out.push(0);
    }
    out.push(0);
    out
}

#[cfg(target_os = "linux")]
mod os {
    //! a tiny x client that owns the CLIPBOARD selection and answers paste
    //! requests until another app copies something. on wayland the
    //! compositor bridges it through xwayland, which every desktop has, so
    //! one path covers both. the thread exits when the selection is taken

    use std::path::PathBuf;
    use std::sync::mpsc;

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, SelectionNotifyEvent,
        SelectionRequestEvent, WindowClass, SELECTION_NOTIFY_EVENT,
    };
    use x11rb::protocol::Event;
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;
    use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME};

    /// what a paste can ask for: the files for file managers, discord and
    /// editors, the paths as text for a terminal
    const FILE_TARGETS: [&str; 2] = ["text/uri-list", "x-special/gnome-copied-files"];
    const TEXT_TARGETS: [&str; 4] = ["UTF8_STRING", "text/plain;charset=utf-8", "text/plain", "STRING"];

    struct Offer {
        uris: String,
        text: String,
    }

    pub fn copy(paths: &[PathBuf]) -> Result<(), String> {
        let offer = Offer {
            uris: super::uri_list(paths),
            text: paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n"),
        };
        let (ready, owned) = mpsc::channel();
        std::thread::Builder::new()
            .name("clipboard".into())
            .spawn(move || serve(offer, ready))
            .map_err(|e| format!("couldn't start the clipboard thread: {e}"))?;
        owned.recv().map_err(|_| "the clipboard thread died".to_string())?
    }

    fn serve(offer: Offer, ready: mpsc::Sender<Result<(), String>>) {
        let owner = match take(&offer) {
            Ok(owner) => owner,
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        };
        let _ = ready.send(Ok(()));
        if let Err(e) = answer(&owner, &offer) {
            log::warn!("clipboard: {e}");
        }
    }

    struct Owner {
        conn: RustConnection,
        window: u32,
        clipboard: Atom,
        targets: Atom,
        files: Vec<Atom>,
        text: Vec<Atom>,
    }

    fn take(offer: &Offer) -> Result<Owner, String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| format!("no x11 display to put it on ({e})"))?;
        let root = conn.setup().roots[screen].root;
        let window = conn.generate_id().map_err(|e| e.to_string())?;
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            COPY_FROM_PARENT,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(|e| e.to_string())?;
        let atom = |name: &str| -> Result<Atom, String> {
            Ok(conn.intern_atom(false, name.as_bytes()).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom)
        };
        let clipboard = atom("CLIPBOARD")?;
        let targets = atom("TARGETS")?;
        let files = FILE_TARGETS.iter().map(|t| atom(t)).collect::<Result<Vec<_>, _>>()?;
        let text = TEXT_TARGETS.iter().map(|t| atom(t)).collect::<Result<Vec<_>, _>>()?;
        conn.set_selection_owner(window, clipboard, CURRENT_TIME).map_err(|e| e.to_string())?;
        let now = conn.get_selection_owner(clipboard).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        if now.owner != window {
            return Err("something else holds the clipboard".into());
        }
        conn.flush().map_err(|e| e.to_string())?;
        log::debug!("clipboard: holding {} bytes of uris", offer.uris.len());
        Ok(Owner { conn, window, clipboard, targets, files, text })
    }

    fn answer(owner: &Owner, offer: &Offer) -> Result<(), String> {
        loop {
            match owner.conn.wait_for_event().map_err(|e| e.to_string())? {
                Event::SelectionRequest(req) if req.owner == owner.window => {
                    let property = if req.property == Atom::from(AtomEnum::NONE) { req.target } else { req.property };
                    let given = give(owner, offer, &req, property).map_err(|e| e.to_string())?;
                    let notify = SelectionNotifyEvent {
                        response_type: SELECTION_NOTIFY_EVENT,
                        sequence: 0,
                        time: req.time,
                        requestor: req.requestor,
                        selection: req.selection,
                        target: req.target,
                        property: if given { property } else { Atom::from(AtomEnum::NONE) },
                    };
                    owner.conn.send_event(false, req.requestor, EventMask::NO_EVENT, notify).map_err(|e| e.to_string())?;
                    owner.conn.flush().map_err(|e| e.to_string())?;
                }
                Event::SelectionClear(ev) if ev.selection == owner.clipboard => return Ok(()),
                _ => {}
            }
        }
    }

    /// writes what was asked for onto the requestor's window. false means
    /// we don't have that target
    fn give(owner: &Owner, offer: &Offer, req: &SelectionRequestEvent, property: Atom) -> Result<bool, x11rb::errors::ConnectionError> {
        let conn = &owner.conn;
        if req.target == owner.targets {
            let mut list = vec![owner.targets];
            list.extend(&owner.files);
            list.extend(&owner.text);
            conn.change_property32(PropMode::REPLACE, req.requestor, property, AtomEnum::ATOM, &list)?;
            return Ok(true);
        }
        let body = if owner.files.contains(&req.target) {
            // gnome's own format says what to do with the files, then lists them
            if req.target == owner.files[1] {
                format!("copy\n{}", offer.uris.replace("\r\n", "\n").trim_end())
            } else {
                offer.uris.clone()
            }
        } else if owner.text.contains(&req.target) {
            offer.text.clone()
        } else {
            return Ok(false);
        };
        conn.change_property8(PropMode::REPLACE, req.requestor, property, req.target, body.as_bytes())?;
        Ok(true)
    }
}

#[cfg(windows)]
mod os {
    //! `CF_HDROP`, what explorer, discord and everything else read files from

    use std::path::PathBuf;

    use windows_sys::Win32::Foundation::{GlobalFree, HANDLE};
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows_sys::Win32::System::Ole::CF_HDROP;

    pub fn copy(paths: &[PathBuf]) -> Result<(), String> {
        let payload = super::hdrop(paths);
        let bytes = payload.len() * std::mem::size_of::<u16>();
        unsafe {
            let global = GlobalAlloc(GMEM_MOVEABLE, bytes);
            if global.is_null() {
                return Err("out of memory for the clipboard".into());
            }
            let dest = GlobalLock(global);
            if dest.is_null() {
                GlobalFree(global);
                return Err("couldn't lock clipboard memory".into());
            }
            std::ptr::copy_nonoverlapping(payload.as_ptr(), dest as *mut u16, payload.len());
            GlobalUnlock(global);
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                GlobalFree(global);
                return Err("another app has the clipboard open, try again".into());
            }
            let set = EmptyClipboard() != 0 && !SetClipboardData(CF_HDROP as u32, global as HANDLE).is_null();
            CloseClipboard();
            if set {
                // the clipboard owns the memory now
                Ok(())
            } else {
                GlobalFree(global);
                Err("windows refused the files".into())
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod os {
    //! file urls on the general pasteboard, which finder, discord and
    //! everything else take as the files

    use std::path::PathBuf;

    use cocoa::appkit::NSPasteboard;
    use cocoa::base::{id, nil, YES};
    use cocoa::foundation::{NSArray, NSAutoreleasePool, NSString, NSURL};

    pub fn copy(paths: &[PathBuf]) -> Result<(), String> {
        unsafe {
            let pool = NSAutoreleasePool::new(nil);
            let urls: Vec<id> = paths
                .iter()
                .map(|p| NSURL::fileURLWithPath_(nil, NSString::alloc(nil).init_str(&p.to_string_lossy()).autorelease()))
                .collect();
            let pasteboard = NSPasteboard::generalPasteboard(nil);
            pasteboard.clearContents();
            let written = pasteboard.writeObjects(NSArray::arrayWithObjects(nil, &urls)) == YES;
            pool.drain();
            if written {
                Ok(())
            } else {
                Err("the pasteboard refused the files".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_keep_plain_paths_and_escape_the_rest() {
        let cases: [(&str, &str); 4] = [
            ("/home/me/Videos/framecorder/2026-11-05_15-42-00.mp4", "file:///home/me/Videos/framecorder/2026-11-05_15-42-00.mp4"),
            ("/home/me/my clips/a.mp4", "file:///home/me/my%20clips/a.mp4"),
            ("/home/me/Vidéos/ü.mp4", "file:///home/me/Vid%C3%A9os/%C3%BC.mp4"),
            ("/tmp/50% done #1.mp4", "file:///tmp/50%25%20done%20%231.mp4"),
        ];
        for (path, uri) in cases {
            assert_eq!(file_uri(std::path::Path::new(path)), uri, "{path}");
        }
        let two = uri_list(&[PathBuf::from("/a.mp4"), PathBuf::from("/b.mp4")]);
        assert_eq!(two, "file:///a.mp4\r\nfile:///b.mp4\r\n");
    }

    #[test]
    fn hdrop_is_a_wide_list_after_a_20_byte_header() {
        let got = hdrop(&[PathBuf::from(r"C:\v\a.mp4"), PathBuf::from(r"C:\v\b c.mp4")]);
        assert_eq!(&got[..2], &[20, 0], "pFiles points past the header");
        assert_eq!(got[8], 1, "fWide");
        let mut want: Vec<u16> = Vec::new();
        for p in [r"C:\v\a.mp4", r"C:\v\b c.mp4"] {
            want.extend(p.encode_utf16());
            want.push(0);
        }
        want.push(0);
        assert_eq!(&got[10..], &want[..]);
    }
}

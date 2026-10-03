//! the tray icon (menu bar on a mac), so syncing carries on with the window
//! closed. on linux it's a StatusNotifierItem over dbus, which kde, and gnome
//! with the usual extension, show. no gtk, so it doesn't fight gpui's loop.

use std::cell::Cell;

use framecorder_app_lib::core::api::Remote;
use framecorder_app_lib::core::engine::{State, Status};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub enum TrayMsg {
    Open,
    SyncNow,
    /// start or stop recording, whichever the frame's up for
    Record,
    Clip,
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    menu: Menu,
    /// the remote: only in the menu while a frame can take commands
    record: MenuItem,
    clip: MenuItem,
    remote_sep: PredefinedMenuItem,
    remote_shown: Cell<bool>,
}

/// makes the tray icon, and sends what's clicked on it down `tx`
pub fn create(icon: &image::RgbaImage, tx: async_channel::Sender<TrayMsg>) -> Result<Tray, String> {
    let open = MenuItem::with_id("open", "Open framecorder", true, None);
    let sync = MenuItem::with_id("sync", "Sync now", true, None);
    let quit = MenuItem::with_id("quit", "Quit", true, None);
    let record = MenuItem::with_id("record", "Start recording", true, None);
    let clip = MenuItem::with_id("clip", "Save clip", true, None);
    let menu = Menu::with_items(&[&open, &sync, &PredefinedMenuItem::separator(), &quit]).map_err(|e| e.to_string())?;

    let (w, h) = icon.dimensions();
    let icon = Icon::from_rgba(icon.as_raw().clone(), w, h).map_err(|e| e.to_string())?;

    let menu_tx = tx.clone();
    MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
        let msg = match ev.id().as_ref() {
            "open" => TrayMsg::Open,
            "sync" => TrayMsg::SyncNow,
            "record" => TrayMsg::Record,
            "clip" => TrayMsg::Clip,
            "quit" => TrayMsg::Quit,
            _ => return,
        };
        let _ = menu_tx.try_send(msg);
    }));
    TrayIconEvent::set_event_handler(Some(move |ev: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
            let _ = tx.try_send(TrayMsg::Open);
        }
    }));

    let icon = TrayIconBuilder::new()
        .with_id("framecorder")
        .with_tooltip("framecorder")
        .with_menu(Box::new(menu.clone()))
        .with_menu_on_left_click(false)
        .with_icon(icon)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(Tray { icon, menu, record, clip, remote_sep: PredefinedMenuItem::separator(), remote_shown: Cell::new(false) })
}

impl Tray {
    /// what hovering the icon says, and whether the remote's in the menu.
    /// `remote` is the frame that takes commands right now, if one does
    pub fn refresh(&self, statuses: &[Status], remote: Option<(&Status, &Remote)>) {
        let text = match (remote, statuses.first()) {
            (Some((s, r)), _) if r.recording => format!("framecorder: {} is recording", s.name),
            (_, None) => "framecorder: not paired yet".to_string(),
            (_, Some(s)) => match s.state {
                State::Connected => format!("framecorder: connected to {}", s.name),
                State::Connecting => format!("framecorder: looking for {}", s.name),
                State::Unreachable => format!("framecorder: can't reach {}, is it on?", s.name),
                State::Full => "framecorder: this computer is out of space".to_string(),
                State::Unpaired | State::WrongFingerprint => format!("framecorder: {} needs pairing again", s.name),
            },
        };
        let _ = self.icon.set_tooltip(Some(text));

        match remote {
            Some((_, r)) => {
                if !self.remote_shown.get() {
                    // under "Open framecorder", above everything else
                    let put = self
                        .menu
                        .insert(&self.remote_sep, 1)
                        .and_then(|_| self.menu.insert(&self.record, 2))
                        .and_then(|_| self.menu.insert(&self.clip, 3));
                    match put {
                        Ok(()) => self.remote_shown.set(true),
                        Err(e) => log::warn!("couldn't put the remote in the tray menu: {e}"),
                    }
                }
                self.record.set_text(if r.recording { "Stop recording" } else { "Start recording" });
                self.clip.set_enabled(r.clip_ready && !r.recording);
            }
            None if self.remote_shown.get() => {
                let _ = self.menu.remove(&self.clip);
                let _ = self.menu.remove(&self.record);
                let _ = self.menu.remove(&self.remote_sep);
                self.remote_shown.set(false);
            }
            None => {}
        }
    }

    pub fn set_visible(&self, visible: bool) {
        let _ = self.icon.set_visible(visible);
    }
}

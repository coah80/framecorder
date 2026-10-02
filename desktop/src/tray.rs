//! the tray icon (menu bar on a mac), so syncing carries on with the window
//! closed. on linux it's a StatusNotifierItem over dbus, which kde, and gnome
//! with the usual extension, show. no gtk, so it doesn't fight gpui's loop.

use framecorder_app_lib::core::engine::{State, Status};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub enum TrayMsg {
    Open,
    SyncNow,
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
}

/// makes the tray icon, and sends what's clicked on it down `tx`
pub fn create(icon: &image::RgbaImage, tx: async_channel::Sender<TrayMsg>) -> Result<Tray, String> {
    let open = MenuItem::with_id("open", "Open framecorder", true, None);
    let sync = MenuItem::with_id("sync", "Sync now", true, None);
    let quit = MenuItem::with_id("quit", "Quit", true, None);
    let menu = Menu::with_items(&[&open, &sync, &PredefinedMenuItem::separator(), &quit]).map_err(|e| e.to_string())?;

    let (w, h) = icon.dimensions();
    let icon = Icon::from_rgba(icon.as_raw().clone(), w, h).map_err(|e| e.to_string())?;

    let menu_tx = tx.clone();
    MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
        let msg = match ev.id().as_ref() {
            "open" => TrayMsg::Open,
            "sync" => TrayMsg::SyncNow,
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
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_icon(icon)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(Tray { icon })
}

impl Tray {
    /// what hovering the icon says, the frame's state in a few words
    pub fn set_status(&self, statuses: &[Status]) {
        let text = match statuses.first() {
            None => "framecorder: not paired yet".to_string(),
            Some(s) => match s.state {
                State::Connected => format!("framecorder: connected to {}", s.name),
                State::Connecting => format!("framecorder: looking for {}", s.name),
                State::Unreachable => format!("framecorder: can't reach {}, is it on?", s.name),
                State::Full => "framecorder: this computer is out of space".to_string(),
                State::Unpaired | State::WrongFingerprint => format!("framecorder: {} needs pairing again", s.name),
            },
        };
        let _ = self.icon.set_tooltip(Some(text));
    }

    pub fn set_visible(&self, visible: bool) {
        let _ = self.icon.set_visible(visible);
    }
}

//! A SteamVR dashboard tab: the overlay handles and the events it gets.
//! Pixels go in with SetOverlayRaw, which is plenty for a UI that only
//! changes when you poke it. SteamVR's own keyboard types into it too.

use std::ffi::{c_char, c_void, CString};

use anyhow::{bail, Result};

use crate::openvr::OpenVr;

const VERSION: &str = "IVROverlay_028";

// Slots in VR_IVROverlay_FnTable (openvr_capi.h, IVROverlay_028).
const SLOT_DESTROY: usize = 3;
const SLOT_SET_WIDTH: usize = 22;
const SLOT_IS_VISIBLE: usize = 45;
const SLOT_POLL_EVENT: usize = 48;
const SLOT_SET_INPUT_METHOD: usize = 50;
const SLOT_SET_MOUSE_SCALE: usize = 52;
const SLOT_SET_TEXTURE: usize = 60;
const SLOT_SET_RAW: usize = 62;
const SLOT_SET_FROM_FILE: usize = 63;
const SLOT_CREATE_DASHBOARD: usize = 67;
const SLOT_IS_DASHBOARD_VISIBLE: usize = 68;
const SLOT_SHOW_DASHBOARD: usize = 72;
const SLOT_SHOW_KEYBOARD_FOR_OVERLAY: usize = 75;
const SLOT_GET_KEYBOARD_TEXT: usize = 76;
const SLOT_HIDE_KEYBOARD: usize = 77;

const INPUT_MOUSE: i32 = 1;
/// EGamepadTextInputMode and EGamepadTextInputLineMode: plain text, more
/// than one line of it.
const TEXT_MODE_NORMAL: i32 = 0;
const LINE_MODE_MULTIPLE: i32 = 1;
/// EKeyboardFlags: SteamVR's full keyboard, with its own text field.
const KEYBOARD_FLAGS: u32 = 0;
/// Room for the longest text the keyboard is ever asked for, as UTF-8.
const KEYBOARD_TEXT_BYTES: usize = 4 * 4096;

pub const EVENT_MOUSE_MOVE: u32 = 300;
pub const EVENT_MOUSE_DOWN: u32 = 301;
pub const EVENT_MOUSE_UP: u32 = 302;
pub const EVENT_OVERLAY_SHOWN: u32 = 500;
pub const EVENT_OVERLAY_HIDDEN: u32 = 501;
pub const EVENT_DASHBOARD_ACTIVATED: u32 = 502;
pub const EVENT_DASHBOARD_DEACTIVATED: u32 = 503;
pub const EVENT_QUIT: u32 = 700;
pub const EVENT_KEYBOARD_CLOSED: u32 = 1200;
pub const EVENT_KEYBOARD_CHAR_INPUT: u32 = 1201;
pub const EVENT_KEYBOARD_DONE: u32 = 1202;

/// VREvent_t. Linux builds of OpenVR pack it to 4 bytes, 60 in total.
#[repr(C, packed(4))]
#[derive(Clone, Copy)]
struct RawEvent {
    kind: u32,
    device: u32,
    age: f32,
    data: [u8; 48],
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
    MouseMove { x: f32, y: f32 },
    MouseDown { x: f32, y: f32 },
    MouseUp { x: f32, y: f32 },
    Shown,
    Hidden,
    DashboardOpened,
    DashboardClosed,
    /// Something was typed on SteamVR's keyboard; `keyboard_text` has it all.
    KeyboardChar,
    /// Done was pressed on the keyboard.
    KeyboardDone,
    /// The keyboard went away, finished with or not.
    KeyboardClosed,
    Quit,
    Other(u32),
}

type Handle = u64;

pub struct DashboardTab<'a> {
    vr: &'a OpenVr,
    table: *const *const c_void,
    main: Handle,
    thumbnail: Handle,
    width: u32,
    height: u32,
}

impl<'a> DashboardTab<'a> {
    pub fn new(vr: &'a OpenVr, key: &str, name: &str, width: u32, height: u32, meters: f32) -> Result<Self> {
        let Some(table) = vr.interface(VERSION) else {
            bail!("SteamVR doesn't offer {VERSION}");
        };
        let key_c = CString::new(key)?;
        let name_c = CString::new(name)?;
        let (mut main, mut thumbnail) = (0u64, 0u64);
        let err = unsafe {
            let f: unsafe extern "C" fn(*const c_char, *const c_char, *mut Handle, *mut Handle) -> i32 =
                slot(table, SLOT_CREATE_DASHBOARD);
            f(key_c.as_ptr(), name_c.as_ptr(), &mut main, &mut thumbnail)
        };
        if err != 0 {
            bail!("SteamVR wouldn't create the dashboard tab (overlay error {err})");
        }

        let tab = Self { vr, table, main, thumbnail, width, height };
        unsafe {
            let set_width: unsafe extern "C" fn(Handle, f32) -> i32 = slot(table, SLOT_SET_WIDTH);
            set_width(main, meters);
            let set_input: unsafe extern "C" fn(Handle, i32) -> i32 = slot(table, SLOT_SET_INPUT_METHOD);
            set_input(main, INPUT_MOUSE);
            let scale = [width as f32, height as f32];
            let set_scale: unsafe extern "C" fn(Handle, *const [f32; 2]) -> i32 = slot(table, SLOT_SET_MOUSE_SCALE);
            set_scale(main, &scale);
        }
        Ok(tab)
    }

    /// Uploads a full frame of RGBA pixels.
    pub fn set_pixels(&self, rgba: &[u8]) -> Result<()> {
        self.set_raw(self.main, rgba, self.width, self.height)
    }

    /// Hands SteamVR a GPU texture to show on the tab.
    ///
    /// # Safety
    /// `texture` must point at a valid `Texture_t` whose handle points at
    /// valid texture data for the call's duration.
    pub unsafe fn set_texture(&self, texture: *const c_void) -> Result<()> {
        let f: unsafe extern "C" fn(Handle, *const c_void) -> i32 = slot(self.table, SLOT_SET_TEXTURE);
        let err = f(self.main, texture);
        if err != 0 {
            bail!("handing the tab texture to SteamVR failed (overlay error {err})");
        }
        Ok(())
    }

    /// Icon shown on the dashboard's tab bar.
    pub fn set_thumbnail(&self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
        self.set_raw(self.thumbnail, rgba, width, height)
    }

    pub fn set_thumbnail_file(&self, path: &str) -> Result<()> {
        let path = CString::new(path)?;
        let err = unsafe {
            let f: unsafe extern "C" fn(Handle, *const c_char) -> i32 = slot(self.table, SLOT_SET_FROM_FILE);
            f(self.thumbnail, path.as_ptr())
        };
        if err != 0 {
            bail!("setting the tab icon failed (overlay error {err})");
        }
        Ok(())
    }

    fn set_raw(&self, handle: Handle, rgba: &[u8], width: u32, height: u32) -> Result<()> {
        if rgba.len() != (width * height * 4) as usize {
            bail!("pixel buffer is the wrong size");
        }
        let err = unsafe {
            let f: unsafe extern "C" fn(Handle, *const c_void, u32, u32, u32) -> i32 = slot(self.table, SLOT_SET_RAW);
            f(handle, rgba.as_ptr().cast(), width, height, 4)
        };
        if err != 0 {
            bail!("uploading the tab contents failed (overlay error {err})");
        }
        Ok(())
    }

    pub fn is_visible(&self) -> bool {
        unsafe {
            let f: unsafe extern "C" fn(Handle) -> bool = slot(self.table, SLOT_IS_VISIBLE);
            f(self.main)
        }
    }

    pub fn dashboard_visible(&self) -> bool {
        unsafe {
            let f: unsafe extern "C" fn() -> bool = slot(self.table, SLOT_IS_DASHBOARD_VISIBLE);
            f()
        }
    }

    /// Next pending event for the tab. Mouse coordinates are in pixels,
    /// top left origin.
    pub fn poll(&self) -> Option<Event> {
        let mut raw = RawEvent { kind: 0, device: 0, age: 0.0, data: [0; 48] };
        let got = unsafe {
            let f: unsafe extern "C" fn(Handle, *mut RawEvent, u32) -> bool = slot(self.table, SLOT_POLL_EVENT);
            f(self.main, &mut raw, std::mem::size_of::<RawEvent>() as u32)
        };
        if !got {
            return None;
        }
        let data = raw.data;
        let x = f32::from_ne_bytes([data[0], data[1], data[2], data[3]]);
        // OpenVR puts the mouse origin at the bottom left.
        let y = self.height as f32 - f32::from_ne_bytes([data[4], data[5], data[6], data[7]]);
        let kind = raw.kind;
        Some(match kind {
            EVENT_MOUSE_MOVE => Event::MouseMove { x, y },
            EVENT_MOUSE_DOWN => Event::MouseDown { x, y },
            EVENT_MOUSE_UP => Event::MouseUp { x, y },
            EVENT_OVERLAY_SHOWN => Event::Shown,
            EVENT_OVERLAY_HIDDEN => Event::Hidden,
            EVENT_DASHBOARD_ACTIVATED => Event::DashboardOpened,
            EVENT_DASHBOARD_DEACTIVATED => Event::DashboardClosed,
            EVENT_QUIT => Event::Quit,
            EVENT_KEYBOARD_CHAR_INPUT => Event::KeyboardChar,
            EVENT_KEYBOARD_DONE => Event::KeyboardDone,
            EVENT_KEYBOARD_CLOSED => Event::KeyboardClosed,
            other => Event::Other(other),
        })
    }

    /// Opens the dashboard on this tab.
    pub fn show(&self, key: &str) {
        if let Ok(key) = CString::new(key) {
            unsafe {
                let f: unsafe extern "C" fn(*const c_char) = slot(self.table, SLOT_SHOW_DASHBOARD);
                f(key.as_ptr());
            }
        }
    }

    /// Brings up SteamVR's keyboard, typing into this tab: its events come
    /// through `poll`, and `keyboard_text` has what's been typed. `existing`
    /// is what it starts with, `max` the most characters it takes.
    pub fn show_keyboard(&self, description: &str, max: u32, existing: &str) -> Result<()> {
        let description = CString::new(description)?;
        let existing = CString::new(existing)?;
        let err = unsafe {
            let f: unsafe extern "C" fn(Handle, i32, i32, u32, *const c_char, u32, *const c_char, u64) -> i32 =
                slot(self.table, SLOT_SHOW_KEYBOARD_FOR_OVERLAY);
            f(self.main, TEXT_MODE_NORMAL, LINE_MODE_MULTIPLE, KEYBOARD_FLAGS, description.as_ptr(), max, existing.as_ptr(), 0)
        };
        if err != 0 {
            bail!("SteamVR wouldn't show its keyboard (overlay error {err})");
        }
        Ok(())
    }

    /// What's in the keyboard's text field right now.
    pub fn keyboard_text(&self) -> String {
        let mut buf = vec![0u8; KEYBOARD_TEXT_BYTES];
        unsafe {
            let f: unsafe extern "C" fn(*mut c_char, u32) -> u32 = slot(self.table, SLOT_GET_KEYBOARD_TEXT);
            f(buf.as_mut_ptr().cast(), buf.len() as u32);
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned()
    }

    pub fn hide_keyboard(&self) {
        unsafe {
            let f: unsafe extern "C" fn() = slot(self.table, SLOT_HIDE_KEYBOARD);
            f();
        }
    }

    pub fn openvr(&self) -> &OpenVr {
        self.vr
    }
}

impl Drop for DashboardTab<'_> {
    fn drop(&mut self) {
        unsafe {
            let f: unsafe extern "C" fn(Handle) -> i32 = slot(self.table, SLOT_DESTROY);
            f(self.main);
        }
    }
}

unsafe fn slot<T: Copy>(table: *const *const c_void, index: usize) -> T {
    std::mem::transmute_copy(&*table.add(index))
}

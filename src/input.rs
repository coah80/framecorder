//! The clip keybind, through SteamVR Input. Overlay apps get their bindings
//! alongside the game's, so it works in-game, and it can be rebound in
//! SteamVR's controller settings under "framecorder".

use std::ffi::{c_char, c_void, CString};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::openvr::OpenVr;

const VERSION: &str = "IVRInput_011";

// Slots in VR_IVRInput_FnTable (openvr_capi.h, IVRInput_011).
const SLOT_SET_MANIFEST: usize = 0;
const SLOT_ACTION_SET: usize = 1;
const SLOT_ACTION: usize = 2;
const SLOT_UPDATE: usize = 4;
const SLOT_DIGITAL: usize = 5;
const SLOT_HAPTIC: usize = 23;

const SET: &str = "/actions/framecorder";
const CLIP: &str = "/actions/framecorder/in/clip";
const BUZZ: &str = "/actions/framecorder/out/haptic";

/// The manifest and default bindings, written out next to each other at
/// startup since SteamVR reads them from disk.
const FILES: [(&str, &str); 2] = [
    ("actions.json", include_str!("../assets/input/actions.json")),
    ("bindings_frame_controller.json", include_str!("../assets/input/bindings_frame_controller.json")),
];

#[repr(C)]
struct ActiveSet {
    set: u64,
    restricted_to_device: u64,
    secondary_set: u64,
    padding: u32,
    priority: i32,
}

#[repr(C)]
#[derive(Default)]
struct DigitalData {
    active: bool,
    origin: u64,
    state: bool,
    changed: bool,
    update_time: f32,
}

pub struct Input {
    table: *const *const c_void,
    set: u64,
    clip: u64,
    buzz: u64,
}

unsafe fn slot<T: Copy>(table: *const *const c_void, index: usize) -> T {
    std::mem::transmute_copy(&*table.add(index))
}

fn manifest_dir() -> Result<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .context("HOME isn't set")?;
    Ok(data.join("framecorder/input"))
}

fn write_files(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, text) in FILES {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text) {
            std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        }
    }
    Ok(())
}

impl Input {
    pub fn new(vr: &OpenVr) -> Result<Self> {
        let Some(table) = vr.interface(VERSION) else {
            bail!("SteamVR doesn't offer {VERSION}");
        };
        let dir = manifest_dir()?;
        write_files(&dir)?;
        let manifest = CString::new(dir.join("actions.json").to_string_lossy().as_bytes())?;
        let handle = |slot_index: usize, name: &str| -> Result<u64> {
            let name = CString::new(name)?;
            let mut h = 0u64;
            let err = unsafe {
                let f: unsafe extern "C" fn(*const c_char, *mut u64) -> i32 = slot(table, slot_index);
                f(name.as_ptr(), &mut h)
            };
            if err != 0 {
                bail!("SteamVR input error {err} for {}", name.to_string_lossy());
            }
            Ok(h)
        };
        let err = unsafe {
            let f: unsafe extern "C" fn(*const c_char) -> i32 = slot(table, SLOT_SET_MANIFEST);
            f(manifest.as_ptr())
        };
        if err != 0 {
            bail!("SteamVR didn't take the input manifest (input error {err})");
        }
        Ok(Self { table, set: handle(SLOT_ACTION_SET, SET)?, clip: handle(SLOT_ACTION, CLIP)?, buzz: handle(SLOT_ACTION, BUZZ)? })
    }

    /// True once each time the clip binding fires.
    pub fn clip_pressed(&self) -> bool {
        let mut active = ActiveSet { set: self.set, restricted_to_device: 0, secondary_set: 0, padding: 0, priority: 0 };
        let mut data = DigitalData::default();
        unsafe {
            let update: unsafe extern "C" fn(*mut ActiveSet, u32, u32) -> i32 = slot(self.table, SLOT_UPDATE);
            if update(&mut active, std::mem::size_of::<ActiveSet>() as u32, 1) != 0 {
                return false;
            }
            let get: unsafe extern "C" fn(u64, *mut DigitalData, u32, u64) -> i32 = slot(self.table, SLOT_DIGITAL);
            if get(self.clip, &mut data, std::mem::size_of::<DigitalData>() as u32, 0) != 0 {
                return false;
            }
        }
        data.active && data.state && data.changed
    }

    /// A short buzz on whichever hand the binding is on.
    pub fn buzz(&self) {
        unsafe {
            let f: unsafe extern "C" fn(u64, f32, f32, f32, f32, u64) -> i32 = slot(self.table, SLOT_HAPTIC);
            f(self.buzz, 0.0, 0.08, 160.0, 0.6, 0);
        }
    }
}

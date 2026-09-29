//! A small note that floats in front of you for a moment ("clipped the last
//! 30 s"), so you know a clip worked without opening the dashboard.

use std::ffi::{c_char, c_void, CString};

use anyhow::{bail, Result};

use crate::openvr::OpenVr;

const VERSION: &str = "IVROverlay_028";

// Slots in VR_IVROverlay_FnTable (openvr_capi.h, IVROverlay_028).
const SLOT_CREATE: usize = 1;
const SLOT_DESTROY: usize = 3;
const SLOT_SET_WIDTH: usize = 22;
const SLOT_SET_TRANSFORM_DEVICE: usize = 35;
const SLOT_SHOW: usize = 43;
const SLOT_HIDE: usize = 44;
const SLOT_SET_RAW: usize = 62;

const HMD: u32 = 0;
/// Where it floats, relative to your head: a little below eye level, 1 m out.
const OFFSET: [f32; 3] = [0.0, -0.18, -1.0];

pub struct Toast {
    table: *const *const c_void,
    handle: u64,
}

unsafe fn slot<T: Copy>(table: *const *const c_void, index: usize) -> T {
    std::mem::transmute_copy(&*table.add(index))
}

impl Toast {
    pub fn new(vr: &OpenVr, key: &str, name: &str, meters: f32) -> Result<Self> {
        let Some(table) = vr.interface(VERSION) else {
            bail!("SteamVR doesn't offer {VERSION}");
        };
        let (key, name) = (CString::new(key)?, CString::new(name)?);
        let mut handle = 0u64;
        let err = unsafe {
            let f: unsafe extern "C" fn(*const c_char, *const c_char, *mut u64) -> i32 = slot(table, SLOT_CREATE);
            f(key.as_ptr(), name.as_ptr(), &mut handle)
        };
        if err != 0 {
            bail!("SteamVR wouldn't create the toast overlay (overlay error {err})");
        }
        let toast = Self { table, handle };
        let [x, y, z] = OFFSET;
        let transform: [[f32; 4]; 3] = [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z]];
        unsafe {
            let set_width: unsafe extern "C" fn(u64, f32) -> i32 = slot(table, SLOT_SET_WIDTH);
            set_width(handle, meters);
            let set_transform: unsafe extern "C" fn(u64, u32, *const [[f32; 4]; 3]) -> i32 = slot(table, SLOT_SET_TRANSFORM_DEVICE);
            set_transform(handle, HMD, &transform);
        }
        Ok(toast)
    }

    pub fn show(&self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
        if rgba.len() != (width * height * 4) as usize {
            bail!("pixel buffer is the wrong size");
        }
        let err = unsafe {
            let raw: unsafe extern "C" fn(u64, *const c_void, u32, u32, u32) -> i32 = slot(self.table, SLOT_SET_RAW);
            raw(self.handle, rgba.as_ptr().cast(), width, height, 4)
        };
        if err != 0 {
            bail!("uploading the toast failed (overlay error {err})");
        }
        unsafe {
            let show: unsafe extern "C" fn(u64) -> i32 = slot(self.table, SLOT_SHOW);
            show(self.handle);
        }
        Ok(())
    }

    pub fn hide(&self) {
        unsafe {
            let hide: unsafe extern "C" fn(u64) -> i32 = slot(self.table, SLOT_HIDE);
            hide(self.handle);
        }
    }
}

impl Drop for Toast {
    fn drop(&mut self) {
        unsafe {
            let f: unsafe extern "C" fn(u64) -> i32 = slot(self.table, SLOT_DESTROY);
            f(self.handle);
        }
    }
}

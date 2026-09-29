//! Asks SteamVR whether a scene app (a game, or SteamVR Home) is running.
//! We connect as a background app, which never starts SteamVR, and let go
//! again right away.

use std::ffi::{c_char, c_void, CString};

const DEFAULT_LIB: &str = "/opt/steamvr/bin/linuxarm64/libopenvr_api.so";
const APP_BACKGROUND: i32 = 3;
/// GetSceneApplicationState is slot 25 in both of these.
const APPLICATIONS_VERSIONS: &[&str] = &["IVRApplications_008", "IVRApplications_007"];
const SLOT_SCENE_STATE: usize = 25;
const SCENE_NONE: i32 = 0;

type InitFn = unsafe extern "C" fn(*mut i32, i32, *const c_char) -> isize;
type ShutdownFn = unsafe extern "C" fn();
type GetInterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;
type SceneStateFn = unsafe extern "C" fn() -> i32;

pub fn library_path() -> String {
    std::env::var("FRAMECORDER_OPENVR_LIB").unwrap_or_else(|_| DEFAULT_LIB.into())
}

/// True if something is rendering a scene right now. Anything going wrong
/// (no SteamVR, no library) means no game.
pub fn scene_running(lib_path: &str) -> bool {
    let Ok(cpath) = CString::new(lib_path) else { return false };
    unsafe {
        let lib = libc::dlopen(cpath.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if lib.is_null() {
            return false;
        }
        let running = probe(lib).unwrap_or(false);
        libc::dlclose(lib);
        running
    }
}

unsafe fn probe(lib: *mut c_void) -> Option<bool> {
    let init: InitFn = sym(lib, c"VR_InitInternal2")?;
    let shutdown: ShutdownFn = sym(lib, c"VR_ShutdownInternal")?;
    let get_interface: GetInterfaceFn = sym(lib, c"VR_GetGenericInterface")?;

    let mut err = 0i32;
    init(&mut err, APP_BACKGROUND, std::ptr::null());
    if err != 0 {
        // almost always "no server", i.e. SteamVR isn't running
        return None;
    }
    let state = APPLICATIONS_VERSIONS.iter().find_map(|v| {
        let name = CString::new(format!("FnTable:{v}")).ok()?;
        let mut err = 0i32;
        let table = get_interface(name.as_ptr(), &mut err) as *const *const c_void;
        if err != 0 || table.is_null() {
            return None;
        }
        let slot = *table.add(SLOT_SCENE_STATE);
        if slot.is_null() {
            return None;
        }
        let f: SceneStateFn = std::mem::transmute(slot);
        Some(f())
    });
    shutdown();
    Some(state.is_some_and(|s| s != SCENE_NONE))
}

unsafe fn sym<T: Copy>(lib: *mut c_void, name: &std::ffi::CStr) -> Option<T> {
    let ptr = libc::dlsym(lib, name.as_ptr());
    (!ptr.is_null()).then(|| std::mem::transmute_copy(&ptr))
}

#[cfg(test)]
mod tests {
    #[test]
    fn missing_library_means_no_game() {
        assert!(!super::scene_running("/nonexistent/libopenvr_api.so"));
    }
}

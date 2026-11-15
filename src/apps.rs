//! framecorder as an app SteamVR knows by name, not an anonymous process:
//! that's what gets it a close button on the Frame's app bar.

use std::ffi::{c_char, c_void, CString};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::openvr::OpenVr;

const VERSION: &str = "IVRApplications_008";
pub const KEY: &str = "coah80.framecorder";

// Slots in VR_IVRApplications_FnTable (openvr_capi.h, IVRApplications_008).
const SLOT_ADD_MANIFEST: usize = 0;
const SLOT_KEY_BY_PROCESS: usize = 5;
const SLOT_IDENTIFY: usize = 11;
const SLOT_SET_AUTO_LAUNCH: usize = 17;
const SLOT_SCENE_PROCESS: usize = 30;

unsafe fn slot<T: Copy>(table: *const *const c_void, index: usize) -> T {
    std::mem::transmute_copy(&*table.add(index))
}

fn manifest(binary: &str) -> String {
    format!(
        r#"{{
  "source": "user",
  "applications": [
    {{
      "app_key": "{KEY}",
      "launch_type": "binary",
      "binary_path_linux": "{binary}",
      "binary_path_linux_arm": "{binary}",
      "is_dashboard_overlay": true,
      "strings": {{
        "en_us": {{
          "name": "framecorder",
          "description": "records what the panels show"
        }}
      }}
    }}
  ]
}}
"#
    )
}

/// Tells SteamVR about framecorder and that this process is it.
pub fn register(vr: &OpenVr) -> Result<()> {
    let Some(table) = vr.interface(VERSION) else { bail!("SteamVR doesn't offer {VERSION}") };
    let home = std::env::var_os("HOME").map(PathBuf::from).context("HOME isn't set")?;
    let binary = std::env::current_exe().context("finding framecorder-ui")?;
    let path = home.join(".local/share/framecorder/framecorder.vrmanifest");
    let text = manifest(&binary.to_string_lossy());
    if std::fs::read_to_string(&path).ok().as_deref() != Some(&text) {
        std::fs::create_dir_all(path.parent().context("no folder")?)?;
        std::fs::write(&path, &text).with_context(|| format!("writing {}", path.display()))?;
    }
    let path = CString::new(path.to_string_lossy().as_bytes())?;
    let key = CString::new(KEY)?;
    let call = |what: &str, err: i32| if err != 0 { bail!("SteamVR application error {err} from {what}") } else { Ok(()) };
    unsafe {
        let add: unsafe extern "C" fn(*const c_char, bool) -> i32 = slot(table, SLOT_ADD_MANIFEST);
        call("adding the manifest", add(path.as_ptr(), false))?;
        // systemd starts it with SteamVR, so SteamVR shouldn't too
        let auto: unsafe extern "C" fn(*const c_char, bool) -> i32 = slot(table, SLOT_SET_AUTO_LAUNCH);
        call("turning off auto launch", auto(key.as_ptr(), false))?;
        let identify: unsafe extern "C" fn(u32, *const c_char) -> i32 = slot(table, SLOT_IDENTIFY);
        call("identifying", identify(std::process::id(), key.as_ptr()))?;
    }
    Ok(())
}

/// Whether a game is running: SteamVR has a scene app, and it isn't one of
/// SteamVR's own (its home environment and tools are `openvr.*`).
pub struct Scene(*const *const c_void);

impl Scene {
    pub fn new(vr: &OpenVr) -> Option<Self> {
        vr.interface(VERSION).map(Self)
    }

    pub fn game(&self) -> bool {
        unsafe {
            let process: unsafe extern "C" fn() -> u32 = slot(self.0, SLOT_SCENE_PROCESS);
            let pid = process();
            if pid == 0 {
                return false;
            }
            let key_of: unsafe extern "C" fn(u32, *mut c_char, u32) -> i32 = slot(self.0, SLOT_KEY_BY_PROCESS);
            let mut key = [0 as c_char; 128];
            if key_of(pid, key.as_mut_ptr(), key.len() as u32) != 0 {
                return true;
            }
            !std::ffi::CStr::from_ptr(key.as_ptr()).to_bytes().starts_with(b"openvr.")
        }
    }
}

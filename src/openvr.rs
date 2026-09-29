//! Just enough OpenVR to ask SteamVR what its lens distortion looks like.
//! We connect as a background app, grab the numbers and disconnect again, so
//! nothing stays attached to the compositor while recording.

use std::ffi::{c_char, c_void, CStr, CString};

use anyhow::{bail, Result};

const DEFAULT_LIB: &str = "/opt/steamvr/bin/linuxarm64/libopenvr_api.so";

/// How we present ourselves to SteamVR.
#[derive(Clone, Copy)]
pub enum AppType {
    /// Doesn't show anything and doesn't start SteamVR.
    Background = 3,
    /// Draws overlays (the dashboard tab). Careful: connecting like this
    /// starts SteamVR's server if it isn't running, and a server started
    /// from outside SteamVR's own launcher leaves the headset without a
    /// working compositor. Check `server_running` first.
    Overlay = 2,
}

/// Interface versions we know the function table layout of. The first four
/// slots haven't moved in any of them; the rest only get used on the newest.
const SYSTEM_VERSIONS: &[&str] = &["IVRSystem_026", "IVRSystem_023", "IVRSystem_022", "IVRSystem_021", "IVRSystem_020", "IVRSystem_019"];
const FULL_LAYOUT: &str = "IVRSystem_026";

// Slots in VR_IVRSystem_FnTable (openvr_capi.h).
const SLOT_RENDER_TARGET_SIZE: usize = 0;
const SLOT_PROJECTION_RAW: usize = 2;
const SLOT_COMPUTE_DISTORTION: usize = 3;
const SLOT_EYE_TO_HEAD: usize = 5;
const SLOT_HIDDEN_AREA_MESH: usize = 34;

const HIDDEN_AREA_STANDARD: i32 = 0;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct DistortionCoordinates {
    red: [f32; 2],
    green: [f32; 2],
    blue: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HmdMatrix34 {
    m: [[f32; 4]; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HiddenAreaMesh {
    vertices: *const [f32; 2],
    triangles: u32,
}

type InitFn = unsafe extern "C" fn(*mut i32, i32, *const c_char) -> isize;
type ShutdownFn = unsafe extern "C" fn();
type GetInterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;
type ErrorNameFn = unsafe extern "C" fn(i32) -> *const c_char;

/// Tangents of the half angles an eye's render target covers. Top is
/// negative, since OpenVR's y points down in texture space.
#[derive(Clone, Copy, Debug)]
pub struct Projection {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

pub struct OpenVr {
    lib: *mut c_void,
    table: *const *const c_void,
    full_layout: bool,
    shutdown: ShutdownFn,
    get_interface: GetInterfaceFn,
}

impl OpenVr {
    pub fn connect() -> Result<Self> {
        Self::connect_as(AppType::Background)
    }

    /// Whether SteamVR's server is up. Connects as a background app, which
    /// SteamVR never starts a server for, so this can't launch one by accident.
    pub fn server_running() -> bool {
        Self::connect_as(AppType::Background).is_ok()
    }

    pub fn connect_as(app: AppType) -> Result<Self> {
        let path = std::env::var("FRAMECORDER_OPENVR_LIB").unwrap_or_else(|_| DEFAULT_LIB.into());
        let cpath = CString::new(path.clone())?;
        let lib = unsafe { libc::dlopen(cpath.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if lib.is_null() {
            bail!("couldn't load {path}: {}", dl_error());
        }

        let init: InitFn = unsafe { sym(lib, c"VR_InitInternal2")? };
        let shutdown: ShutdownFn = unsafe { sym(lib, c"VR_ShutdownInternal")? };
        let get_interface: GetInterfaceFn = unsafe { sym(lib, c"VR_GetGenericInterface")? };
        let error_name: Option<ErrorNameFn> = unsafe { sym(lib, c"VR_GetVRInitErrorAsEnglishDescription").ok() };

        let mut err = 0i32;
        unsafe { init(&mut err, app as i32, std::ptr::null()) };
        if err != 0 {
            let msg = error_name
                .map(|f| unsafe { CStr::from_ptr(f(err)) }.to_string_lossy().into_owned())
                .unwrap_or_default();
            unsafe { libc::dlclose(lib) };
            bail!("SteamVR isn't available ({err}: {msg})");
        }

        let found = SYSTEM_VERSIONS.iter().find_map(|v| {
            let name = CString::new(format!("FnTable:{v}")).ok()?;
            let mut err = 0i32;
            let table = unsafe { get_interface(name.as_ptr(), &mut err) };
            (err == 0 && !table.is_null()).then_some((*v, table as *const *const c_void))
        });
        let Some((version, table)) = found else {
            unsafe {
                shutdown();
                libc::dlclose(lib);
            }
            bail!("this SteamVR doesn't offer a known IVRSystem version");
        };
        log::debug!("using {version}");
        if version != FULL_LAYOUT {
            log::warn!("SteamVR only offers {version}, so the hidden area and eye roll can't be read");
        }

        Ok(Self { lib, table, full_layout: version == FULL_LAYOUT, shutdown, get_interface })
    }

    /// Function table of another OpenVR interface, e.g. `IVROverlay_028`.
    pub fn interface(&self, version: &str) -> Option<*const *const c_void> {
        let name = CString::new(format!("FnTable:{version}")).ok()?;
        let mut err = 0i32;
        let table = unsafe { (self.get_interface)(name.as_ptr(), &mut err) };
        (err == 0 && !table.is_null()).then_some(table as *const *const c_void)
    }

    unsafe fn slot<T: Copy>(&self, index: usize) -> T {
        std::mem::transmute_copy(&*self.table.add(index))
    }

    pub fn render_target_size(&self) -> (u32, u32) {
        let (mut w, mut h) = (0, 0);
        unsafe {
            let f: unsafe extern "C" fn(*mut u32, *mut u32) = self.slot(SLOT_RENDER_TARGET_SIZE);
            f(&mut w, &mut h);
        }
        (w, h)
    }

    pub fn projection(&self, eye: usize) -> Projection {
        let (mut l, mut r, mut t, mut b) = (0.0, 0.0, 0.0, 0.0);
        unsafe {
            let f: unsafe extern "C" fn(i32, *mut f32, *mut f32, *mut f32, *mut f32) = self.slot(SLOT_PROJECTION_RAW);
            f(eye as i32, &mut l, &mut r, &mut t, &mut b);
        }
        Projection { left: l, right: r, top: t, bottom: b }
    }

    /// For a point on the panel (eye local, 0..1), where in the undistorted
    /// render target each color channel comes from.
    pub fn distortion(&self, eye: usize, u: f32, v: f32) -> Option<[[f32; 2]; 3]> {
        let mut out = DistortionCoordinates::default();
        let ok = unsafe {
            let f: unsafe extern "C" fn(i32, f32, f32, *mut DistortionCoordinates) -> bool =
                self.slot(SLOT_COMPUTE_DISTORTION);
            f(eye as i32, u, v, &mut out)
        };
        ok.then_some([out.red, out.green, out.blue])
    }

    /// How far the eye's view is rolled relative to the head, in radians.
    /// Canted displays show up here.
    pub fn eye_roll(&self, eye: usize) -> f64 {
        if !self.full_layout {
            return 0.0;
        }
        let t = unsafe {
            let f: unsafe extern "C" fn(i32) -> HmdMatrix34 = self.slot(SLOT_EYE_TO_HEAD);
            f(eye as i32)
        };
        log::debug!("eye {eye} to head: {:?}", t.m);
        (t.m[1][0] as f64).atan2(t.m[0][0] as f64)
    }

    /// Triangles, in render target UV, covering what the compositor never
    /// draws for this eye.
    pub fn hidden_area(&self, eye: usize) -> Vec<[[f32; 2]; 3]> {
        if !self.full_layout {
            return Vec::new();
        }
        unsafe {
            let f: unsafe extern "C" fn(i32, i32) -> HiddenAreaMesh = self.slot(SLOT_HIDDEN_AREA_MESH);
            let mesh = f(eye as i32, HIDDEN_AREA_STANDARD);
            if mesh.vertices.is_null() || mesh.triangles == 0 || mesh.triangles > 1 << 20 {
                return Vec::new();
            }
            std::slice::from_raw_parts(mesh.vertices, mesh.triangles as usize * 3)
                .as_chunks::<3>()
                .0
                .to_vec()
        }
    }
}

impl Drop for OpenVr {
    fn drop(&mut self) {
        unsafe {
            (self.shutdown)();
            libc::dlclose(self.lib);
        }
    }
}

unsafe fn sym<T: Copy>(lib: *mut c_void, name: &CStr) -> Result<T> {
    let ptr = libc::dlsym(lib, name.as_ptr());
    if ptr.is_null() {
        bail!("{} is missing from libopenvr_api", name.to_string_lossy());
    }
    Ok(std::mem::transmute_copy(&ptr))
}

fn dl_error() -> String {
    let err = unsafe { libc::dlerror() };
    if err.is_null() {
        return "unknown error".into();
    }
    unsafe { CStr::from_ptr(err) }.to_string_lossy().into_owned()
}

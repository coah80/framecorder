//! Bits of Android the Rust side can't reach on its own: a foreground
//! service so syncing survives the app going to the background, the
//! multicast lock mDNS needs, MediaStore, and the share sheet.

use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

pub struct FrameSync<R: Runtime> {
    #[cfg(target_os = "android")]
    handle: tauri::plugin::PluginHandle<R>,
    #[cfg(not(target_os = "android"))]
    _r: std::marker::PhantomData<fn() -> R>,
}

#[derive(Deserialize)]
struct Saved {
    uri: String,
}

#[derive(Deserialize)]
struct Name {
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveArgs<'a> {
    path: &'a str,
    name: &'a str,
    subdir: &'a str,
}

#[derive(Serialize)]
struct UriArgs<'a> {
    uri: &'a str,
}

#[derive(Serialize)]
struct BusyArgs {
    busy: bool,
}

/// Commands that just resolve() send back null.
type Nothing = serde::de::IgnoredAny;

impl<R: Runtime> FrameSync<R> {
    #[cfg(target_os = "android")]
    fn call<T: serde::de::DeserializeOwned>(&self, cmd: &str, args: impl Serialize) -> Result<T, String> {
        self.handle.run_mobile_plugin(cmd, args).map_err(|e| e.to_string())
    }

    #[cfg(not(target_os = "android"))]
    fn call<T: serde::de::DeserializeOwned>(&self, _cmd: &str, _args: impl Serialize) -> Result<T, String> {
        Err("only on android".into())
    }

    /// Starts the "framecorder is syncing" foreground service and takes the
    /// multicast lock.
    pub fn start_service(&self) -> Result<(), String> {
        self.call::<Nothing>("startService", ()).map(|_| ())
    }

    pub fn stop_service(&self) -> Result<(), String> {
        self.call::<Nothing>("stopService", ()).map(|_| ())
    }

    /// Holds a partial wake lock while a download runs.
    pub fn set_busy(&self, busy: bool) -> Result<(), String> {
        self.call::<Nothing>("setBusy", BusyArgs { busy }).map(|_| ())
    }

    /// Copies a finished download into Movies/framecorder[/subdir] and
    /// returns its content:// uri. Blocks for the copy.
    pub fn save_to_gallery(&self, path: &str, name: &str, subdir: &str) -> Result<String, String> {
        self.call::<Saved>("saveToGallery", SaveArgs { path, name, subdir }).map(|s| s.uri)
    }

    pub fn open(&self, uri: &str) -> Result<(), String> {
        self.call::<Nothing>("open", UriArgs { uri }).map(|_| ())
    }

    pub fn share(&self, uri: &str) -> Result<(), String> {
        self.call::<Nothing>("share", UriArgs { uri }).map(|_| ())
    }

    pub fn device_name(&self) -> Result<String, String> {
        self.call::<Name>("deviceName", ()).map(|n| n.name)
    }
}

pub trait FrameSyncExt<R: Runtime> {
    fn framesync(&self) -> &FrameSync<R>;
}

impl<R: Runtime, T: Manager<R>> FrameSyncExt<R> for T {
    fn framesync(&self) -> &FrameSync<R> {
        self.state::<FrameSync<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("framesync")
        .setup(|app, _api| {
            #[cfg(target_os = "android")]
            let fs = FrameSync { handle: _api.register_android_plugin("com.framecorder.framesync", "FrameSyncPlugin")? };
            #[cfg(not(target_os = "android"))]
            let fs = FrameSync::<R> { _r: std::marker::PhantomData };
            app.manage(fs);
            Ok(())
        })
        .build()
}

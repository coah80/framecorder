//! Puts the app icon into the Windows exe, so Explorer, the taskbar and the
//! start menu show it. Nothing to do anywhere else.

fn main() {
    println!("cargo::rerun-if-changed=../app/icons/icon.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../app/icons/icon.ico");
        res.compile().expect("couldn't put the icon in the exe");
    }
}

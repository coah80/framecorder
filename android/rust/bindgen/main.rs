//! Writes the Kotlin side of the FFI from the built library:
//! `cargo run -p uniffi-bindgen -- generate --library <libframecorder_ffi.so> --language kotlin --out-dir <dir>`

fn main() {
    uniffi::uniffi_bindgen_main()
}

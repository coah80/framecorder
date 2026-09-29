use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let src = "shaders/convert.comp";
    println!("cargo::rerun-if-changed={src}");

    let status = Command::new("glslc")
        .args(["-O", "--target-env=vulkan1.1", "-o"])
        .arg(out.join("convert.spv"))
        .arg(src)
        .status()
        .expect("glslc not found, install shaderc");
    assert!(status.success(), "failed to compile {src}");
}

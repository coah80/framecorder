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

    link_ffmpeg_deps();
}

/// Our FFmpeg is static, so what it uses itself has to be linked in too (x264
/// for the tests' build, libm, libatomic). ffmpeg-sys-next only links FFmpeg's
/// own libraries.
fn link_ffmpeg_deps() {
    let dir = PathBuf::from(std::env::var("FFMPEG_DIR").expect("FFMPEG_DIR isn't set, see .cargo/config.toml"));
    println!("cargo::rerun-if-changed={}", dir.join(".built").display());
    let mut libs = Vec::new();
    for name in ["libavformat", "libavcodec", "libavutil"] {
        let pc = dir.join(format!("lib/pkgconfig/{name}.pc"));
        let text = std::fs::read_to_string(&pc)
            .unwrap_or_else(|_| panic!("no FFmpeg in {}: run packaging/ffmpeg.sh first", dir.display()));
        // a static-only build lists them under Libs, a shared one under Libs.private
        let flags = text.lines().filter_map(|l| l.strip_prefix("Libs:").or_else(|| l.strip_prefix("Libs.private:")));
        for lib in flags.flat_map(str::split_whitespace).filter_map(|f| f.strip_prefix("-l")) {
            if !lib.starts_with("av") && !libs.contains(&lib.to_string()) {
                libs.push(lib.to_string());
            }
        }
    }
    for lib in libs {
        println!("cargo::rustc-link-lib={lib}");
    }
}

use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=mojo/simd_matcher.mojo");
    let out_dir = match std::env::var("OUT_DIR") {
        Ok(d) => d,
        Err(_) => return,
    };
    let dest_path = Path::new(&out_dir).join("libsimd_matcher.so");

    if let Ok(status) = Command::new("mojo")
        .args([
            "build",
            "--emit",
            "shared-lib",
            "mojo/simd_matcher.mojo",
            "-o",
        ])
        .arg(&dest_path)
        .status()
    {
        if status.success() {
            println!(
                "cargo:rustc-env=SPARK_AEGIS_SO_BUILT={}",
                dest_path.display()
            );
            let _ = std::fs::copy(&dest_path, "mojo/libsimd_matcher.so");
        } else {
            println!("cargo:warning=mojo build failed, fallback to pure Rust engine");
        }
    } else {
        println!("cargo:warning=mojo toolchain unavailable, fallback to pure Rust engine");
    }
}

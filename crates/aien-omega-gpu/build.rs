//! Links `libomega_gpu.a` from the omega commit pinned in `omega.lock`.
//!
//! Offline by design: nothing is downloaded and no CUDA/nvcc is used. Order:
//!   1. `AIEN_FORCE_CPU_STUB=1`      -> stub (CI without the chip).
//!   2. `AIEN_OMEGA_GPU_LIB=<file>`  -> link that prebuilt archive (not sha-checked).
//!   3. `AIEN_OMEGA_DIR=<checkout>`  -> its HEAD must equal omega.lock; built with
//!      `make libomega_gpu` into OUT_DIR. `AIEN_PHYSICS_DIR` (default `<omega>/../physics`)
//!      is passed as PHYSICS_DIR; omega's own physics.lock check still runs.
//!   4. otherwise                    -> stub, with a build warning.
//!
//! `cfg(has_omega_gpu)` is set only when the real library is linked.
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_omega_gpu)");
    for v in [
        "AIEN_FORCE_CPU_STUB",
        "AIEN_OMEGA_GPU_LIB",
        "AIEN_OMEGA_DIR",
        "AIEN_PHYSICS_DIR",
    ] {
        println!("cargo:rerun-if-env-changed={v}");
    }
    let lock_path = Path::new(&env("CARGO_MANIFEST_DIR")).join("../../omega.lock");
    println!("cargo:rerun-if-changed={}", lock_path.display());
    let pinned = std::fs::read_to_string(&lock_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", lock_path.display()))
        .trim()
        .to_string();
    assert!(
        pinned.len() == 40 && pinned.bytes().all(|b| b.is_ascii_hexdigit()),
        "omega.lock must hold one full 40-hex sha"
    );
    println!("cargo:rustc-env=AIEN_OMEGA_PINNED_SHA={pinned}");

    if std::env::var("AIEN_FORCE_CPU_STUB").as_deref() == Ok("1") {
        return;
    }
    if let Ok(lib) = std::env::var("AIEN_OMEGA_GPU_LIB") {
        let lib = PathBuf::from(lib);
        assert!(
            lib.is_file(),
            "AIEN_OMEGA_GPU_LIB {} is not a file",
            lib.display()
        );
        link(&lib);
        return;
    }
    let Ok(dir) = std::env::var("AIEN_OMEGA_DIR") else {
        println!("cargo:warning=aien-omega-gpu: no AIEN_OMEGA_DIR or AIEN_OMEGA_GPU_LIB, building the CPU stub");
        return;
    };
    let dir = PathBuf::from(dir);
    let head = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    assert!(
        head == pinned,
        "omega checkout {} is at '{head}', omega.lock pins {pinned}",
        dir.display()
    );
    let physics = std::env::var("AIEN_PHYSICS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dir.join("../physics"));
    let out = PathBuf::from(env("OUT_DIR")).join("omega-build");
    std::fs::create_dir_all(&out).expect("create omega build dir");
    let status = Command::new("make")
        .current_dir(&dir)
        .arg(format!("OUT_DIR={}", out.display()))
        .arg(format!("PHYSICS_DIR={}", physics.display()))
        .arg(out.join("libomega_gpu.a"))
        .status()
        .expect("run make");
    assert!(
        status.success(),
        "make libomega_gpu failed in {}",
        dir.display()
    );
    link(&out.join("libomega_gpu.a"));
}

fn link(lib: &Path) {
    let dir = lib.parent().expect("lib has a parent dir");
    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=static=omega_gpu");
    println!("cargo:rustc-link-lib=dylib=pthread");
    println!("cargo:rustc-link-lib=dylib=m");
    println!("cargo:rerun-if-changed={}", lib.display());
    println!("cargo:rustc-cfg=has_omega_gpu");
}

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} not set"))
}

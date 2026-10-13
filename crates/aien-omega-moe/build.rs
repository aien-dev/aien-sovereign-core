//! Decides whether omega's MoE window API (`src/omega_gpu_moe_api.h`, OWN-30B cut 2) is linked.
//!
//! The archive itself is linked by `aien-omega-gpu`'s build script (this crate only declares
//! the extra symbols), so the decision mirrors its inputs:
//!   * `AIEN_FORCE_CPU_STUB=1`: stub.
//!   * `AIEN_OMEGA_GPU_LIB=<archive>`: native when `nm` finds
//!     `omega_gpu_matmul_resident_windows_f32` defined in that archive.
//!   * `AIEN_OMEGA_DIR=<checkout>`: native when the checkout has `src/omega_gpu_moe_api.h`.
//!   * otherwise: stub (every call returns `Unavailable`).
//!
//! `cfg(has_omega_moe)` is set only when native. A stub never fails the build: the whole-tensor
//! fallback in the model code stays usable, and `is_native()` says which one a binary carries.
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_omega_moe)");
    for v in ["AIEN_FORCE_CPU_STUB", "AIEN_OMEGA_GPU_LIB", "AIEN_OMEGA_DIR"] {
        println!("cargo:rerun-if-env-changed={v}");
    }
    if std::env::var("AIEN_FORCE_CPU_STUB").as_deref() == Ok("1") {
        return;
    }
    if let Ok(lib) = std::env::var("AIEN_OMEGA_GPU_LIB") {
        println!("cargo:rerun-if-changed={lib}");
        if archive_defines(Path::new(&lib), "omega_gpu_matmul_resident_windows_f32") {
            println!("cargo:rustc-cfg=has_omega_moe");
        } else {
            println!("cargo:warning=aien-omega-moe: {lib} does not define the MoE window API; window calls are stubbed");
        }
        return;
    }
    if let Ok(dir) = std::env::var("AIEN_OMEGA_DIR") {
        let header = Path::new(&dir).join("src/omega_gpu_moe_api.h");
        println!("cargo:rerun-if-changed={}", header.display());
        if header.is_file() {
            println!("cargo:rustc-cfg=has_omega_moe");
        } else {
            println!("cargo:warning=aien-omega-moe: {dir} has no src/omega_gpu_moe_api.h (omega before OWN-30B cut 2); window calls are stubbed");
        }
    }
}

fn archive_defines(lib: &Path, symbol: &str) -> bool {
    let Ok(out) = Command::new("nm").arg(lib).output() else {
        println!("cargo:warning=aien-omega-moe: nm not available, assuming the archive lacks the MoE window API");
        return false;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|l| l.ends_with(&format!(" T {symbol}")))
}

//! Links omega `librx_compose.a` (COMPOSITION-2 + the rxc_host ABI facade).
//!
//! Offline: nothing is downloaded. Order:
//!   1. `AIEN_FORCE_CPU_STUB=1`          -> stub (no library; every call returns
//!      `ComposeError::Unavailable`).
//!   2. `AIEN_OMEGA_COMPOSE_LIB=<file>`  -> link that prebuilt archive (not sha-checked).
//!   3. `AIEN_OMEGA_COMPOSE_DIR=<checkout>`, else `AIEN_OMEGA_DIR` (the checkout
//!      aien-omega-gpu builds from) -> its HEAD must equal omega.lock, then
//!      `make <OUT_DIR>/librx_compose.a` runs in it. `AIEN_OMEGA_COMPOSE_SHA=<40-hex>`
//!      replaces the expected sha: a documented developer override for building
//!      against an unmerged omega branch, never used for a release or a receipt. `AIEN_PHYSICS_DIR` (default `<omega>/../physics`) is passed
//!      as PHYSICS_DIR and `AIEN_AIENOS_LOCK_REPO` (if set) as AIENOS_LOCK_REPO;
//!      omega's own lock checks still run.
//!   4. otherwise                        -> stub, with a build warning.
//!
//! `AIEN_OMEGA_COMPOSE_DIR` exists so a developer override never touches the
//! checkout aien-omega-gpu checks against omega.lock.
//! `cfg(has_omega_compose)` is set only when the real library is linked.
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_omega_compose)");
    println!("cargo:rustc-check-cfg=cfg(has_omega_wait_ms)");
    for v in [
        "AIEN_FORCE_CPU_STUB",
        "AIEN_OMEGA_COMPOSE_LIB",
        "AIEN_OMEGA_COMPOSE_DIR",
        "AIEN_OMEGA_DIR",
        "AIEN_OMEGA_COMPOSE_SHA",
        "AIEN_PHYSICS_DIR",
        "AIEN_AIENOS_LOCK_REPO",
    ] {
        println!("cargo:rerun-if-env-changed={v}");
    }
    let lock_path = Path::new(&env("CARGO_MANIFEST_DIR")).join("../../omega.lock");
    println!("cargo:rerun-if-changed={}", lock_path.display());
    let pinned = std::fs::read_to_string(&lock_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", lock_path.display()))
        .trim()
        .to_string();
    assert!(is_sha(&pinned), "omega.lock must hold one full 40-hex sha");
    let expected = match std::env::var("AIEN_OMEGA_COMPOSE_SHA") {
        Ok(s) => {
            let s = s.trim().to_string();
            assert!(
                is_sha(&s),
                "AIEN_OMEGA_COMPOSE_SHA must be a full 40-hex sha"
            );
            println!("cargo:warning=aien-omega-compose: AIEN_OMEGA_COMPOSE_SHA={s} overrides omega.lock {pinned} (developer override, not omega.lock)");
            s
        }
        Err(_) => pinned.clone(),
    };
    println!("cargo:rustc-env=AIEN_OMEGA_COMPOSE_EXPECTED_SHA={expected}");

    if std::env::var("AIEN_FORCE_CPU_STUB").as_deref() == Ok("1") {
        return;
    }
    if let Ok(lib) = std::env::var("AIEN_OMEGA_COMPOSE_LIB") {
        let lib = PathBuf::from(lib);
        assert!(
            lib.is_file(),
            "AIEN_OMEGA_COMPOSE_LIB {} is not a file",
            lib.display()
        );
        require_wait_ms_symbol(&lib);
        link(&lib);
        return;
    }
    let Ok(dir) =
        std::env::var("AIEN_OMEGA_COMPOSE_DIR").or_else(|_| std::env::var("AIEN_OMEGA_DIR"))
    else {
        println!("cargo:warning=aien-omega-compose: no AIEN_OMEGA_COMPOSE_DIR, AIEN_OMEGA_DIR or AIEN_OMEGA_COMPOSE_LIB, building the stub");
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
        head == expected,
        "omega checkout {} is at '{head}', expected {expected}",
        dir.display()
    );
    let physics = std::env::var("AIEN_PHYSICS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dir.join("../physics"));
    let out = PathBuf::from(env("OUT_DIR")).join("omega-compose-build");
    std::fs::create_dir_all(&out).expect("create omega build dir");
    let mut make = Command::new("make");
    make.current_dir(&dir)
        .env_remove("MAKEFLAGS")
        .env_remove("MFLAGS")
        .arg(format!("OUT_DIR={}", out.display()))
        .arg(format!("PHYSICS_DIR={}", physics.display()));
    if let Ok(repo) = std::env::var("AIEN_AIENOS_LOCK_REPO") {
        make.arg(format!("AIENOS_LOCK_REPO={repo}"));
    }
    let status = make
        .arg(out.join("librx_compose.a"))
        .status()
        .expect("run make");
    assert!(
        status.success(),
        "make librx_compose.a failed in {}",
        dir.display()
    );
    detect_wait_ms(&dir);
    require_wait_ms_symbol(&out.join("librx_compose.a"));
    link(&out.join("librx_compose.a"));
}

/// Check the omega checkout's host header. omega.lock pins a commit that
/// declares `rxc_host_set_wait_ms`, so a real link without it is a build error, never a silent
/// downgrade (the 120 s document budget would be refused at runtime).
fn detect_wait_ms(dir: &Path) {
    let h = dir.join("src/runtime/rxc_host_abi.h");
    let text =
        std::fs::read_to_string(&h).unwrap_or_else(|e| panic!("cannot read {}: {e}", h.display()));
    assert!(
        header_declares_wait_ms(&text),
        "omega header {} does not declare rxc_host_set_wait_ms; omega.lock pins a commit that has it, so this checkout is not usable. Use AIEN_FORCE_CPU_STUB=1 for a stub build.",
        h.display()
    );
}

/// A real declaration, not a mention in a comment: a line that starts with
/// `int rxc_host_set_wait_ms(`.
fn header_declares_wait_ms(header: &str) -> bool {
    header
        .lines()
        .any(|l| l.trim_start().starts_with("int rxc_host_set_wait_ms("))
}

/// Fail the build unless the linked archive defines `rxc_host_set_wait_ms` (checked with `nm`
/// on both the make path and a prebuilt `AIEN_OMEGA_COMPOSE_LIB`), then set `has_omega_wait_ms`.
fn require_wait_ms_symbol(lib: &Path) {
    let out = Command::new("nm")
        .arg("-g")
        .arg("--defined-only")
        .arg(lib)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "cannot run `nm` to check {} for rxc_host_set_wait_ms ({e}); install binutils or use AIEN_FORCE_CPU_STUB=1",
                lib.display()
            )
        });
    assert!(
        out.status.success(),
        "`nm` failed on {}: {}",
        lib.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        archive_defines_wait_ms(&text),
        "{} does not define rxc_host_set_wait_ms; omega.lock pins a commit that has it. Rebuild the archive from that omega, or use AIEN_FORCE_CPU_STUB=1 for a stub build.",
        lib.display()
    );
    println!("cargo:rustc-cfg=has_omega_wait_ms");
}

fn archive_defines_wait_ms(nm_out: &str) -> bool {
    nm_out
        .lines()
        .any(|l| l.split_whitespace().last() == Some("rxc_host_set_wait_ms"))
}

fn link(lib: &Path) {
    let dir = lib.parent().expect("lib has a parent dir");
    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=static=rx_compose");
    println!("cargo:rustc-link-lib=dylib=pthread");
    println!("cargo:rustc-link-lib=dylib=m");
    println!("cargo:rerun-if-changed={}", lib.display());
    println!("cargo:rustc-cfg=has_omega_compose");
    println!("cargo:linked=1");
}

fn is_sha(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} not set"))
}

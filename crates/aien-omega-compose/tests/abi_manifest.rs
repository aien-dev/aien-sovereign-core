//! The Rust mirror of the rxc_host ABI (src/ffi.rs) must equal abi/rxc_host_abi.manifest,
//! field by field (offset, size, kind, order), constant by constant and signature by
//! signature. Runs in every build, the stub included.

#[test]
fn rust_ffi_matches_the_manifest() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("abi/rxc_host_abi.manifest");
    aien_abi_conformance::assert_manifest(&aien_omega_compose::ffi::rust_abi(), &path);
}

/// What this test binary linked, as `build.rs` recorded it. Prints one
/// `AIEN_NATIVE_IDENTITY {json}` line (scripts/abi-qualify.sh collects it). A linked build
/// must never report the stub, a stub build must never qualify, and with
/// `AIEN_ABI_QUALIFY=1` (the qualification run) the identity must qualify.
#[test]
fn native_identity_matches_the_link() {
    let id = aien_omega_compose::native_identity();
    println!("AIEN_NATIVE_IDENTITY {}", id.to_json());
    if cfg!(has_omega_compose) {
        assert_ne!(id.link, "stub", "{id:?}");
        assert!(id.digest != "none", "{id:?}");
    } else {
        assert_eq!(id.link, "stub", "{id:?}");
        assert!(!id.qualifies());
    }
    if std::env::var("AIEN_ABI_QUALIFY").as_deref() == Ok("1") {
        assert!(
            id.qualifies(),
            "{} does not qualify: {:?}",
            id.component,
            id.gaps()
        );
    }
}

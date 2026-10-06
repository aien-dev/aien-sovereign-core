//! NEXT-PHASE-1 v5: runs the shell negative tests of the ACCEPTANCE-v5 receipt
//! rows (docs/campaigns/next-phase-1/test-rows-v5.sh) under `cargo test`.
//! Needs bash and jq; a missing tool fails the test, it is never skipped.
use std::path::Path;
use std::process::Command;

#[test]
fn acceptance_v5_rows_fail_on_v4_reply_and_synthetic_failures() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/campaigns/next-phase-1/test-rows-v5.sh");
    assert!(script.is_file(), "missing {}", script.display());
    let out = Command::new("bash")
        .arg(&script)
        .output()
        .expect("bash must be installed to run the v5 receipt tests");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "test-rows-v5.sh failed\n{stdout}\n{stderr}"
    );
    assert!(stdout.contains(" 0 failed"), "{stdout}");
}

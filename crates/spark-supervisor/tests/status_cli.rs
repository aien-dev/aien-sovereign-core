//! `spark-supervisor status` must refuse instead of printing a table that looks live.

use std::process::Command;

#[test]
fn status_refuses_with_nonzero_exit_and_writes_nothing() {
    let dir = std::env::temp_dir().join(format!("spark-supervisor-status-{}", std::process::id()));
    let config = dir.join("supervisor.toml");
    let _ = std::fs::remove_dir_all(&dir);

    let out = Command::new(env!("CARGO_BIN_EXE_spark-supervisor"))
        .arg("--config")
        .arg(&config)
        .arg("status")
        .output()
        .expect("run binary");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("live status is not available from a separate process"),
        "stderr={stderr}"
    );
    assert!(!stdout.contains("SERVICE"), "table printed: {stdout}");
    assert!(!stdout.contains("RUNNING") && !stdout.contains("STOPPED"));
    assert!(!config.exists(), "status must not generate a config file");
}

#[test]
fn check_still_succeeds_on_valid_config() {
    let dir = std::env::temp_dir().join(format!("spark-supervisor-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("supervisor.toml");
    std::fs::write(
        &config,
        "[services.a]\ncommand = \"sleep\"\nargs = [\"1\"]\n",
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_spark-supervisor"))
        .arg("--config")
        .arg(&config)
        .arg("check")
        .output()
        .expect("run binary");
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("is valid"));
    let _ = std::fs::remove_dir_all(&dir);
}

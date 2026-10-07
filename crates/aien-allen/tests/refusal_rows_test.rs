//! The no-provision invariant (source scan, G5 analog) and the arch#150
//! s4(6) row table. Rows that need approvals live outside this crate:
//! DEPENDENCY sc#249, NOT_RUN.
use std::path::PathBuf;

fn src_files() -> Vec<(String, String)> {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    std::fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect()
}

const BANNED: &[&str] = &[
    "fn encode_subject",
    "fn genesis",
    "fn provision",
    "fn mint",
    "cs_provision",
    "fn create_subject",
    "thread::spawn",
    "std::thread",
    "tokio",
];

fn violations(text: &str) -> Vec<&'static str> {
    BANNED
        .iter()
        .copied()
        .filter(|b| text.contains(b))
        .collect()
}

#[test]
fn production_source_has_no_provision_or_encode_path() {
    for (name, text) in src_files() {
        assert!(
            violations(&text).is_empty(),
            "{name}: {:?}",
            violations(&text)
        );
    }
    // The subject module has a decoder and an id function only.
    let s = src_files()
        .into_iter()
        .find(|(n, _)| n == "subject_v0.rs")
        .unwrap()
        .1;
    assert!(!s.contains("pub fn encode"));
    // Only the pin and deployment writers touch the filesystem for writing.
    for (name, text) in src_files() {
        let writes = text.contains("fs::write") || text.contains("File::create");
        assert_eq!(writes, name == "binding.rs", "{name}: unexpected writer");
    }
}

#[test]
fn the_scan_would_catch_a_provision_function() {
    // Negative control: the scan flags a provisioning function.
    assert_eq!(violations("pub fn provision() {}"), vec!["fn provision"]);
    assert!(violations("pub fn decode() {}").is_empty());
}

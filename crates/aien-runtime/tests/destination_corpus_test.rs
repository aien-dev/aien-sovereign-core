//! The destination parser over every campaign goal corpus in this repository
//! (v5 G6 finding: two independently authored v5 goals were refused before
//! the model). For each goal the probe records what `named_destination` reads
//! and what `classify_destination` then decides, and compares the whole table
//! with the committed record
//! `docs/campaigns/open-model-qwen3/evidence-v5/destination-probe.tsv`.
//! Any change to how a real goal is read shows up here as a diff.
//!
//! The workspace is empty and `is_file` sees no files, so an edit goal whose
//! file a campaign seeds reads as `New` here; the parse column is what matters.
//! To rewrite the record: `DEST_PROBE_WRITE=<path> cargo test -p aien-runtime
//! --test destination_corpus_test`, then review the diff.
use aien_runtime::destination::named_destination;
use aien_runtime::spine::{classify_destination, TargetClass};
use std::path::{Path, PathBuf};

const CORPUS: &[&str] = &[
    "docs/campaigns/open-model-qwen3/tasks-oq3-v5.json",
    "docs/campaigns/open-model-qwen3/tasks-oq3-v4.json",
    "docs/campaigns/open-model-qwen3/tasks-oq3-v3.json",
    "docs/campaigns/open-model-qwen3/dryrun-tasks-v2.json",
    "docs/campaigns/open-model-qwen3/diagnostic-long-1-tasks.json",
    "docs/campaigns/next-phase-1/tasks-v5.json",
    "docs/campaigns/next-phase-1/tasks-v6.json",
    "docs/campaigns/next-phase-1/tasks-v8.json",
];
const RECORD: &str = "docs/campaigns/open-model-qwen3/evidence-v5/destination-probe.tsv";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// (corpus, id, goal) for every task of every corpus file.
fn goals() -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for f in CORPUS {
        let text = std::fs::read_to_string(repo().join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        for t in v["tasks"]
            .as_array()
            .unwrap_or_else(|| panic!("{f}: no tasks"))
        {
            out.push((
                f.to_string(),
                t["id"].as_str().unwrap().to_string(),
                t["goal"].as_str().unwrap().to_string(),
            ));
        }
    }
    out
}

fn verdict(c: &TargetClass) -> String {
    match c {
        TargetClass::New => "New".into(),
        TargetClass::Edit(p, _) => format!("Edit({p})"),
        TargetClass::Refused(why) => format!("Refused({why})"),
    }
}

fn table(ws: &Path) -> String {
    let mut s = String::from("corpus\tid\tparse\tverdict\n");
    for (f, id, goal) in goals() {
        let parse = match named_destination(&goal, &|_| false) {
            Ok(Some(p)) => format!("dest {p}"),
            Ok(None) => "none".into(),
            Err(e) => format!("error {e}"),
        };
        let (class, _) = classify_destination(&goal, ws);
        s.push_str(&format!("{f}\t{id}\t{parse}\t{}\n", verdict(&class)));
    }
    s
}

#[test]
fn every_campaign_goal_reads_as_recorded() {
    let ws = tempfile::tempdir().unwrap();
    let now = table(ws.path());
    assert!(goals().len() >= 60, "a corpus file went missing");
    if let Ok(p) = std::env::var("DEST_PROBE_WRITE") {
        std::fs::write(&p, &now).unwrap();
    }
    let record = std::fs::read_to_string(repo().join(RECORD)).unwrap_or_default();
    assert!(
        now == record,
        "the destination parser reads a campaign goal differently from {RECORD}:\n{}",
        now.lines()
            .zip(record.lines().chain(std::iter::repeat("")))
            .filter(|(a, b)| a != b)
            .map(|(a, b)| format!("  now:    {a}\n  record: {b}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The three v5 goals the G6 finding is about, by their declared destination.
#[test]
fn v5_goals_resolve_to_their_declared_destinations() {
    let ws = tempfile::tempdir().unwrap();
    let v5: Vec<_> = goals()
        .into_iter()
        .filter(|(f, _, _)| f.ends_with("tasks-oq3-v5.json"))
        .collect();
    let goal = |id: &str| v5.iter().find(|(_, i, _)| i == id).unwrap().2.clone();
    for (id, want) in [
        ("D2", "docs/rename-photos.md"),
        ("D4", "docs/choir-summary.md"),
    ] {
        assert_eq!(
            named_destination(&goal(id), &|_| false),
            Ok(Some(want.to_string())),
            "{id}"
        );
        assert_eq!(
            classify_destination(&goal(id), ws.path()).0,
            TargetClass::New,
            "{id}"
        );
    }
    // N1 is read, then refused as unsafe (outside the workspace).
    assert_eq!(
        named_destination(&goal("N1"), &|_| false),
        Ok(Some("../shared-stuff/reminder.txt".to_string()))
    );
    assert!(matches!(
        classify_destination(&goal("N1"), ws.path()).0,
        TargetClass::Refused(why) if why.contains("cannot be written safely")
    ));
}

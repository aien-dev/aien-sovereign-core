//! Issue #288: the task kind, prompt and budget follow the DESTINATION the
//! goal names, not the first path that happens to exist.

use aien_runtime::spine::{classify_target, task_decision, task_plan, ProposalKind, TargetClass};
use std::path::PathBuf;

fn ws_with(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let ws = d.path().join("ws");
    std::fs::create_dir_all(ws.join("docs")).unwrap();
    for (n, c) in files {
        std::fs::write(ws.join(n), c).unwrap();
    }
    let ws = std::fs::canonicalize(ws).unwrap();
    (d, ws)
}

#[test]
fn create_new_file_about_an_existing_one_is_a_new_document() {
    let (_d, ws) = ws_with(&[("README.md", "# readme\n")]);
    let goal = "Create docs/SUMMARY.md about README.md";
    assert_eq!(classify_target(goal, &ws), TargetClass::New);
    let ((prompt, target, kind), dest) = task_decision(goal, &ws).unwrap();
    assert_eq!((target, kind), (None, ProposalKind::Document));
    assert_eq!(dest.as_deref(), Some("docs/SUMMARY.md"));
    assert!(!prompt.contains("already exists"));
}

#[test]
fn update_destination_using_a_source_is_an_edit_of_the_destination() {
    let (_d, ws) = ws_with(&[("README.md", "# readme\n"), ("docs/NOTES.md", "n\n")]);
    let (p, t, k) = task_plan("Update README.md using docs/NOTES.md", &ws).unwrap();
    assert_eq!(k, ProposalKind::Edit);
    assert_eq!(t, Some(("README.md".into(), "# readme\n".into())));
    assert!(p.contains("The file README.md already exists"));
}

#[test]
fn append_to_existing_guide_is_an_edit() {
    let (_d, ws) = ws_with(&[("docs/GUIDE.md", "# guide\n")]);
    let (_, t, k) = task_plan("Append a section to docs/GUIDE.md", &ws).unwrap();
    assert_eq!(
        (t.map(|t| t.0), k),
        (Some("docs/GUIDE.md".into()), ProposalKind::Edit)
    );
}

#[test]
fn missing_destination_with_existing_source_is_new() {
    let (_d, ws) = ws_with(&[("src.txt", "s\n")]);
    let (_, t, k) = task_plan("Write out.md based on src.txt", &ws).unwrap();
    assert_eq!((t, k), (None, ProposalKind::Document));
}

#[test]
fn unsafe_destinations_are_refused_with_a_reason() {
    let d = tempfile::tempdir().unwrap();
    let outside = d.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("s.txt"), "s\n").unwrap();
    let ws = d.path().join("ws");
    std::fs::create_dir_all(ws.join("docs")).unwrap();
    std::os::unix::fs::symlink(outside.join("s.txt"), ws.join("link.md")).unwrap();
    std::os::unix::fs::symlink(&outside, ws.join("linkdir")).unwrap();
    std::fs::write(ws.join("README.md"), "r\n").unwrap();
    let ws = std::fs::canonicalize(ws).unwrap();
    let abs = format!("Create {} about README.md", outside.join("n.md").display());
    for goal in [
        "Create ../x.md about README.md",
        abs.as_str(),
        "Create ~/x.md about README.md",
        "Update link.md using README.md",
        "Create linkdir/new.md about README.md",
    ] {
        let TargetClass::Refused(why) = classify_target(goal, &ws) else {
            panic!("not refused: {goal}");
        };
        assert!(why.contains("destination"), "{goal}: {why}");
        assert!(task_plan(goal, &ws).is_err(), "{goal}");
    }
}

#[test]
fn two_destinations_are_an_ambiguous_refusal_naming_both() {
    let (_d, ws) = ws_with(&[("a.md", "a\n")]);
    for goal in ["Create a.md and b.md", "Update a.md. Then create b.md"] {
        let e = task_plan(goal, &ws).unwrap_err();
        assert!(
            e.contains("ambiguous destination") && e.contains("a.md") && e.contains("b.md"),
            "{e}"
        );
    }
}

#[test]
fn source_only_goal_is_refused_with_candidates() {
    let (_d, ws) = ws_with(&[("README.md", "# r\n")]);
    for goal in ["Summarise README.md", "Edit the given README.md "] {
        let r = task_plan(goal, &ws);
        if goal.starts_with("Edit") {
            let (_, t, k) = r.unwrap();
            assert_eq!(
                (t.map(|t| t.0), k),
                (Some("README.md".into()), ProposalKind::Edit)
            );
        } else {
            let e = r.unwrap_err();
            assert!(
                e.contains("no destination named") && e.contains("README.md"),
                "{e}"
            );
        }
    }
    let (_, t, _) = task_plan("Update the reference README.md", &ws).unwrap();
    assert_eq!(t.map(|t| t.0), Some("README.md".into()));
}

#[test]
fn new_document_prompt_states_the_destination() {
    let (_d, ws) = ws_with(&[("README.md", "# r\n")]);
    let (p, _, _) = task_plan("Create docs/SUMMARY.md about README.md", &ws).unwrap();
    assert!(p.contains("filename: docs/SUMMARY.md"), "{p}");
}

#[test]
fn rename_is_refused() {
    let (_d, ws) = ws_with(&[("a.md", "a\n")]);
    assert!(task_plan("Rename a.md to b.md", &ws)
        .unwrap_err()
        .contains("rename is not supported"));
}

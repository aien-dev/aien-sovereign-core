//! sc#336: a one-sentence or one-line file was refused as uncertain before any
//! model call. "a one-sentence X" and "a single sentence" are now read as a
//! file of exactly one sentence (`SingleSentence`), and "containing just one
//! line that says \"S\"" as exactly one line holding S. A goal whose
//! destination is outside the workspace is refused for that destination
//! first, before any requirement is read.
use aien_runtime::requirements::{analyze, refusal_reason, Requirement};
use aien_runtime::spine::task_decision;

use Requirement::*;

/// tasks-oq3-v4 R1, verbatim.
const V4_R1: &str = "Create the file docs/THANKS.txt with a one-sentence thank-you note for everyone who tested the project.";
/// v5 R1 (sc#339), verbatim.
const V5_R1: &str = "Save a one-sentence reminder about stretching the shoulders before swimming into notes/swim-tip.txt.";
/// v5 N1 (sc#339), verbatim.
const V5_N1: &str = "Create a file at ../shared-stuff/reminder.txt containing just one line that says \"Renew the library card on Friday.\"";

fn reqs(goal: &str) -> Vec<Requirement> {
    let ex = analyze(goal);
    assert!(
        ex.uncertain.is_empty(),
        "{goal:?} should be read, uncertain: {:?}",
        ex.uncertain
    );
    ex.requirements
}

#[test]
fn the_one_sentence_goals_are_read() {
    assert_eq!(reqs(V4_R1), [SingleSentence]);
    assert_eq!(reqs(V5_R1), [SingleSentence]);
    for goal in [
        "Write a single sentence about tea into notes/tea.txt.",
        "Save a single-sentence note to notes/a.txt.",
        "Write just one sentence about tea.",
    ] {
        assert_eq!(reqs(goal), [SingleSentence], "{goal}");
    }
}

#[test]
fn the_one_line_that_says_goal_is_read() {
    let r = reqs(V5_N1);
    assert!(
        r.contains(&AddedLines {
            n: 1,
            exact: true,
            prior: None
        }),
        "{r:?}"
    );
    assert!(
        r.contains(&RequiredPhrases(vec![
            "Renew the library card on Friday".to_string()
        ])),
        "{r:?}"
    );
}

#[test]
fn a_one_sentence_file_is_checked() {
    let r = [SingleSentence];
    for good in [
        "Thank you to everyone who tested the project!\n",
        "Stretch your shoulders before you swim.",
        "Stretch your shoulders before you swim\n",
    ] {
        assert!(refusal_reason(&r, good).is_none(), "{good:?}");
    }
    for bad in [
        "",
        "Thanks to all. See you soon.\n",
        "Thanks to all.\nAnd more thanks to you.\n",
    ] {
        assert!(refusal_reason(&r, bad).is_some(), "{bad:?}");
    }
}

#[test]
fn other_sentence_wordings_stay_uncertain() {
    for goal in [
        "Write a one-sentence summary at the top of each section.",
        "Add one sentence per section.",
        "Do not write a one-sentence note.",
        "Write a two-sentence note.",
        "Write a one-sentence note in every paragraph.",
        // an addition: the file may hold more than the new sentence
        "Add a one-sentence summary to README.md.",
        "Append a single sentence to notes/a.txt.",
    ] {
        let ex = analyze(goal);
        assert!(
            !ex.uncertain.is_empty(),
            "{goal:?} should stay uncertain, got {:?}",
            ex.requirements
        );
    }
}

#[test]
fn an_outside_destination_is_refused_first_and_named() {
    let d = tempfile::tempdir().unwrap();
    let ws = d.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let ws = std::fs::canonicalize(ws).unwrap();
    // The destination is refused by `task_decision`, which runs before the
    // requirement reader in `run_task_inner`.
    let why = task_decision(V5_N1, &ws).expect_err("outside the workspace");
    assert!(why.contains("../shared-stuff/reminder.txt"), "{why}");
}

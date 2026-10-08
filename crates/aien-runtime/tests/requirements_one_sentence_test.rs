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

#[test]
fn a_short_second_sentence_still_counts() {
    let r = [SingleSentence];
    for bad in [
        "Thanks for coming. Bye.\n",
        "Thanks for the help.\n\nBye!\n",
        "Thanks for the help. OK\n",
        "Thanks for the help to the whole project. Cheers\n",
        "Run took 5 ms. Done.\n",
        "He lives on Main St. He is nice.\n",
        "Thanks for the help\nBye\n",
        "Thanks.Bye.\n",
        "Thanks for coming\u{3002}Bye\u{3002}\n",
    ] {
        assert!(refusal_reason(&r, bad).is_some(), "{bad:?}");
    }
    for good in [
        "Hi.\n",
        "Dr. Lee thanks everyone who tested the project.\n",
        "Version 3.5 is out, e.g. for testers.\n",
        "Thanks to everyone who tested the project\u{2026}\n",
        "Read README.md before you test the project.\n",
    ] {
        assert!(refusal_reason(&r, good).is_none(), "{good:?}");
    }
}

#[test]
fn one_sentence_beside_other_content_or_an_edit_stays_uncertain() {
    for goal in [
        "Create a.txt with a one-sentence abstract and a one-sentence conclusion.",
        "Create a.txt with a one-sentence note. Append a goodbye.",
        "Update README.md with a one-sentence summary.",
        "Replace the intro with a single sentence.",
        "Rewrite notes.txt as a one-sentence note.",
        "Put a one-sentence summary at the top of README.md.",
        "Create a.txt with one-sentence notes.",
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
fn a_said_line_that_cannot_be_read_whole_stays_uncertain() {
    for goal in [
        "Create a.txt containing one line that says \"unclosed.",
        "Create a.txt containing one line that says \"\".",
        "Create a.txt containing one line that says \"Hi \" there\".",
        "Create a.txt containing one line that says \"Hi.\" in bold.",
        "Create a.txt containing one line that says \"Hi there\" and another line.",
    ] {
        let ex = analyze(goal);
        assert!(
            !ex.uncertain.is_empty(),
            "{goal:?} should stay uncertain, got {:?}",
            ex.requirements
        );
    }
    // a quote that closes the goal sentence, followed by a new sentence
    let r = reqs("Create a.txt containing one line that says \"Call Mum.\" Then save it.");
    assert!(
        r.contains(&RequiredPhrases(vec!["Call Mum".to_string()])),
        "{r:?}"
    );
}

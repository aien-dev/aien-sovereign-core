//! sc#349: `one line saying hello` / `one line that says hello` (no quotes).
//! A short plain phrase that runs to the end of its sentence (or to a trailing
//! destination file) is a required phrase; anything wider is UNCERTAIN, never
//! dropped. The one-line count is kept in every case.
use aien_runtime::control::ControlResponse;
use aien_runtime::requirements::{analyze, refusal_reason, Requirement};
use aien_runtime::spine::{ComposeBridge, ComposeProposer, Generation};
use std::sync::Arc;
use std::time::Duration;

use Requirement::*;

fn read(goal: &str) -> Vec<Requirement> {
    let ex = analyze(goal);
    assert!(ex.uncertain.is_empty(), "{goal:?}: {:?}", ex.uncertain);
    ex.requirements
}

fn one_line_with(goal: &str, phrase: &str) {
    let r = read(goal);
    assert!(
        r.iter().any(|q| matches!(
            q,
            AddedLines {
                n: 1,
                exact: true,
                ..
            }
        )),
        "{goal}: {r:?}"
    );
    assert!(
        r.contains(&RequiredPhrases(vec![phrase.to_string()])),
        "{goal}: {r:?}"
    );
}

#[test]
fn a_plain_unquoted_phrase_is_required() {
    one_line_with("Add a single line saying hello to a.md", "hello");
    one_line_with("Create a.txt containing one line that says hello.", "hello");
    one_line_with("Add one line to notes/TODO.md saying buy milk", "buy milk");
    one_line_with("Add one line saying Good Morning.", "Good Morning");
    one_line_with("Add one line to a.md that says hello world!", "hello world");
}

#[test]
fn an_ambiguous_unquoted_phrase_is_uncertain_never_dropped() {
    for goal in [
        "Add a single line saying hello and then save it to a.md",
        "Add a single line saying something nice to a.md",
        "Add one line saying hello, then stop",
        "Add one line saying it to a.md",
        "Create a.txt containing one line that says the project keeps every change inside its workspace.",
        "Add one line saying",
        "Add one line saying save the file to a.md",
        "Add one line saying hello to the end of a.md",
    ] {
        let ex = analyze(goal);
        assert!(!ex.uncertain.is_empty(), "{goal:?} should be uncertain: {:?}", ex.requirements);
    }
}

#[test]
fn unrelated_numbers_and_words_are_unchanged() {
    assert!(read("Fix 2 typos in README.md").is_empty());
    assert!(read("Add a line saying hello to a.md").is_empty());
}

/// tasks-v5 goals (docs/campaigns/next-phase-1/tasks-v5.json), verbatim.
#[test]
fn v5_campaign_goals_read_as_before() {
    for goal in [
        "Create the file NOTES.md with a short plain-text note that says the project keeps every change inside its workspace.",
        "Create the file docs/CONTACT.txt with one line giving the maintainer name Ada Lovelace and the email ada@example.org.",
        "Create the file TODO.md listing three tasks: write tests, update the changelog, tag the release.",
    ] {
        let ex = analyze(goal);
        println!("V5 {goal}\n  -> {:?} | uncertain {:?}", ex.requirements, ex.uncertain);
    }
}

#[test]
fn the_check_refuses_a_line_without_the_phrase() {
    let reqs = analyze("Add a single line saying hello to a.md")
        .resolved("")
        .requirements;
    assert!(refusal_reason(&reqs, "goodbye\n").is_some());
    assert!(refusal_reason(&reqs, "hello\n").is_none());
    assert!(refusal_reason(&reqs, "hello\nhello again\n").is_some());
}

#[test]
fn linked_compose_does_not_commit_a_line_without_the_phrase() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let run = |reply: &'static str, sub: &str| {
        let proposer: ComposeProposer = Arc::new(move |_: &str, _: Duration| {
            Ok(Generation {
                text: reply.to_string(),
                tokens: 5,
                finish_reason: Some("eos".into()),
                ..Default::default()
            })
        });
        let bridge = ComposeBridge::new(tmp.path().join(sub), proposer, "test:said");
        bridge.run_task(
            "Create a.md containing one line saying hello.",
            ws.to_str().unwrap(),
        )
    };
    let resp = run("filename: a.md\ngoodbye\n", "c1");
    if !aien_omega_compose::LINKED {
        assert!(matches!(resp, ControlResponse::Error(_)));
        return;
    }
    let ControlResponse::ComposeTaskResult(r) = resp else {
        panic!("{resp:?}")
    };
    assert!(!r.committed, "{r:?}");
    assert!(r.proposal.is_none());
    assert!(!ws.join("a.md").exists());
    let ControlResponse::ComposeTaskResult(ok) = run("filename: a.md\nhello\n", "c2") else {
        panic!()
    };
    assert!(ok.committed, "{ok:?}");
    assert_eq!(std::fs::read_to_string(ws.join("a.md")).unwrap(), "hello\n");
}

/// The compose verification boundary that runs without the linked compose
/// library: a reply whose one line lacks the phrase is never approved.
#[test]
fn the_proposal_boundary_approves_only_the_line_with_the_phrase() {
    use aien_runtime::spine::{propose_task_checked, COMPOSE_MAX_ATTEMPTS};
    let reqs = analyze("Create a.md containing one line saying hello.")
        .resolved("")
        .requirements;
    let go = |reply: &'static str| {
        let proposer = move |_: &str, _: Duration| {
            Ok(Generation {
                text: reply.to_string(),
                tokens: 5,
                finish_reason: Some("eos".into()),
                ..Default::default()
            })
        };
        propose_task_checked(
            &proposer,
            "base",
            None,
            &reqs,
            Duration::from_secs(5),
            Duration::from_millis(1),
            COMPOSE_MAX_ATTEMPTS,
        )
        .0
    };
    assert!(go("filename: a.md\ngoodbye\n").is_err());
    assert!(go("filename: a.md\nhello\n").is_ok());
}

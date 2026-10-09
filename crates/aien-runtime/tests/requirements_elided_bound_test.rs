//! #344: a second bound whose noun is left out ("at least 20 lines and no more
//! than 40") is read with the earlier noun, or reported UNCERTAIN. It is never
//! dropped silently.
use aien_runtime::control::ControlResponse;
use aien_runtime::requirements::{analyze, refusal_reason, Requirement};
use aien_runtime::spine::{
    check_file_proposal, propose_task_checked, ComposeBridge, ComposeProposer, Generation,
    COMPOSE_MAX_ATTEMPTS,
};
use std::sync::Arc;
use std::time::Duration;

use Requirement::*;

fn read(goal: &str) -> Vec<Requirement> {
    let ex = analyze(goal);
    assert!(ex.uncertain.is_empty(), "{goal:?}: {:?}", ex.uncertain);
    ex.requirements
}

#[test]
fn elided_max_after_min_is_read_with_the_earlier_noun() {
    let want = [MinLines(20), MaxLines(40)];
    for g in [
        "Write at least 20 lines and no more than 40.",
        "Write at least 20 lines and no more than 40",
        "Write at least 20 lines, and no more than 40.",
        "Write at least 20 lines but at most 40.",
        "Write no fewer than 20 lines and up to 40.",
        "Create docs/a.md with at least 20 lines and at most 40 in total.",
    ] {
        let r = read(g);
        for w in &want {
            assert!(r.contains(w), "{g:?} -> {r:?}");
        }
    }
    let r = read("Write at least 100 words but at most 200.");
    assert!(
        r.contains(&MinWords(100)) && r.contains(&MaxWords(200)),
        "{r:?}"
    );
    // strict cue keeps its adjustment: fewer than 40 is at most 39
    let r = read("Write at least 20 lines and fewer than 40.");
    assert!(r.contains(&MaxLines(39)), "{r:?}");
}

#[test]
fn unsupported_noun_or_direction_is_uncertain_not_silent() {
    // paragraphs have no upper bound in the model
    let ex = analyze("Write at least 3 paragraphs and no more than 5.");
    assert!(!ex.uncertain.is_empty(), "{ex:?}");
    // items: no upper bound either
    let ex = analyze("List at least 3 items and no more than 5.");
    assert!(!ex.uncertain.is_empty(), "{ex:?}");
    // a cue with no earlier noun to carry over, after a read count
    let ex = analyze("Write at least 20 lines and then keep it under, no more than 40.");
    assert!(!ex.uncertain.is_empty(), "{ex:?}");
}

#[test]
fn ordinary_numbers_stay_silent() {
    for g in [
        "Write at least 20 lines and fix 2 typos.",
        "Fix 2 typos in README.md",
        "Update section 3 of docs/a.md",
        "Write at least 20 lines and update section 3.",
    ] {
        let r = analyze(g);
        assert!(r.uncertain.is_empty(), "{g:?}: {:?}", r.uncertain);
        assert!(
            !r.requirements.iter().any(|x| matches!(x, MaxLines(_))),
            "{g:?}: {:?}",
            r.requirements
        );
    }
    assert!(analyze("Fix 2 typos").requirements.is_empty());
    assert!(analyze("Update section 3").requirements.is_empty());
}

fn gen(text: &str) -> Result<Generation, String> {
    Ok(Generation {
        text: text.to_string(),
        tokens: 5,
        finish_reason: Some("eos".into()),
        ..Default::default()
    })
}

fn reply(lines: usize) -> String {
    let mut s = String::from("filename: docs/NOTES.md\n");
    for i in 0..lines {
        s.push_str(&format!("line number {i}\n"));
    }
    s
}

const GOAL: &str = "Create the file docs/NOTES.md with at least 20 lines and no more than 40.";

#[test]
fn bytes_over_the_second_bound_are_refused_and_within_bounds_pass() {
    let reqs = analyze(GOAL).requirements;
    let over = check_file_proposal(&reply(45)).unwrap().content;
    let why = refusal_reason(&reqs, &over).expect("45 lines must be refused");
    assert!(why.contains("40"), "{why}");
    for ok in [20usize, 30, 40] {
        let c = check_file_proposal(&reply(ok)).unwrap().content;
        assert!(refusal_reason(&reqs, &c).is_none(), "{ok} lines");
    }
}

fn bridge(tmp: &std::path::Path, lines: usize) -> ComposeBridge {
    let text = reply(lines);
    let proposer: ComposeProposer = Arc::new(move |_: &str, _: Duration| gen(&text));
    ComposeBridge::new(tmp.join("compose"), proposer, "test:elided")
}

fn report(resp: ControlResponse) -> Option<aien_runtime::control::ComposeTaskReport> {
    if !aien_omega_compose::LINKED {
        assert!(matches!(resp, ControlResponse::Error(_)));
        return None;
    }
    let ControlResponse::ComposeTaskResult(r) = resp else {
        panic!("{resp:?}")
    };
    Some(*r)
}

#[test]
fn linked_compose_does_not_commit_45_lines_but_commits_30() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let r = report(bridge(tmp.path(), 45).run_task(GOAL, ws.to_str().unwrap()));
    if let Some(r) = r {
        assert!(!r.committed, "{r:?}");
        assert!(r.proposal.is_none());
        assert_eq!(r.proposal_attempts.len(), COMPOSE_MAX_ATTEMPTS as usize);
        assert_eq!(std::fs::read_dir(&ws).unwrap().count(), 0);
    }
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    if let Some(r) = report(bridge(tmp.path(), 30).run_task(GOAL, ws.to_str().unwrap())) {
        assert!(r.committed, "{r:?}");
        let p = check_file_proposal(r.proposal.as_deref().unwrap()).unwrap();
        assert_eq!(p.content, check_file_proposal(&reply(30)).unwrap().content);
    }
}

/// The real compose verification boundary (`propose_task_checked`), which does
/// not need the linked omega build: the 45 line reply is refused on every
/// attempt and nothing is approved; the 30 line reply is approved byte for byte.
#[test]
fn compose_boundary_refuses_45_lines_and_approves_30() {
    let ex = analyze(GOAL);
    assert!(ex.uncertain.is_empty());
    let run = |lines: usize| {
        let text = reply(lines);
        let proposer = move |_: &str, _: Duration| gen(&text);
        propose_task_checked(
            &proposer,
            "base",
            None,
            &ex.requirements,
            Duration::from_secs(5),
            Duration::from_millis(1),
            COMPOSE_MAX_ATTEMPTS,
        )
    };
    let (out, attempts) = run(45);
    let why = out.unwrap_err();
    assert!(why.contains("40"), "{why}");
    assert_eq!(attempts.len(), COMPOSE_MAX_ATTEMPTS as usize);
    assert!(attempts
        .iter()
        .all(|a| a.outcome == "refused" && a.aegis.is_none()));
    let (out, attempts) = run(30);
    assert_eq!(out.unwrap(), reply(30));
    assert_eq!(attempts.len(), 1);
}

//! A goal that asks the document to start with a markdown heading
//! (Requirement::FirstLineHeading). The ALLEN end-to-end demo v2 request was
//! refused as uncertain because no recognizer read this wording (arch#162).
//! Recognizing it must not loosen any other uncertain wording.
use aien_runtime::requirements::{analyze, refusal_reason, Requirement};
use aien_runtime::spine::{
    check_file_proposal, propose_task_checked, Generation, COMPOSE_MAX_ATTEMPTS,
};
use std::sync::Mutex;
use std::time::Duration;

use Requirement::*;

/// The S3 request of docs/campaigns/allen-e2e/DEMO-v2.md, verbatim.
const DEMO_V2_GOAL: &str = "Write garden.md in the workspace, a garden plan. Start with a Markdown heading line that begins with \"# \". Use at most 200 words.";

fn gen(text: &str) -> Result<Generation, String> {
    Ok(Generation {
        text: text.to_string(),
        tokens: 5,
        finish_reason: Some("eos".into()),
        ..Default::default()
    })
}

fn run(
    goal: &str,
    replies: Vec<String>,
) -> (
    Result<String, String>,
    Vec<aien_runtime::control::ProposalAttempt>,
    Vec<String>,
) {
    let prompts = Mutex::new(Vec::new());
    let queue = Mutex::new(replies);
    let n = Mutex::new(0usize);
    let proposer = |p: &str, _: Duration| {
        prompts.lock().unwrap().push(p.to_string());
        let q = queue.lock().unwrap();
        let mut i = n.lock().unwrap();
        let r = q[(*i).min(q.len() - 1)].clone();
        *i += 1;
        gen(&r)
    };
    let ex = analyze(goal);
    assert!(ex.uncertain.is_empty(), "{:?}", ex.uncertain);
    let (out, a) = propose_task_checked(
        &proposer,
        "base",
        None,
        &ex.requirements,
        Duration::from_secs(5),
        Duration::from_millis(1),
        COMPOSE_MAX_ATTEMPTS,
    );
    let p = prompts.lock().unwrap().clone();
    (out, a, p)
}

const NO_HEADING: &str =
    "filename: garden.md\nGarden plan\n\nPlant tomatoes in May and water them weekly.\n";
const GOOD: &str =
    "filename: garden.md\n# Garden plan\n\nPlant tomatoes in May and water them weekly.\n";

#[test]
fn demo_v2_goal_is_fully_recognized() {
    let ex = analyze(DEMO_V2_GOAL);
    assert!(ex.uncertain.is_empty(), "{:?}", ex.uncertain);
    assert_eq!(
        ex.requirements,
        [MaxWords(200), FirstLineHeading { level: Some(1) }]
    );
    // The S8 request differs only in the file name.
    let s8 = DEMO_V2_GOAL.replace("garden.md", "garden-b.md");
    assert_eq!(analyze(&s8), ex);
}

#[test]
fn first_heading_phrasings() {
    for (g, want) in [
        (
            "Start with a Markdown heading line that begins with \"# \".",
            Some(1),
        ),
        (
            "start with a markdown heading line that starts with \"## \"",
            Some(2),
        ),
        ("Begin with a heading.", None),
        ("Start with a Markdown heading.", None),
        ("It must start with a level-two heading.", Some(2)),
        ("Open with a markdown heading line.", None),
        (
            "Start with a heading that begins with \u{201C}# \u{201D}.",
            Some(1),
        ),
    ] {
        let ex = analyze(g);
        assert!(ex.uncertain.is_empty(), "{g}: {:?}", ex.uncertain);
        assert_eq!(ex.requirements, [FirstLineHeading { level: want }], "{g}");
    }
}

#[test]
fn near_misses_stay_uncertain() {
    for g in [
        // negated: never inverted
        "Don't start with a Markdown heading.",
        // a marker that is not a heading marker on its own
        "Start with a Markdown heading line that begins with \"#\".",
        // the level adjective and the marker disagree
        "Start with a level-one heading line that begins with \"## \".",
        // seven hashes is not a heading
        "Start with a heading line that begins with \"####### \".",
        // emphasis around the wording
        "Start with a **Markdown heading**.",
        "Start with a **heading**.",
        // another word before "heading"
        "Begin with a short heading.",
        // a qualifier after the heading
        "Start with a Markdown heading about tomatoes.",
        "Start with a Markdown heading line that begins with \"# \" in bold.",
        // no marker after "begins with"
        "Start with a Markdown heading line that begins with a hash.",
        // a sibling constraint in the same sentence is never swallowed (R319 MUST 1)
        "Begin with a markdown heading and keep it brief",
        "Begin with a markdown heading and lowercase only",
        "Begin with a markdown heading and end with a signature",
        "Begin with a markdown heading, then a table",
        "Begin with a markdown heading and a few sections",
        "Open with a level-2 heading line and a short list",
        "Open with a markdown heading line, and keep it short.",
        "Start with a heading line that begins with \"# \" and at most 5 items.",
        "Start with a heading, ####### markdown.",
        // other heading cues the safety net already refused
        "The page needs a Summary heading.",
        "Include headings for setup and use.",
    ] {
        let ex = analyze(g);
        assert!(!ex.uncertain.is_empty(), "{g}: should be uncertain");
        assert!(
            !ex.requirements
                .iter()
                .any(|r| matches!(r, FirstLineHeading { .. })),
            "{g}: {:?}",
            ex.requirements
        );
    }
}

#[test]
fn the_first_line_is_checked() {
    let one = FirstLineHeading { level: Some(1) };
    let any = FirstLineHeading { level: None };
    for (doc, ok1, ok_any) in [
        ("# Garden plan\nbody\n", true, true),
        ("# Garden plan\r\nbody\r\n", true, true),
        ("# Garden plan", true, true),
        ("## Garden plan\n", false, true),
        ("###### Six\n", false, true),
        ("####### Seven\n", false, false),
        ("Garden plan\n# Later\n", false, false),
        ("\n# Garden plan\n", false, false),
        (" # Indented\n", false, false),
        ("#Garden\n", false, false),
        ("# \nbody\n", false, false),
        ("#\n", false, false),
        ("", false, false),
        ("\u{feff}# Garden plan\n", false, false),
    ] {
        assert_eq!(one.check(doc).is_none(), ok1, "level 1 on {doc:?}");
        assert_eq!(any.check(doc).is_none(), ok_any, "any level on {doc:?}");
    }
    let why = one.check("Garden plan\n").unwrap();
    assert!(why.contains("first line"), "{why}");
}

#[test]
fn a_reply_without_the_heading_is_refused_then_a_good_one_passes() {
    let ex = analyze(DEMO_V2_GOAL);
    let content = check_file_proposal(NO_HEADING).unwrap().content;
    let why = refusal_reason(&ex.requirements, &content).unwrap();
    assert!(why.contains("first line"), "{why}");
    let (out, a, prompts) = run(DEMO_V2_GOAL, vec![NO_HEADING.into(), GOOD.into()]);
    assert_eq!(out.unwrap(), GOOD);
    assert_eq!(a[0].outcome, "refused");
    assert_eq!(a[1].outcome, "parsed");
    // the retry prompt names what was missed
    assert!(prompts[1].contains("first line"), "{}", prompts[1]);
}

#[test]
fn a_reply_that_never_has_the_heading_exhausts_with_nothing_approved() {
    let (out, a, _) = run(DEMO_V2_GOAL, vec![NO_HEADING.into()]);
    assert!(out.unwrap_err().contains("first line"));
    assert_eq!(a.len(), COMPOSE_MAX_ATTEMPTS as usize);
    assert!(a
        .iter()
        .all(|x| x.outcome == "refused" && x.aegis.is_none()));
}

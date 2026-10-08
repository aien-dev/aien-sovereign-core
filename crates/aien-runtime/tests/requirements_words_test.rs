//! sc#333: required-word wordings were either refused as uncertain before any
//! model call ("Make sure the words whetstone and angle both appear") or, worse,
//! silently not read ("use the exact words greens and browns", "must name
//! Priya, Tomas and Wen"): the product would approve a document without them.
//! They are now `RequiredWords`; near misses stay UNCERTAIN, never silent. A
//! hyphenated required word ("first-aid") is matched as a whole word.
use aien_runtime::requirements::{analyze, refusal_reason, Requirement};

use Requirement::*;

fn words(w: &[&str]) -> Requirement {
    RequiredWords(w.iter().map(|x| x.to_string()).collect())
}

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
fn the_exact_words_are_read_not_dropped() {
    // tasks-oq3-v4 W1, last sentence, verbatim.
    assert_eq!(
        reqs("Somewhere in the guide use the exact words greens and browns."),
        [words(&["greens", "browns"])]
    );
    // tasks-oq3-v4 W5, last sentence, verbatim.
    let r = reqs("Use sections titled \"Shelter\", \"Food and water\" and \"Safety\", and use the exact words headlamp and first-aid somewhere in the list.");
    assert!(r.contains(&words(&["headlamp", "first-aid"])), "{r:?}");
}

#[test]
fn named_people_are_read_not_dropped() {
    // tasks-oq3-v4 W4, verbatim.
    let w4 = "Create the file docs/WEEK-SUMMARY.md summarising notes/standup-log.txt in at least 14 lines. It needs a section titled \"Highlights\" and a section titled \"Open questions\", and it must name Priya, Tomas and Wen.";
    assert!(reqs(w4).contains(&words(&["Priya", "Tomas", "Wen"])));
    // v5 D4 (sc#339), verbatim.
    let d4 = "There is a file at notes/choir-rehearsal.txt with my rough notes from choir practice. Please create a new file called docs/choir-summary.md that summarises it, and leave the original alone. The summary needs at least 13 lines, two sections titled \"Key Points\" and \"Next Steps\", and must mention Marta, Owen and Priya by name.";
    assert!(reqs(d4).contains(&words(&["Marta", "Owen", "Priya"])));
}

#[test]
fn the_words_that_must_appear_are_read() {
    // v5 D1, last sentence, verbatim.
    assert_eq!(
        reqs("Make sure the words whetstone and angle both appear somewhere in it."),
        [words(&["whetstone", "angle"])]
    );
    // v5 D5, the word clause (the rest of D5 is sc#331 and sc#334).
    assert_eq!(
        reqs("Make me a checklist, and be sure the words passport and charger show up."),
        [words(&["passport", "charger"])]
    );
    for (goal, w) in [
        (
            "Ensure the words red and blue are used.",
            vec!["red", "blue"],
        ),
        (
            "Check that the terms alpha, beta and gamma all appear in the file.",
            vec!["alpha", "beta", "gamma"],
        ),
        ("Make sure the word kettle appears.", vec!["kettle"]),
    ] {
        assert_eq!(reqs(goal), [words(&w)], "{goal}");
    }
}

#[test]
fn a_hyphenated_word_is_matched_whole() {
    let r = [words(&["first-aid", "headlamp"])];
    assert!(refusal_reason(&r, "Pack the First-Aid kit and a headlamp.").is_none());
    for bad in [
        "Pack the first aid kit and a headlamp.",
        "Ask a first-aider; bring a headlamp.",
        "Pack a headlamp.",
    ] {
        let why = refusal_reason(&r, bad).expect("unmet");
        assert!(why.contains("first-aid"), "{bad}: {why}");
    }
}

#[test]
fn near_misses_stay_uncertain() {
    for goal in [
        // negated
        "Make sure the words whetstone and angle never appear.",
        "Make sure the words whetstone and angle do not appear.",
        // a scope the check cannot measure
        "Make sure the words whetstone and angle appear in every section.",
        "Use the exact words greens and browns somewhere in each section.",
        // a count on the words
        "Use the exact words greens and browns twice.",
        // not single words
        "Make sure the words sharp knife and angle appear.",
        // a partly named list
        "It must name Priya, Tomas and the dog.",
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
fn wordings_without_a_word_list_stay_silent() {
    for goal in [
        "Name the file notes.md.",
        "It should mention safety and comfort.",
        "Please name it after the river.",
    ] {
        let ex = analyze(goal);
        assert!(
            ex.uncertain.is_empty() && ex.requirements.is_empty(),
            "{goal:?}: {ex:?}"
        );
    }
}

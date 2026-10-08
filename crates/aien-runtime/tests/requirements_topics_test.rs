//! sc#332: a counted topic list ("Cover three topics: drying, labelling and
//! storage.") was refused as uncertain before any model call, and the topic
//! check refused word forms the goal itself accepts ("dried", "Label",
//! "stored", "withdrew"). The list is now read when the count matches it, the
//! word-form rule agrees on y/i, doubled consonants and -age, and a form the
//! goal ties to one topic ("withdrew ... for the third") is accepted for it.
use aien_runtime::requirements::{analyze, refusal_reason, Requirement};

use Requirement::*;

/// tasks-oq3-v4 W3, verbatim.
const W3: &str = "Please put together docs/SEED-SAVING.md, a short explainer on saving vegetable seeds from your own garden. Make it at least eighteen lines long. Cover three topics: drying, labelling and storage. A topic counts as covered when its word, or another form of it such as dried, labelled or stored, appears in the file.";
/// v5 D3 (sc#339), verbatim.
const D3: &str = "Write an explainer for beginners called docs/savings-basics.md about how a savings account works. It must cover four topics: interest, deposit, withdraw and balance, and the file needs at least 18 lines. A topic counts as covered when its word or another form of that word appears, for example interests or interested for the first one, deposits or deposited for the second, withdrew or withdrawing for the third, and balances or balancing for the last.";

fn reqs(goal: &str) -> Vec<Requirement> {
    let ex = analyze(goal);
    assert!(
        ex.uncertain.is_empty(),
        "{goal:?} should be read, uncertain: {:?}",
        ex.uncertain
    );
    ex.requirements
}

fn topics(r: &[Requirement]) -> Vec<String> {
    r.iter()
        .find_map(|x| match x {
            RequiredTopics(t) => Some(t.clone()),
            _ => None,
        })
        .expect("a topic requirement")
}

#[test]
fn the_counted_topic_lists_are_read() {
    let r = reqs(W3);
    assert!(r.contains(&MinLines(18)), "{r:?}");
    assert_eq!(topics(&r), ["drying", "labelling", "storage"]);
    let r = reqs(D3);
    assert!(r.contains(&MinLines(18)), "{r:?}");
    assert_eq!(
        topics(&r),
        ["interest", "deposit", "withdraw (or withdrew)", "balance"]
    );
}

#[test]
fn the_forms_the_goals_accept_cover_their_topics() {
    // evidence-v5/topic-forms-probe.txt: each sentence uses a form the goal accepts.
    for (topic, text) in [
        (
            "drying",
            "Leave the pods until they have dried on the plant.",
        ),
        ("drying", "Dry the seeds on a plate."),
        ("labelling", "Label each envelope with the variety."),
        ("labelling", "Keep every packet labelled."),
        ("storage", "Store the jars somewhere cool."),
        ("storage", "The seeds are stored in a tin."),
        ("withdraw (or withdrew)", "She withdrew fifty pounds."),
        ("withdraw (or withdrew)", "Withdrawing money is free."),
        ("interest", "The bank pays interest monthly."),
        ("balance", "Check the balances each week."),
        // the same rule elsewhere
        ("manage", "Good management helps."),
        ("running", "Run every day."),
        ("addressing", "Write the address."),
        ("stories", "Tell a story."),
    ] {
        let r = [RequiredTopics(vec![topic.to_string()])];
        assert!(refusal_reason(&r, text).is_none(), "{topic}: {text}");
    }
}

#[test]
fn other_words_still_do_not_cover_a_topic() {
    for (topic, text) in [
        ("drying", "Wet the seeds."),
        ("storage", "Stop here."),
        ("labelling", "Lab work is fun."),
        ("manage", "One man went."),
        ("storage", "A story."),
        // an irregular form counts only when the goal names it
        ("withdraw", "She withdrew fifty pounds."),
        ("withdraw (or withdrew)", "She withdrawal fifty pounds."),
    ] {
        let r = [RequiredTopics(vec![topic.to_string()])];
        assert!(refusal_reason(&r, text).is_some(), "{topic}: {text}");
    }
}

#[test]
fn counted_topic_near_misses_stay_uncertain() {
    for goal in [
        // the count differs from the list
        "Cover three topics: drying and storage.",
        "It must cover two topics: interest, deposit and balance.",
        // negated
        "Do not cover three topics: drying, labelling and storage.",
        // a list item that is not a topic
        "Cover two topics: drying and at least 3 lines.",
    ] {
        let ex = analyze(goal);
        assert!(
            !ex.uncertain.is_empty(),
            "{goal:?} should stay uncertain, got {:?}",
            ex.requirements
        );
    }
}

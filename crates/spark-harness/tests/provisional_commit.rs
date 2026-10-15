//! Headstart M3 (#537): the `Committing` state. A journaled `BeginCommit`
//! freezes a `Committable` task and its inputs; nothing upstream may change
//! them until a `CommitOutcome` is recorded; a replay that finds no outcome
//! marks the task `CommitUncertain` and never runs it again. The authorize
//! stage repeats this from `Committed`. Bound request keys are journaled.

mod common;

use common::{enabled, H};
use spark_harness::execution::TaskState;
use spark_harness::readiness::ProofClaim;
use spark_harness::scheduler::{
    CommitResult, CommitStage, Effects, EventKind, Outcome, SchedError, MAX_APPROVAL_BINDINGS,
    MAX_APPROVAL_KEY_LEN, RESERVED_ID_PREFIX,
};
use std::collections::BTreeMap;

const COMMIT: CommitStage = CommitStage::Commit;
const AUTHORIZE: CommitStage = CommitStage::Authorize;

fn remove_receipt(h: &H, receipt_id: &str) {
    std::fs::remove_file(
        h.engine
            .completion_store
            .join("receipts")
            .join(format!("{receipt_id}.json")),
    )
    .unwrap();
}

fn completion(h: &H, task: &str) -> ProofClaim {
    h.sched.task(task).unwrap().completion.clone().unwrap()
}

/// `BeginCommit` of `stage` built from the task's current accepted state.
fn stage_kind(h: &H, task: &str, stage: CommitStage) -> EventKind {
    let producers: BTreeMap<String, ProofClaim> = h
        .sched
        .task(task)
        .unwrap()
        .bindings
        .keys()
        .map(|p| (p.clone(), completion(h, p)))
        .collect();
    EventKind::BeginCommit {
        task: task.into(),
        generation: h.generation(task),
        stage,
        completion: completion(h, task),
        producers,
    }
}

fn begin_kind(h: &H, task: &str) -> EventKind {
    stage_kind(h, task, COMMIT)
}

fn begin_stage(h: &mut H, task: &str, stage: CommitStage) -> Result<Outcome, SchedError> {
    h.advance(10);
    let kind = stage_kind(h, task, stage);
    h.send(kind)
}

fn begin(h: &mut H, task: &str) -> Result<Outcome, SchedError> {
    begin_stage(h, task, COMMIT)
}

/// The outcome of the stage that is running.
fn outcome(h: &mut H, task: &str, outcome: CommitResult) -> Effects {
    let generation = h.generation(task);
    let stage = h.sched.task(task).unwrap().commit.as_ref().unwrap().stage;
    h.ok(EventKind::CommitOutcome {
        task: task.into(),
        generation,
        stage,
        outcome,
    })
}

fn bind(h: &mut H, key: &str, stage: CommitStage, task: &str) -> Result<Outcome, SchedError> {
    let generation = h.generation(task);
    h.send(EventKind::BindApproval {
        key: key.into(),
        stage,
        task: task.into(),
        generation,
    })
}

fn bound(h: &H, key: &str) -> Option<(CommitStage, String, u64)> {
    h.sched
        .approval(key)
        .map(|b| (b.stage, b.task.clone(), b.generation))
}

fn committed() -> CommitResult {
    CommitResult::Committed {
        detail: "test".into(),
    }
}

fn in_progress(r: &Result<Outcome, SchedError>, committing: &str) -> bool {
    matches!(r, Err(SchedError::CommitInProgress { committing: c, .. }) if c == committing)
}

/// a -> b -> c with a and b final. c is still provisional.
fn chain() -> H {
    let mut h = H::new(enabled());
    h.register("a", 1, &[]);
    h.register("b", 1, &["a"]);
    h.register("c", 1, &["b"]);
    h.publish("a", 1);
    h.publish("b", 1);
    h.finish("a");
    h.finish("b");
    assert_eq!(h.sched.state("b"), Some(TaskState::Committable));
    assert_eq!(h.sched.state("c"), Some(TaskState::Provisional));
    h
}

#[test]
fn begin_commit_freezes_the_inputs_and_the_outcome_ends_it() {
    let mut h = chain();
    let gen_b = h.generation("b");
    assert!(matches!(begin(&mut h, "b"), Ok(Outcome::Applied(_))));
    assert_eq!(h.sched.state("b"), Some(TaskState::Committing));
    let inputs = h.sched.task("b").unwrap().commit.clone().unwrap();
    assert_eq!(inputs.completion, completion(&h, "b"));
    assert_eq!(inputs.producers.get("a"), Some(&completion(&h, "a")));
    // The consumer keeps working against the committing producer.
    assert_eq!(h.sched.state("c"), Some(TaskState::Provisional));
    outcome(&mut h, "b", committed());
    assert_eq!(h.sched.state("b"), Some(TaskState::Committed));
    assert_eq!(h.generation("b"), gen_b);
    // Done: a second begin is refused, as is a second outcome.
    assert!(matches!(
        begin(&mut h, "b"),
        Err(SchedError::WrongState { .. })
    ));
    let r = h.send(EventKind::CommitOutcome {
        task: "b".into(),
        generation: gen_b,
        stage: COMMIT,
        outcome: committed(),
    });
    assert!(matches!(r, Err(SchedError::WrongState { .. })), "{r:?}");
}

#[test]
fn while_committing_upstream_and_own_changes_are_refused() {
    let mut h = chain();
    let gen_a = h.generation("a");
    let gen_b = h.generation("b");
    begin(&mut h, "b").unwrap();
    let refused_on_a = [
        EventKind::Revoke {
            task: "a".into(),
            generation: gen_a,
            reason: "test".into(),
        },
        EventKind::Cancel {
            task: "a".into(),
            generation: gen_a,
        },
        EventKind::NewGeneration {
            task: "a".into(),
            generation: gen_a + 1,
        },
        EventKind::ProducerFailed {
            task: "a".into(),
            generation: gen_a,
            reason: "test".into(),
        },
        EventKind::InterfacePublished {
            task: "a".into(),
            generation: gen_a,
            claim: h.mint_interface("a", 2),
        },
    ];
    for kind in refused_on_a {
        let r = h.send(kind.clone());
        assert!(in_progress(&r, "b"), "{kind:?}: {r:?}");
    }
    let refused_on_b = [
        EventKind::Cancel {
            task: "b".into(),
            generation: gen_b,
        },
        EventKind::NewGeneration {
            task: "b".into(),
            generation: gen_b + 1,
        },
        EventKind::ProducerFailed {
            task: "b".into(),
            generation: gen_b,
            reason: "test".into(),
        },
    ];
    for kind in refused_on_b {
        let r = h.send(kind.clone());
        assert!(in_progress(&r, "b"), "{kind:?}: {r:?}");
    }
    // Revoke needs an active task; Committing is not one.
    let r = h.send(EventKind::Revoke {
        task: "b".into(),
        generation: gen_b,
        reason: "test".into(),
    });
    assert!(matches!(r, Err(SchedError::WrongState { .. })), "{r:?}");
    assert_eq!(h.sched.state("a"), Some(TaskState::Committable));
    assert_eq!(h.sched.state("b"), Some(TaskState::Committing));
    assert_eq!(h.generation("a"), gen_a);
    assert_eq!(h.generation("b"), gen_b);

    // Once the outcome is in, the same revoke is accepted and does not
    // reach the committed task.
    outcome(&mut h, "b", committed());
    let fx = h.ok(EventKind::Revoke {
        task: "a".into(),
        generation: gen_a,
        reason: "test".into(),
    });
    assert!(!fx.revoked.contains(&"b".to_string()), "{fx:?}");
    assert_eq!(h.sched.state("b"), Some(TaskState::Committed));
    assert_eq!(h.sched.state("c"), Some(TaskState::Provisional));
}

#[test]
fn without_a_commit_the_same_revoke_reaches_the_consumer() {
    // Negative control for the test above.
    let mut h = chain();
    let gen_a = h.generation("a");
    let gen_b = h.generation("b");
    h.ok(EventKind::Revoke {
        task: "a".into(),
        generation: gen_a,
        reason: "test".into(),
    });
    assert_eq!(h.sched.state("b"), Some(TaskState::Blocked));
    assert_eq!(h.generation("b"), gen_b + 1);
}

#[test]
fn begin_commit_refuses_inputs_that_differ_from_the_state() {
    let mut h = chain();
    let good = begin_kind(&h, "b");
    let EventKind::BeginCommit {
        generation,
        completion: own,
        producers,
        ..
    } = good.clone()
    else {
        unreachable!()
    };
    let mut other = own.clone();
    other.digest = format!("{}0", other.digest);
    let with =
        |completion: ProofClaim, producers: BTreeMap<String, ProofClaim>| EventKind::BeginCommit {
            task: "b".into(),
            generation,
            stage: COMMIT,
            completion,
            producers,
        };
    let mut missing = producers.clone();
    missing.clear();
    let mut extra = producers.clone();
    extra.insert("c".into(), own.clone());
    let mut wrong_producer = producers.clone();
    wrong_producer.insert("a".into(), other.clone());
    let cases = [
        ("own completion", with(other.clone(), producers.clone())),
        ("no producers", with(own.clone(), missing)),
        ("extra producer", with(own.clone(), extra)),
        ("producer claim", with(own.clone(), wrong_producer)),
        (
            "stale generation",
            EventKind::BeginCommit {
                task: "b".into(),
                generation: generation + 1,
                stage: COMMIT,
                completion: own.clone(),
                producers: producers.clone(),
            },
        ),
    ];
    for (what, kind) in cases {
        let r = h.send(kind);
        assert!(r.is_err(), "{what}: {r:?}");
        assert_eq!(h.sched.state("b"), Some(TaskState::Committable), "{what}");
    }
    // Not committable yet.
    let r = h.send(EventKind::BeginCommit {
        task: "c".into(),
        generation: h.generation("c"),
        stage: COMMIT,
        completion: own.clone(),
        producers: BTreeMap::from([("b".to_string(), completion(&h, "b"))]),
    });
    assert!(matches!(r, Err(SchedError::WrongState { .. })), "{r:?}");
    // A producer receipt that no longer verifies.
    remove_receipt(&h, &completion(&h, "a").receipt_id);
    let r = h.send(good.clone());
    assert!(
        matches!(r, Err(SchedError::Proof { ref task, .. }) if task == "a"),
        "{r:?}"
    );
    assert_eq!(h.sched.state("b"), Some(TaskState::Committable));
}

#[test]
fn begin_commit_accepts_the_matching_inputs() {
    // Positive control for the refusals above.
    let mut h = chain();
    let kind = begin_kind(&h, "b");
    assert!(matches!(h.send(kind), Ok(Outcome::Applied(_))));
}

#[test]
fn only_one_commit_runs_per_dependency_chain() {
    let mut h = chain();
    h.register("x", 1, &[]);
    h.publish("x", 1);
    h.finish("x");
    // c becomes committable too.
    h.publish("c", 1);
    h.finish("c");
    begin(&mut h, "b").unwrap();
    // Below and above b: refused.
    assert!(in_progress(&begin(&mut h, "c"), "b"));
    assert!(in_progress(&begin(&mut h, "a"), "b"));
    // An unrelated task commits at the same time.
    assert!(begin(&mut h, "x").is_ok());
    outcome(&mut h, "b", committed());
    // a is upstream of a committed task only: it may commit now.
    assert!(begin(&mut h, "c").is_ok());
    assert!(begin(&mut h, "a").is_ok());
}

#[test]
fn failed_commit_aborts_retryably_and_revokes_consumers() {
    let mut h = chain();
    let gen_b = h.generation("b");
    let gen_c = h.generation("c");
    begin(&mut h, "b").unwrap();
    outcome(
        &mut h,
        "b",
        CommitResult::Failed {
            reason: "refused".into(),
        },
    );
    assert_eq!(h.sched.state("b"), Some(TaskState::Aborted));
    assert_eq!(h.sched.state("c"), Some(TaskState::Blocked));
    assert_eq!(h.generation("c"), gen_c + 1);
    h.ok(EventKind::Retry {
        task: "b".into(),
        generation: gen_b,
    });
    assert_eq!(h.generation("b"), gen_b + 1);
}

#[test]
fn uncertain_commit_is_terminal_and_revokes_consumers() {
    let mut h = chain();
    let gen_b = h.generation("b");
    let gen_c = h.generation("c");
    begin(&mut h, "b").unwrap();
    outcome(
        &mut h,
        "b",
        CommitResult::Uncertain {
            reason: "transport".into(),
        },
    );
    assert_eq!(h.sched.state("b"), Some(TaskState::CommitUncertain));
    assert_eq!(h.sched.state("c"), Some(TaskState::Blocked));
    assert_eq!(h.generation("c"), gen_c + 1);
    for kind in [
        EventKind::Retry {
            task: "b".into(),
            generation: gen_b,
        },
        EventKind::Cancel {
            task: "b".into(),
            generation: gen_b,
        },
        EventKind::NewGeneration {
            task: "b".into(),
            generation: gen_b + 1,
        },
    ] {
        let r = h.send(kind.clone());
        assert!(r.is_err(), "{kind:?}: {r:?}");
        assert_eq!(h.sched.state("b"), Some(TaskState::CommitUncertain));
    }
    assert!(begin(&mut h, "b").is_err());
}

#[test]
fn replayed_commit_without_an_outcome_becomes_uncertain_and_never_reruns() {
    let mut h = H::journaled(enabled());
    h.register("a", 1, &[]);
    h.register("b", 1, &["a"]);
    h.register("c", 1, &["b"]);
    h.publish("a", 1);
    h.publish("b", 1);
    h.finish("a");
    h.finish("b");
    let gen_c = h.generation("c");
    begin(&mut h, "b").unwrap();
    // Crash before the outcome.
    h.restart(enabled()).unwrap();
    assert_eq!(h.sched.state("b"), Some(TaskState::CommitUncertain));
    assert_eq!(h.sched.state("c"), Some(TaskState::Blocked));
    assert_eq!(h.generation("c"), gen_c + 1);
    let journal = std::fs::read_to_string(h.fx.journal()).unwrap();
    assert!(journal.contains(&format!("{RESERVED_ID_PREFIX}commit-uncertain-")));
    // Neither a new begin nor a late outcome is accepted.
    assert!(begin(&mut h, "b").is_err());
    let r = h.send(EventKind::CommitOutcome {
        task: "b".into(),
        generation: h.generation("b"),
        stage: COMMIT,
        outcome: committed(),
    });
    assert!(matches!(r, Err(SchedError::WrongState { .. })), "{r:?}");
    // A second restart settles nothing new.
    let lines = std::fs::read_to_string(h.fx.journal())
        .unwrap()
        .lines()
        .count();
    h.restart(enabled()).unwrap();
    assert_eq!(h.sched.state("b"), Some(TaskState::CommitUncertain));
    assert_eq!(
        std::fs::read_to_string(h.fx.journal())
            .unwrap()
            .lines()
            .count(),
        lines
    );
}

#[test]
fn replayed_commit_with_an_outcome_keeps_it() {
    // Negative control: a recorded outcome survives the restart unchanged.
    let mut h = H::journaled(enabled());
    h.register("a", 1, &[]);
    h.publish("a", 1);
    h.finish("a");
    begin(&mut h, "a").unwrap();
    outcome(&mut h, "a", committed());
    let snap = h.sched.snapshot();
    h.restart(enabled()).unwrap();
    assert_eq!(h.sched.state("a"), Some(TaskState::Committed));
    assert_eq!(h.sched.snapshot()["tasks"], snap["tasks"]);
}

#[test]
fn audit_defers_a_revocation_that_would_reach_a_running_commit() {
    let mut h = chain();
    begin(&mut h, "b").unwrap();
    remove_receipt(&h, &completion(&h, "a").receipt_id);
    let revoked = h.sched.audit().unwrap();
    assert!(revoked.is_empty(), "{revoked:?}");
    assert_eq!(h.sched.state("a"), Some(TaskState::Committable));
    assert_eq!(h.sched.state("b"), Some(TaskState::Committing));
    // After the outcome the deferred revocation goes through and stops at
    // the committed task.
    outcome(&mut h, "b", committed());
    let revoked = h.sched.audit().unwrap();
    assert!(revoked.contains(&"a".to_string()), "{revoked:?}");
    assert_eq!(h.sched.state("b"), Some(TaskState::Committed));
}

#[test]
fn audit_without_a_commit_revokes_the_whole_chain() {
    // Negative control for the deferral above.
    let mut h = chain();
    remove_receipt(&h, &completion(&h, "a").receipt_id);
    let revoked = h.sched.audit().unwrap();
    assert!(revoked.contains(&"a".to_string()), "{revoked:?}");
    assert_eq!(h.sched.state("b"), Some(TaskState::Blocked));
}

#[test]
fn begin_commit_redelivery_is_a_duplicate() {
    let mut h = chain();
    h.advance(10);
    let kind = begin_kind(&h, "b");
    let event = spark_harness::scheduler::Event {
        id: "begin-b".into(),
        kind,
    };
    assert!(matches!(
        h.sched.deliver(event.clone()),
        Ok(Outcome::Applied(_))
    ));
    let r = h.sched.deliver(event);
    assert!(matches!(r, Ok(Outcome::Duplicate)), "{r:?}");
    assert_eq!(h.sched.state("b"), Some(TaskState::Committing));
}

// ---------- the authorize stage ----------

fn wrong_state(r: &Result<Outcome, SchedError>) -> bool {
    matches!(r, Err(SchedError::WrongState { .. }))
}

#[test]
fn the_authorize_stage_begins_only_after_the_commit_stage_committed() {
    let mut h = chain();
    let gen_a = h.generation("a");
    let gen_b = h.generation("b");
    // Committable: the grant cannot come before the commit.
    assert!(wrong_state(&begin_stage(&mut h, "b", AUTHORIZE)));
    begin(&mut h, "b").unwrap();
    // Nor while the commit runs.
    assert!(wrong_state(&begin_stage(&mut h, "b", AUTHORIZE)));
    outcome(&mut h, "b", committed());
    assert_eq!(h.sched.task("b").unwrap().committed_stage, Some(COMMIT));
    assert!(matches!(
        begin_stage(&mut h, "b", AUTHORIZE),
        Ok(Outcome::Applied(_))
    ));
    assert_eq!(h.sched.state("b"), Some(TaskState::Committing));
    let inputs = h.sched.task("b").unwrap().commit.clone().unwrap();
    assert_eq!(inputs.stage, AUTHORIZE);
    assert_eq!(inputs.producers.get("a"), Some(&completion(&h, "a")));
    // The authorize stage freezes the upstream too.
    let r = h.send(EventKind::Revoke {
        task: "a".into(),
        generation: gen_a,
        reason: "test".into(),
    });
    assert!(in_progress(&r, "b"), "{r:?}");
    outcome(&mut h, "b", committed());
    let t = h.sched.task("b").unwrap();
    assert_eq!((t.state, t.generation), (TaskState::Committed, gen_b));
    assert_eq!(t.committed_stage, Some(AUTHORIZE));
    // Once per generation: a second grant and a second commit are refused.
    assert!(wrong_state(&begin_stage(&mut h, "b", AUTHORIZE)));
    assert!(wrong_state(&begin(&mut h, "b")));
}

#[test]
fn the_authorize_stage_re_verifies_the_producers() {
    let mut h = chain();
    begin(&mut h, "b").unwrap();
    outcome(&mut h, "b", committed());
    remove_receipt(&h, &completion(&h, "a").receipt_id);
    let r = begin_stage(&mut h, "b", AUTHORIZE);
    assert!(
        matches!(r, Err(SchedError::Proof { ref task, .. }) if task == "a"),
        "{r:?}"
    );
    assert_eq!(h.sched.state("b"), Some(TaskState::Committed));
}

#[test]
fn an_outcome_must_name_the_running_stage() {
    let mut h = chain();
    begin(&mut h, "b").unwrap();
    let r = h.send(EventKind::CommitOutcome {
        task: "b".into(),
        generation: h.generation("b"),
        stage: AUTHORIZE,
        outcome: committed(),
    });
    assert!(matches!(r, Err(SchedError::InvalidEvent(_))), "{r:?}");
    assert_eq!(h.sched.state("b"), Some(TaskState::Committing));
}

#[test]
fn a_refused_grant_keeps_the_commit_and_may_begin_again() {
    let mut h = chain();
    let gen_c = h.generation("c");
    begin(&mut h, "b").unwrap();
    outcome(&mut h, "b", committed());
    begin_stage(&mut h, "b", AUTHORIZE).unwrap();
    let fx = outcome(
        &mut h,
        "b",
        CommitResult::Failed {
            reason: "EFFECT_REFUSED DeskMacRequired".into(),
        },
    );
    assert!(fx.revoked.is_empty() && fx.aborted.is_empty(), "{fx:?}");
    let t = h.sched.task("b").unwrap();
    assert_eq!(
        (t.state, t.committed_stage),
        (TaskState::Committed, Some(COMMIT))
    );
    assert_eq!(h.generation("c"), gen_c);
    assert!(begin_stage(&mut h, "b", AUTHORIZE).is_ok());
}

#[test]
fn an_uncertain_grant_is_terminal_and_revokes_consumers() {
    let mut h = chain();
    let gen_c = h.generation("c");
    begin(&mut h, "b").unwrap();
    outcome(&mut h, "b", committed());
    begin_stage(&mut h, "b", AUTHORIZE).unwrap();
    outcome(
        &mut h,
        "b",
        CommitResult::Uncertain {
            reason: "transport".into(),
        },
    );
    assert_eq!(h.sched.state("b"), Some(TaskState::CommitUncertain));
    assert_eq!(h.generation("c"), gen_c + 1);
    assert!(begin_stage(&mut h, "b", AUTHORIZE).is_err());
}

#[test]
fn a_replayed_grant_without_an_outcome_becomes_uncertain_and_never_reruns() {
    let mut h = H::journaled(enabled());
    h.register("a", 1, &[]);
    h.register("b", 1, &["a"]);
    h.publish("a", 1);
    h.finish("a");
    h.finish("b");
    begin(&mut h, "b").unwrap();
    outcome(&mut h, "b", committed());
    begin_stage(&mut h, "b", AUTHORIZE).unwrap();
    // Crash during the grant.
    h.restart(enabled()).unwrap();
    let t = h.sched.task("b").unwrap();
    assert_eq!(t.state, TaskState::CommitUncertain);
    assert_eq!(t.commit.as_ref().unwrap().stage, AUTHORIZE);
    let journal = std::fs::read_to_string(h.fx.journal()).unwrap();
    assert!(journal.contains(&format!("{RESERVED_ID_PREFIX}commit-uncertain-")));
    assert!(begin_stage(&mut h, "b", AUTHORIZE).is_err());
}

// ---------- bound requests ----------

#[test]
fn bindings_survive_a_replay_and_are_dropped_when_their_stage_commits() {
    let mut h = H::journaled(enabled());
    h.register("a", 1, &[]);
    h.register("b", 1, &["a"]);
    let gen_b = h.generation("b");
    bind(&mut h, "approval:x", COMMIT, "b").unwrap();
    bind(&mut h, "proposal:y", AUTHORIZE, "b").unwrap();
    h.restart(enabled()).unwrap();
    assert_eq!(bound(&h, "approval:x"), Some((COMMIT, "b".into(), gen_b)));
    assert_eq!(
        bound(&h, "proposal:y"),
        Some((AUTHORIZE, "b".into(), gen_b))
    );
    h.publish("a", 1);
    h.finish("a");
    h.finish("b");
    begin(&mut h, "b").unwrap();
    // Running: still bound.
    assert!(bound(&h, "approval:x").is_some());
    outcome(&mut h, "b", committed());
    assert_eq!(bound(&h, "approval:x"), None);
    assert!(bound(&h, "proposal:y").is_some());
    h.restart(enabled()).unwrap();
    assert_eq!(bound(&h, "approval:x"), None);
    assert!(bound(&h, "proposal:y").is_some());
    begin_stage(&mut h, "b", AUTHORIZE).unwrap();
    outcome(&mut h, "b", committed());
    assert_eq!(bound(&h, "proposal:y"), None);
    assert_eq!(h.sched.approvals_len(), 0);
}

#[test]
fn bindings_stay_after_a_failed_or_uncertain_commit() {
    let mut h = chain();
    let gen_b = h.generation("b");
    bind(&mut h, "approval:x", COMMIT, "b").unwrap();
    begin(&mut h, "b").unwrap();
    outcome(
        &mut h,
        "b",
        CommitResult::Failed {
            reason: "refused".into(),
        },
    );
    // Kept at the old generation: a request for it is still fenced (and
    // refused as stale) until it is bound again.
    assert_eq!(bound(&h, "approval:x"), Some((COMMIT, "b".into(), gen_b)));

    let mut h = chain();
    bind(&mut h, "approval:x", COMMIT, "b").unwrap();
    begin(&mut h, "b").unwrap();
    outcome(
        &mut h,
        "b",
        CommitResult::Uncertain {
            reason: "transport".into(),
        },
    );
    assert!(bound(&h, "approval:x").is_some());
}

#[test]
fn bind_refuses_bad_keys_stale_generations_and_committed_stages() {
    let mut h = chain();
    let gen_b = h.generation("b");
    for key in [String::new(), "k".repeat(MAX_APPROVAL_KEY_LEN + 1)] {
        let r = bind(&mut h, &key, COMMIT, "b");
        assert!(matches!(r, Err(SchedError::InvalidEvent(_))), "{r:?}");
    }
    assert!(bind(&mut h, &"k".repeat(MAX_APPROVAL_KEY_LEN), COMMIT, "b").is_ok());
    let r = h.send(EventKind::BindApproval {
        key: "approval:old".into(),
        stage: COMMIT,
        task: "b".into(),
        generation: gen_b + 1,
    });
    assert!(
        matches!(r, Err(SchedError::FutureGeneration { .. })),
        "{r:?}"
    );
    begin(&mut h, "b").unwrap();
    outcome(&mut h, "b", committed());
    assert!(wrong_state(&bind(&mut h, "approval:late", COMMIT, "b")));
    assert!(bind(&mut h, "proposal:ok", AUTHORIZE, "b").is_ok());
}

#[test]
fn the_binding_table_is_bounded_and_fails_closed() {
    let mut h = chain();
    for i in 0..MAX_APPROVAL_BINDINGS {
        bind(&mut h, &format!("approval:{i}"), COMMIT, "c").unwrap();
    }
    assert_eq!(h.sched.approvals_len(), MAX_APPROVAL_BINDINGS);
    let r = bind(&mut h, "approval:one-more", COMMIT, "c");
    assert!(matches!(r, Err(SchedError::BindingsFull { .. })), "{r:?}");
    assert_eq!(bound(&h, "approval:one-more"), None);
    // Rebinding a held key replaces it.
    assert!(bind(&mut h, "approval:0", COMMIT, "b").is_ok());
    assert_eq!(h.sched.approvals_len(), MAX_APPROVAL_BINDINGS);
}

//! Headstart M2 (#535): one test per guard that a deletion mutation must
//! break, plus bounded duplicate memory and reserved audit ids.

mod common;

use common::{enabled, H};
use serde_json::json;
use spark_harness::completion::CompletionError;
use spark_harness::execution::TaskState;
use spark_harness::readiness::{
    verify_interface, InterfaceSubmission, OperationClass, ReadinessLevel,
};
use spark_harness::scheduler::{
    Event, EventKind, Outcome, Revocation, SchedError, RESERVED_ID_PREFIX,
};

fn remove_receipt(h: &H, receipt_id: &str) {
    std::fs::remove_file(
        h.engine
            .completion_store
            .join("receipts")
            .join(format!("{receipt_id}.json")),
    )
    .unwrap();
}

#[test]
fn heartbeat_after_lease_expiry_is_refused() {
    let mut h = H::new(enabled());
    h.register("a", 1, &[]);
    h.advance(1_000);
    // No Tick has run yet, so a is still Provisional, but its lease is gone.
    let r = h.send(EventKind::Heartbeat { task: "a".into() });
    assert!(matches!(r, Err(SchedError::LeaseExpired(_))), "{r:?}");
    assert_eq!(h.sched.state("a"), Some(TaskState::Provisional));
    let fx = h.ok(EventKind::Tick);
    assert_eq!(fx.aborted, vec!["a"]);
}

#[test]
fn finalize_rechecks_the_whole_ancestor_chain() {
    let mut h = H::new(enabled());
    h.register("a", 1, &[]);
    h.register("b", 1, &["a"]);
    h.register("c", 1, &["b"]);
    h.publish("a", 1);
    h.publish("b", 1);
    h.work_done("c");
    h.finish("a");
    h.finish("b");
    assert!(h.sched.may_finalize("c").is_ok());
    // Only the grandparent's completion disappears; b's own proof is fine.
    let a = h.sched.task("a").unwrap().completion.clone().unwrap();
    remove_receipt(&h, &a.receipt_id);
    assert!(h.sched.may_finalize("b").is_err());
    let r = h.sched.may_finalize("c");
    assert!(
        matches!(r, Err(SchedError::Proof { ref task, .. }) if task == "a"),
        "{r:?}"
    );
    assert!(h.send(EventKind::Finalize { task: "c".into() }).is_err());
    assert_eq!(h.sched.state("c"), Some(TaskState::AwaitingFinalProof));
}

#[test]
fn proofs_are_accepted_only_for_active_tasks() {
    let mut h = H::new(enabled());
    h.register("p", 1, &[]);
    h.register("b", 1, &["p"]);
    assert_eq!(h.sched.state("b"), Some(TaskState::Blocked));
    // A valid receipt for b exists, but b has not started.
    let claim = h.mint_interface("b", 1);
    let r = h.send(EventKind::InterfacePublished {
        task: "b".into(),
        generation: 1,
        claim,
    });
    assert!(matches!(r, Err(SchedError::WrongState { .. })), "{r:?}");
    assert!(h.sched.task("b").unwrap().interface.is_none());
}

#[test]
fn failed_final_upgrade_falls_back_to_the_interface_level() {
    let mut h = H::new(enabled());
    h.register("a", 1, &[]);
    h.register("b", 1, &["a"]);
    h.publish("a", 1);
    h.finish("a");
    // b was bound at the interface level; a is final now, so it upgrades.
    assert!(h.sched.may_start("b", OperationClass::Commit).is_ok());
    let a = h.sched.task("a").unwrap().completion.clone().unwrap();
    remove_receipt(&h, &a.receipt_id);
    // The interface still verifies: reversible work may go on...
    let r = h
        .sched
        .may_start("b", OperationClass::ReadOnlyAnalysis)
        .unwrap();
    assert_eq!(r[0].level(), ReadinessLevel::InterfaceVerified);
    // ...but nothing irreversible.
    assert!(matches!(
        h.sched.may_start("b", OperationClass::Commit),
        Err(SchedError::NotPermitted { .. })
    ));
}

#[test]
fn reserved_ids_and_audit_events_cannot_be_delivered() {
    let mut h = H::new(enabled());
    h.register("a", 1, &[]);
    let r = h.sched.deliver(Event {
        id: format!("{RESERVED_ID_PREFIX}audit-3"),
        kind: EventKind::Tick,
    });
    assert!(matches!(r, Err(SchedError::InvalidEvent(_))), "{r:?}");
    let r = h.send(EventKind::AuditRevoke {
        revocations: vec![Revocation {
            task: "a".into(),
            reason: "forged".into(),
        }],
    });
    assert!(matches!(r, Err(SchedError::InvalidEvent(_))), "{r:?}");
    assert_eq!(h.sched.state("a"), Some(TaskState::Provisional));
    // A caller id that looks like an old audit id does not block the audit.
    let (claim, _) = h.publish("a", 1);
    h.sched
        .deliver(Event {
            id: "audit-4-a".into(),
            kind: EventKind::Tick,
        })
        .unwrap();
    remove_receipt(&h, &claim.receipt_id);
    assert_eq!(h.sched.audit().unwrap(), vec!["a"]);
}

#[test]
fn duplicate_memory_is_bounded() {
    let mut h = H::new(enabled());
    h.register("a", 1, &[]);
    let (claim, _) = h.publish("a", 1);
    let publish = Event {
        id: "pub-a-1".into(),
        kind: EventKind::InterfacePublished {
            task: "a".into(),
            generation: 1,
            claim,
        },
    };
    // Same claim again under a new id: no change, so it applies cleanly.
    assert!(matches!(
        h.sched.deliver(publish.clone()),
        Ok(Outcome::Applied(_))
    ));
    assert_eq!(h.sched.deliver(publish.clone()), Ok(Outcome::Duplicate));
    let bump = Event {
        id: "gen-a-2".into(),
        kind: EventKind::NewGeneration {
            task: "a".into(),
            generation: 2,
        },
    };
    h.sched.deliver(bump.clone()).unwrap();
    assert_eq!(h.sched.deliver(bump), Ok(Outcome::Duplicate));
    // The generation-1 bucket is gone: the old event is refused as stale,
    // and either way it changes nothing.
    let before = h.sched.snapshot();
    assert!(matches!(
        h.sched.deliver(publish),
        Err(SchedError::StaleGeneration { .. })
    ));
    assert_eq!(h.sched.snapshot(), before);
    // Generation-free events live in a window of `dedup_window` ids.
    for _ in 0..500 {
        h.ok(EventKind::Heartbeat { task: "a".into() });
    }
    let window = enabled().dedup_window;
    assert!(
        h.sched.remembered_event_ids() <= window + 2,
        "{} ids remembered",
        h.sched.remembered_event_ids()
    );
}

#[test]
fn interface_republished_at_a_new_commit_retires_the_old_receipt() {
    let mut h = H::new(enabled());
    h.register("a", 1, &[]);
    let old = h.mint_interface("a", 1);
    // Same generation and interface, new commit: a new receipt replaces it.
    let new = h
        .engine
        .publish_interface(&InterfaceSubmission {
            task_id: "a".into(),
            generation: 1,
            source_commit: "2222222222222222222222222222222222222222".into(),
            interface: json!({ "task": "a", "api_version": 1 }),
        })
        .unwrap()
        .claim();
    assert_eq!(old.digest, new.digest);
    assert_ne!(old.receipt_id, new.receipt_id);
    let store = &h.engine.completion_store;
    assert!(verify_interface(store, "a", 1, &new).is_ok());
    assert!(matches!(
        verify_interface(store, "a", 1, &old),
        Err(CompletionError::NotCurrentCompletion(_))
    ));
}

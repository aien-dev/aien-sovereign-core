//! Headstart M2 (#535): the scheduler journal survives restarts, ignores a
//! torn final line, refuses corruption, and replay never creates a
//! committable task without re-verification.

mod common;

use common::{enabled, H};
use spark_harness::execution::TaskState;
use spark_harness::scheduler::{EventKind, ProvisionalConfig, SchedError, Scheduler};
use std::fs::{self, OpenOptions};
use std::io::Write;

fn scenario(h: &mut H) {
    h.register("a", 1, &[]);
    h.register("b", 1, &["a"]);
    h.register("c", 1, &["b"]);
    h.publish("a", 1);
    h.publish("b", 1);
    h.advance(40);
    h.work_done("c");
    h.publish("a", 2); // interface change: b and c revoked, re-dispatched
    h.publish("b", 1);
    h.finish("a");
}

#[test]
fn replay_reproduces_state_and_trace_exactly() {
    let mut h = H::journaled(enabled());
    scenario(&mut h);
    let snap = h.sched.snapshot();
    let trace = h.sched.timing_trace();
    // Move the clock: replay must use journaled times, not the clock.
    h.advance(500);
    assert_eq!(h.reopen(enabled()).unwrap(), 0);
    assert_eq!(h.sched.snapshot(), snap);
    assert_eq!(h.sched.timing_trace(), trace);
    h.sched.check_invariants().unwrap();
    // The replayed scheduler keeps working and keeps journaling.
    h.work_done("b");
    h.finalize("b");
    let snap2 = h.sched.snapshot();
    h.reopen(enabled()).unwrap();
    assert_eq!(h.sched.snapshot(), snap2);
    assert_eq!(h.sched.state("b"), Some(TaskState::Committable));
}

#[test]
fn torn_last_line_is_ignored_and_truncated() {
    let mut h = H::journaled(enabled());
    scenario(&mut h);
    let snap = h.sched.snapshot();
    let path = h.fx.journal();
    let before = fs::read(&path).unwrap();
    // Simulate a crash mid-append of the next event.
    let torn = br#"{"seq":999,"id":"ev-torn","at":1,"kind":{"type":"fin"#;
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(torn)
        .unwrap();
    assert_eq!(h.reopen(enabled()).unwrap(), torn.len());
    assert_eq!(h.sched.snapshot(), snap);
    assert_eq!(fs::read(&path).unwrap(), before, "torn tail truncated");
    // Next append starts on a clean line and replays.
    h.work_done("b");
    let snap2 = h.sched.snapshot();
    assert_eq!(h.reopen(enabled()).unwrap(), 0);
    assert_eq!(h.sched.snapshot(), snap2);
}

#[test]
fn corrupt_complete_line_fails_closed() {
    let mut h = H::journaled(enabled());
    scenario(&mut h);
    let path = h.fx.journal();
    let text = fs::read_to_string(&path).unwrap();
    let mut lines: Vec<&str> = text.lines().collect();
    lines[3] = "{\"not\":\"an event\"}";
    fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();
    let r = h.reopen(enabled());
    assert!(matches!(r, Err(SchedError::Journal(_))), "{r:?}");
}

#[test]
fn reordered_or_duplicated_lines_fail_replay() {
    let mut h = H::journaled(enabled());
    scenario(&mut h);
    let path = h.fx.journal();
    let text = fs::read_to_string(&path).unwrap();
    let mut lines: Vec<&str> = text.lines().collect();
    let dup = lines[2];
    lines.insert(3, dup);
    fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();
    let r = h.reopen(enabled());
    assert!(matches!(r, Err(SchedError::Journal(_))), "{r:?}");
}

#[test]
fn config_mismatch_is_refused() {
    let mut h = H::journaled(enabled());
    scenario(&mut h);
    let other = ProvisionalConfig {
        enabled: false,
        ..enabled()
    };
    let r = h.reopen(other);
    assert!(matches!(r, Err(SchedError::Journal(_))), "{r:?}");
}

#[test]
fn journal_has_a_single_writer() {
    let h = H::journaled(enabled());
    let second = Scheduler::open(
        enabled(),
        &h.engine.completion_store,
        &h.fx.journal(),
        common::clock(&h.ticks),
    );
    assert!(matches!(second, Err(SchedError::Journal(_))));
}

#[test]
fn replay_never_turns_provisional_work_committable() {
    let mut h = H::journaled(enabled());
    h.register("a", 1, &[]);
    h.register("b", 1, &["a"]);
    h.publish("a", 1);
    h.work_done("b");
    // Crash before a is final: b replays as AwaitingFinalProof, not more.
    h.reopen(enabled()).unwrap();
    assert_eq!(h.sched.state("b"), Some(TaskState::AwaitingFinalProof));
    assert!(h.sched.may_finalize("b").is_err());
    assert!(h.send(EventKind::Finalize { task: "b".into() }).is_err());
    assert!(h.sched.audit().unwrap().is_empty());
}

#[test]
fn replayed_committable_task_is_reverified_against_the_store() {
    let mut h = H::journaled(enabled());
    h.register("a", 1, &[]);
    h.register("b", 1, &["a"]);
    h.publish("a", 1);
    h.work_done("b");
    h.finish("a");
    h.finalize("b");
    assert!(h.sched.may_finalize("b").is_ok());
    let a_completion = h.sched.task("a").unwrap().completion.clone().unwrap();
    // The store loses a's completion receipt while the scheduler is down.
    fs::remove_file(
        h.engine
            .completion_store
            .join("receipts")
            .join(format!("{}.json", a_completion.receipt_id)),
    )
    .unwrap();
    h.reopen(enabled()).unwrap();
    // Replay is pure, so the state comes back as journaled...
    assert_eq!(h.sched.state("b"), Some(TaskState::Committable));
    // ...but the gate re-verifies and refuses, and the audit revokes.
    assert!(matches!(
        h.sched.may_finalize("b"),
        Err(SchedError::Proof { .. })
    ));
    let revoked = h.sched.audit().unwrap();
    assert!(revoked.contains(&"b".to_string()), "{revoked:?}");
    assert_eq!(h.sched.state("b"), Some(TaskState::Blocked));
    // The revocation itself is journaled.
    h.reopen(enabled()).unwrap();
    assert_eq!(h.sched.state("b"), Some(TaskState::Blocked));
}

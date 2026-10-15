//! Headstart M3 (#537): provisional engineering preparation and the final
//! commit fence (crate::provisional). Compiled only with
//! `--features provisional-prep`; without it this binary holds no tests.
//!
//! The M2 scheduler and the M1/M2 harness store are real (spark-harness,
//! shipped workflow and eval definitions). The World commit is the real
//! approved-proposal path; tests that need it to commit run fully only in a
//! linked build (cfg compose_linked) and check the stub refusal otherwise.
//! Timing lines start with `M3-TIMING` (run with `--nocapture`).
#![cfg(feature = "provisional-prep")]

#[path = "support/home_guard.rs"]
mod home_guard;

use aien_omega_compose::hex;
use aien_runtime::approved::{
    ApprovedComposeReport, ApprovedProposal, ApprovedRefusal, ProposerHook,
};
use aien_runtime::approved_auth::{desk_key_path, DeskKey};
use aien_runtime::control::ControlResponse;
use aien_runtime::effects::MintRequest;
use aien_runtime::provisional::{FenceError, PrepAdapter, PrepError, PrepOperation};
use aien_runtime::{ComposeBridge, ComposeProposer};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use spark_harness::completion::CompletionSubmission;
use spark_harness::execution::TaskState;
use spark_harness::readiness::{InterfaceSubmission, OperationClass, ProofClaim, ReadinessLevel};
use spark_harness::scheduler::{
    Clock, Effects, Event, EventKind, Outcome, ProvisionalConfig, ResourceLimits, ResourceRequest,
    SchedError,
};
use spark_harness::{HarnessEngine, WorkflowState};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const COMMIT: &str = "1111111111111111111111111111111111111111";
const PATH: &str = "NOTES.md";
const CONTENT: &str = "approved after the producers were final\n";
const GOAL: &str = "Write NOTES.md: one line about the parser interface";

// ---------- harness store and scheduler ----------

fn fixed_receipt_clock() -> u64 {
    1_760_000_000
}

fn cfg(enabled: bool, max_concurrency: u32) -> ProvisionalConfig {
    ProvisionalConfig {
        enabled,
        queue_capacity: 8,
        limits: ResourceLimits {
            cpu_slots: 8,
            memory_bytes: 1 << 30,
            max_concurrency,
        },
        lease_ttl: 1_000_000,
        dedup_window: 64,
    }
}

fn clock(t: &Arc<AtomicU64>) -> Clock {
    let t = Arc::clone(t);
    Box::new(move || t.load(Ordering::SeqCst))
}

fn copy_definitions(dir: &Path) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../spark-harness");
    std::fs::create_dir_all(dir.join("schemas")).unwrap();
    for f in ["workflows.json", "evals.json"] {
        std::fs::copy(src.join(f), dir.join(f)).unwrap();
    }
    for e in std::fs::read_dir(src.join("schemas")).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), dir.join("schemas").join(e.file_name())).unwrap();
    }
}

fn artifact(task: &str) -> Value {
    json!({
        "bug": format!("Parser rejects valid token sequence in {task}"),
        "reproduction_steps": ["Run parser test"],
        "failure_evidence": "thread panicked at line 42",
        "regression_test": "tests::test_parser",
        "diff_summary": "Fixed token parser logic",
        "retest_evidence": "All tests passing cleanly"
    })
}

/// One adapter, its harness store, a compose home and a workspace, all in
/// one temp dir: `<tmp>/harness`, `<tmp>/compose`, `<tmp>/ws`.
struct H {
    tmp: tempfile::TempDir,
    engine: HarnessEngine,
    ticks: Arc<AtomicU64>,
    adapter: Arc<PrepAdapter>,
    n: u64,
}

impl H {
    fn new(c: ProvisionalConfig) -> Self {
        Self::build(c, false)
    }

    fn journaled(c: ProvisionalConfig) -> Self {
        Self::build(c, true)
    }

    fn build(c: ProvisionalConfig, journaled: bool) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let hd = tmp.path().join("harness");
        copy_definitions(&hd);
        std::fs::create_dir_all(tmp.path().join("ws")).unwrap();
        let mut engine = HarnessEngine::load(&hd).expect("shipped definitions load");
        engine.completion_clock = fixed_receipt_clock;
        let ticks = Arc::new(AtomicU64::new(0));
        let adapter = if journaled {
            let (a, revoked) = PrepAdapter::open(
                c,
                &engine.completion_store,
                &tmp.path().join("journal.jsonl"),
                clock(&ticks),
            )
            .unwrap();
            assert!(revoked.is_empty());
            a
        } else {
            PrepAdapter::new(c, &engine.completion_store, clock(&ticks)).unwrap()
        };
        H {
            tmp,
            engine,
            ticks,
            adapter: Arc::new(adapter),
            n: 0,
        }
    }

    /// A daemon restart: drop the adapter, replay its journal, audit.
    fn restart(&mut self, c: ProvisionalConfig) -> Vec<String> {
        let placeholder =
            PrepAdapter::new(c.clone(), &self.engine.completion_store, clock(&self.ticks)).unwrap();
        drop(std::mem::replace(&mut self.adapter, Arc::new(placeholder)));
        self.ticks.store(0, Ordering::SeqCst);
        let (a, revoked) = PrepAdapter::open(
            c,
            &self.engine.completion_store,
            &self.tmp.path().join("journal.jsonl"),
            clock(&self.ticks),
        )
        .unwrap();
        self.adapter = Arc::new(a);
        revoked
    }

    fn ws(&self) -> PathBuf {
        self.tmp.path().join("ws")
    }

    fn send(&mut self, kind: EventKind) -> Result<Outcome, SchedError> {
        self.n += 1;
        self.ticks.fetch_add(10, Ordering::SeqCst);
        let r = self.adapter.deliver(Event {
            id: format!("ev-{}", self.n),
            kind,
        });
        self.adapter.inspect(|s| s.check_invariants()).unwrap();
        r
    }

    fn ok(&mut self, kind: EventKind) -> Effects {
        match self.send(kind.clone()) {
            Ok(Outcome::Applied(fx)) => fx,
            other => panic!("{kind:?} not applied: {other:?}"),
        }
    }

    fn gen(&self, task: &str) -> u64 {
        self.adapter.generation(task).unwrap()
    }

    fn state(&self, task: &str) -> TaskState {
        self.adapter.state(task).unwrap()
    }

    fn register(&mut self, task: &str, producers: &[&str]) -> Effects {
        self.ok(EventKind::RegisterTask {
            task: task.into(),
            resources: ResourceRequest {
                cpu_slots: 1,
                memory_bytes: 1 << 20,
            },
            producers: producers.iter().map(|p| p.to_string()).collect(),
        })
    }

    fn mint_interface(&self, task: &str, version: u32) -> ProofClaim {
        self.engine
            .publish_interface(&InterfaceSubmission {
                task_id: task.into(),
                generation: self.gen(task),
                source_commit: COMMIT.into(),
                interface: json!({ "task": task, "api_version": version }),
            })
            .unwrap()
            .claim()
    }

    fn publish(&mut self, task: &str, version: u32) -> (ProofClaim, Effects) {
        let claim = self.mint_interface(task, version);
        let generation = self.gen(task);
        let fx = self.ok(EventKind::InterfacePublished {
            task: task.into(),
            generation,
            claim: claim.clone(),
        });
        (claim, fx)
    }

    fn mint_completion(&self, task: &str) -> ProofClaim {
        let mut wf = WorkflowState {
            id: format!("wf-{task}"),
            workflow_type: "engineering_fix".into(),
            current_state: "review".into(),
            history: Vec::new(),
            created_at: "2026-10-15T00:00:00Z".into(),
        };
        let rec = self
            .engine
            .complete_verified_workflow(
                &mut wf,
                &CompletionSubmission {
                    task_id: task.into(),
                    generation: self.gen(task),
                    source_commit: COMMIT.into(),
                    artifact: artifact(task),
                },
            )
            .unwrap();
        ProofClaim {
            receipt_id: rec.receipt_id,
            commit: rec.source_commit,
            digest: rec.artifact_digest,
        }
    }

    fn work_done(&mut self, task: &str) {
        let generation = self.gen(task);
        self.ok(EventKind::WorkDone {
            task: task.into(),
            generation,
            result_digest: format!("result-of-{task}"),
        });
    }

    fn finalize(&mut self, task: &str) {
        let generation = self.gen(task);
        self.ok(EventKind::Finalize {
            task: task.into(),
            generation,
        });
    }

    fn complete(&mut self, task: &str) -> ProofClaim {
        let claim = self.mint_completion(task);
        let generation = self.gen(task);
        self.ok(EventKind::CompletionVerified {
            task: task.into(),
            generation,
            claim: claim.clone(),
        });
        claim
    }

    /// Work done, finalize, M1 completion: the task is final and committable.
    fn finish(&mut self, task: &str) {
        self.work_done(task);
        self.finalize(task);
        self.complete(task);
        assert_eq!(self.state(task), TaskState::Committable);
    }

    fn prepare(
        &self,
        task: &str,
    ) -> Result<aien_runtime::provisional::PreparationResult, PrepError> {
        self.adapter.prepare(task, self.gen(task), GOAL, &self.ws())
    }
}

// ---------- compose home and approvals ----------

fn model_never_called() -> ComposeProposer {
    Arc::new(|_: &str, _: Duration| panic!("the model proposer ran on the approved path"))
}

fn sha(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

fn interplane_sha(path: &str, content: &str) -> String {
    let s = format!(
        "{{\"content\":{},\"path\":{}}}",
        serde_json::to_string(content).unwrap(),
        serde_json::to_string(path).unwrap()
    );
    sha(s.as_bytes())
}

struct Compose {
    bridge: Arc<ComposeBridge>,
    hook: ProposerHook,
    ws: String,
}

impl Compose {
    fn new(h: &H) -> Self {
        DeskKey::create(&desk_key_path(&h.tmp.path().join("compose"))).unwrap();
        Self::reopen(h)
    }

    /// A fresh bridge on the same compose home and desk key (a daemon restart).
    fn reopen(h: &H) -> Self {
        let bridge = Arc::new(ComposeBridge::new(
            h.tmp.path().join("compose"),
            model_never_called(),
            "test:model-never-called",
        ));
        let hook = ProposerHook::new(bridge.clone());
        Compose {
            bridge,
            hook,
            ws: h.ws().display().to_string(),
        }
    }

    /// An approval sealed with this home's desk key.
    fn approval(&self, request: &str) -> ApprovedProposal {
        let mut p = ApprovedProposal {
            request_id: request.into(),
            trace_id: format!("trace-{request}"),
            approval_id: format!("appr-{request}"),
            approver: "interplane-host".into(),
            path: PATH.into(),
            content: CONTENT.into(),
            approved_proposal_sha256: interplane_sha(PATH, CONTENT),
            content_sha256: sha(CONTENT.as_bytes()),
            approval_mac: String::new(),
            requirements: Some(String::new()),
            requirements_mac: String::new(),
            requirements_base: None,
        };
        DeskKey::load(&desk_key_path(self.bridge.dir()))
            .unwrap()
            .seal(&mut p, Path::new(&self.ws));
        p
    }
}

type Submit = Result<ApprovedComposeReport, Box<ApprovedRefusal>>;

/// The canonical path's answer for a valid approval: COMMITTED when linked,
/// the stub refusal otherwise. Returns the report when linked.
fn committed_or_stub(r: Submit) -> Option<ApprovedComposeReport> {
    if !aien_omega_compose::LINKED {
        let e = r.expect_err("stub build must refuse");
        assert!(
            e.refused_by == "REPLAY_REFUSED" || e.name == "ComposeError",
            "{e}"
        );
        return None;
    }
    let r = r.unwrap_or_else(|e| panic!("want COMMITTED, got {e}"));
    assert_eq!(r.state, "COMMITTED");
    let t = r.task.as_ref().expect("a compose run");
    assert!(t.committed && t.cx_promotion != 0, "{t:?}");
    Some(r)
}

/// Every file under `dir` with its sha256, for "nothing was written" checks.
fn tree(dir: &Path) -> BTreeMap<PathBuf, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.insert(p.clone(), sha(&std::fs::read(&p).unwrap_or_default()));
            }
        }
    }
    out
}

fn fence_err<T: std::fmt::Debug>(r: Result<T, FenceError>) -> FenceError {
    match r {
        Err(e) => e,
        Ok(v) => panic!("the fence let a call through: {v:?}"),
    }
}

// ---------- tests ----------

#[test]
fn the_one_whitelisted_operation_is_read_only_analysis() {
    let op = PrepOperation::ComposePlanPreview;
    assert_eq!(op.class(), OperationClass::ReadOnlyAnalysis);
    for c in OperationClass::ALL {
        if c != OperationClass::ReadOnlyAnalysis {
            assert_ne!(op.class(), c);
        }
    }
}

struct Timing {
    critical_ms: f64,
    overlap_ms: f64,
    prep_ms: f64,
    prep_started_before_producer_final: bool,
}

const PRODUCER_WORK: Duration = Duration::from_millis(60);
const PREP_LOAD: Duration = Duration::from_millis(40);

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Producer A (60 ms of simulated work on another thread) and consumer B,
/// whose preparation is the real whitelisted operation plus 40 ms of
/// simulated preparation load (the test hook, outside the adapter mutex).
/// Ends with B's fenced World commit through the approved path.
fn two_stage(enabled: bool) -> Timing {
    let mut h = H::new(cfg(enabled, 4));
    let c = Compose::new(&h);
    h.adapter
        .set_prep_hook(Some(Arc::new(|_: &str| std::thread::sleep(PREP_LOAD))));
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    let t0 = Instant::now();
    let producer = std::thread::spawn(move || {
        std::thread::sleep(PRODUCER_WORK);
        Instant::now()
    });
    let mut prep = None;
    if enabled {
        assert_eq!(h.state("b"), TaskState::Provisional);
        let s = Instant::now();
        let r = h.prepare("b").expect("early preparation");
        assert!(r.is_provisional(), "bound to an interface, not a final");
        prep = Some((s, Instant::now()));
    } else {
        assert_eq!(h.state("b"), TaskState::Blocked);
        assert!(matches!(
            h.prepare("b"),
            Err(PrepError::Scheduler(SchedError::WrongState { .. }))
        ));
    }
    let producer_end = producer.join().unwrap();
    h.finish("a");
    if !enabled {
        assert_eq!(h.state("b"), TaskState::Provisional);
        let s = Instant::now();
        let r = h.prepare("b").expect("sequential preparation");
        assert!(!r.is_provisional());
        prep = Some((s, Instant::now()));
    }
    h.finish("b");
    let _home = home_guard::home_slot();
    let g = h.gen("b");
    let out = h
        .adapter
        .submit_approved("b", g, &c.hook, &c.approval("req-two"), &c.ws)
        .expect("fence passes");
    committed_or_stub(out.outcome);
    let end = Instant::now();
    let (ps, pe) = prep.unwrap();
    let overlap = pe.min(producer_end).saturating_duration_since(ps.max(t0));
    Timing {
        critical_ms: ms(end - t0),
        overlap_ms: ms(overlap),
        prep_ms: ms(pe - ps),
        prep_started_before_producer_final: ps < producer_end,
    }
}

#[test]
fn two_stage_overlap_measured_against_the_sequential_fallback() {
    let on = two_stage(true);
    let off = two_stage(false);
    for (name, t) in [("enabled", &on), ("disabled", &off)] {
        eprintln!(
            "M3-TIMING two_stage {name}: critical_path_ms={:.2} prep_ms={:.2} overlap_ms={:.2} producer_work_ms={} prep_load_ms={} linked={}",
            t.critical_ms,
            t.prep_ms,
            t.overlap_ms,
            PRODUCER_WORK.as_millis(),
            PREP_LOAD.as_millis(),
            aien_omega_compose::LINKED
        );
    }
    assert!(on.prep_started_before_producer_final && on.overlap_ms > 0.0);
    assert!(!off.prep_started_before_producer_final);
    assert_eq!(off.overlap_ms, 0.0, "disabled means no early start");
}

#[test]
fn three_stage_chain_prepares_two_consumers_while_the_root_runs() {
    let _home = home_guard::home_slot();
    let mut h = H::new(cfg(true, 4));
    let c = Compose::new(&h);
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.register("c", &["b"]);
    let (ia, _) = h.publish("a", 1);
    assert_eq!(h.state("b"), TaskState::Provisional);
    let (ib, _) = h.publish("b", 1);
    assert_eq!(h.state("c"), TaskState::Provisional);
    let t0 = Instant::now();
    let rb = h.prepare("b").unwrap();
    let rc = h.prepare("c").unwrap();
    eprintln!(
        "M3-TIMING three_stage: two preparations while the root is not final took {:.3} ms",
        ms(t0.elapsed())
    );
    assert_eq!(h.state("a"), TaskState::Provisional, "root still running");
    // Each result is bound to its producer's interface receipt.
    let b0 = &rb.bindings()[0];
    assert_eq!(
        (b0.producer_task.as_str(), b0.receipt_id.as_str()),
        ("a", ia.receipt_id.as_str())
    );
    assert_eq!(
        (b0.source_commit.as_str(), b0.artifact_digest.as_str()),
        (COMMIT, ia.digest.as_str())
    );
    assert_eq!(b0.producer_generation, 1);
    assert_eq!(b0.level, ReadinessLevel::InterfaceVerified);
    assert_eq!(rc.bindings()[0].receipt_id, ib.receipt_id);
    assert_eq!(rc.operation(), PrepOperation::ComposePlanPreview);
    assert_eq!(rc.class(), OperationClass::ReadOnlyAnalysis);
    assert_eq!(rc.plan_sha256().len(), 64);
    // The fence refuses C until the whole chain is final.
    let gc = h.gen("c");
    let p = c.approval("req-three");
    assert!(matches!(
        fence_err(h.adapter.submit_approved("c", gc, &c.hook, &p, &c.ws)),
        FenceError::NotCommittable { .. }
    ));
    h.finish("a");
    h.finish("b");
    h.work_done("c");
    h.finalize("c");
    // Committable but without its own M1 completion: still refused.
    assert!(matches!(
        fence_err(h.adapter.submit_approved("c", gc, &c.hook, &p, &c.ws)),
        FenceError::NoCompletion(_)
    ));
    h.complete("c");
    let out = h
        .adapter
        .submit_approved("c", gc, &c.hook, &p, &c.ws)
        .expect("fence passes once A, B and C are final");
    assert_eq!(out.proof.producers.len(), 1);
    assert_eq!(out.proof.producers[0].level, ReadinessLevel::FinalVerified);
    assert_eq!(out.proof.completion.level, ReadinessLevel::FinalVerified);
    committed_or_stub(out.outcome);
}

#[test]
fn failed_producer_discards_the_dependent_result() {
    let mut h = H::new(cfg(true, 4));
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    h.prepare("b").unwrap();
    assert!(h.adapter.prepared("b").is_some());
    h.ok(EventKind::ProducerFailed {
        task: "a".into(),
        generation: 1,
        reason: "tests failed".into(),
    });
    assert_eq!((h.state("b"), h.gen("b")), (TaskState::Blocked, 2));
    assert!(h.adapter.prepared("b").is_none(), "result discarded");
    assert!(matches!(
        h.adapter.prepare("b", 1, GOAL, &h.ws()),
        Err(PrepError::StaleGeneration { .. })
    ));
    assert!(matches!(
        fence_err(h.adapter.fenced_for_tests("b", 1, |_| ())),
        FenceError::StaleGeneration { .. }
    ));
}

#[test]
fn completed_producer_upgrades_the_binding_to_final() {
    let mut h = H::new(cfg(true, 4));
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    assert!(h.prepare("b").unwrap().is_provisional());
    h.finish("a");
    let r = h.prepare("b").unwrap();
    assert!(!r.is_provisional());
    assert_eq!(r.bindings()[0].level, ReadinessLevel::FinalVerified);
}

#[test]
fn changed_interface_revokes_and_rebinds_to_the_new_receipt() {
    let mut h = H::new(cfg(true, 4));
    h.register("a", &[]);
    h.register("b", &["a"]);
    let (v1, _) = h.publish("a", 1);
    assert_eq!(
        h.prepare("b").unwrap().bindings()[0].receipt_id,
        v1.receipt_id
    );
    let (v2, fx) = h.publish("a", 2);
    assert_ne!(v1.receipt_id, v2.receipt_id);
    assert!(fx.revoked.contains(&"b".to_string()));
    assert!(h.adapter.prepared("b").is_none());
    assert_eq!((h.state("b"), h.gen("b")), (TaskState::Provisional, 2));
    let r = h.prepare("b").unwrap();
    assert_eq!(
        (r.generation(), r.bindings()[0].receipt_id.as_str()),
        (2, v2.receipt_id.as_str())
    );
}

#[test]
fn new_producer_generation_makes_old_generations_stale() {
    let mut h = H::new(cfg(true, 4));
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    h.prepare("b").unwrap();
    h.ok(EventKind::NewGeneration {
        task: "a".into(),
        generation: 2,
    });
    assert!(h.adapter.prepared("b").is_none());
    assert_eq!(h.gen("b"), 2);
    for g in [1, 3] {
        assert!(matches!(
            h.adapter.prepare("b", g, GOAL, &h.ws()),
            Err(PrepError::StaleGeneration { .. })
        ));
    }
}

#[test]
fn cancellation_discards_results_and_refuses_preparation() {
    let mut h = H::new(cfg(true, 4));
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    h.prepare("b").unwrap();
    h.ok(EventKind::Cancel {
        task: "a".into(),
        generation: 1,
    });
    assert_eq!(h.state("b"), TaskState::Aborted);
    assert!(h.adapter.prepared("b").is_none());
    assert!(matches!(
        h.prepare("b"),
        Err(PrepError::Scheduler(SchedError::WrongState { .. }))
    ));
}

#[test]
fn duplicate_event_is_a_noop_and_keeps_the_result() {
    let mut h = H::new(cfg(true, 4));
    h.register("a", &[]);
    h.register("b", &["a"]);
    let claim = h.mint_interface("a", 1);
    let ev = Event {
        id: "publish-a".into(),
        kind: EventKind::InterfacePublished {
            task: "a".into(),
            generation: 1,
            claim,
        },
    };
    assert!(matches!(
        h.adapter.deliver(ev.clone()),
        Ok(Outcome::Applied(_))
    ));
    let r = h.prepare("b").unwrap();
    assert_eq!(h.adapter.deliver(ev).unwrap(), Outcome::Duplicate);
    assert_eq!(h.adapter.prepared("b"), Some(r));
    assert_eq!(h.gen("b"), 1);
}

#[test]
fn invalid_receipts_are_refused_by_the_event_gate_and_the_fence() {
    let mut h = H::new(cfg(true, 4));
    h.register("a", &[]);
    h.register("b", &["a"]);
    // A claim naming no stored receipt never reaches the scheduler.
    let forged = ProofClaim {
        receipt_id: "ab".repeat(32),
        commit: COMMIT.into(),
        digest: "cd".repeat(32),
    };
    assert!(matches!(
        h.send(EventKind::InterfacePublished {
            task: "a".into(),
            generation: 1,
            claim: forged,
        }),
        Err(SchedError::Proof { .. })
    ));
    assert_eq!(h.state("b"), TaskState::Blocked);
    h.publish("a", 1);
    h.finish("a");
    h.finish("b");
    // Tamper with B's own M1 receipt in the store after it was accepted.
    let rid = h
        .adapter
        .inspect(|s| s.task("b").unwrap().completion.clone().unwrap().receipt_id);
    let file = h
        .engine
        .completion_store
        .join(aien_proof::evidence::RECEIPT_DIR)
        .join(format!("{rid}.json"));
    let original = std::fs::read(&file).unwrap();
    let text = String::from_utf8(original.clone()).unwrap();
    std::fs::write(&file, text.replacen("spark-harness", "spark-harnesz", 1)).unwrap();
    let mut ran = false;
    assert!(matches!(
        fence_err(h.adapter.fenced_for_tests("b", 1, |_| ran = true)),
        FenceError::CompletionInvalid { .. }
    ));
    assert!(!ran);
    std::fs::write(&file, &original).unwrap();
    // Tamper with the producer's receipt instead: M2 may_finalize refuses.
    let ra = h
        .adapter
        .inspect(|s| s.task("a").unwrap().completion.clone().unwrap().receipt_id);
    let fa = file.with_file_name(format!("{ra}.json"));
    let oa = std::fs::read(&fa).unwrap();
    std::fs::write(
        &fa,
        String::from_utf8(oa.clone())
            .unwrap()
            .replacen("spark-harness", "spark-harnesz", 1),
    )
    .unwrap();
    assert!(matches!(
        fence_err(h.adapter.fenced_for_tests("b", 1, |_| ran = true)),
        FenceError::Scheduler(SchedError::Proof { .. })
    ));
    assert!(!ran);
    std::fs::write(&fa, &oa).unwrap();
    h.adapter.fenced_for_tests("b", 1, |_| ran = true).unwrap();
    assert!(ran);
}

#[test]
fn early_fence_alone_commits_nothing_and_writes_nothing() {
    let _home = home_guard::home_slot();
    let mut h = H::new(cfg(true, 4));
    let c = Compose::new(&h);
    let mut ws_file = std::fs::File::create(h.ws().join("parser.md")).unwrap();
    std::io::Write::write_all(&mut ws_file, b"# Parser\n").unwrap();
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    let p = c.approval("req-early");
    let compose_dir = c.bridge.dir().to_path_buf();
    let before = (tree(&h.ws()), tree(&compose_dir));
    // The early gate passes and preparation runs ...
    let r = h.prepare("b").unwrap();
    assert!(r.is_provisional());
    // ... but no commit or grant path is reachable from it.
    let g = h.gen("b");
    {
        assert!(matches!(
            fence_err(h.adapter.submit_approved("b", g, &c.hook, &p, &c.ws)),
            FenceError::NotCommittable {
                state: TaskState::Provisional,
                ..
            }
        ));
        let req = MintRequest {
            cx_promotion: 1,
            proposal_sha256: sha(b"x"),
            workspace: c.ws.clone(),
            approver: "drake".into(),
            constraints: vec![],
        };
        assert!(matches!(
            fence_err(h.adapter.authorize("b", g, &c.bridge, &req, None, None)),
            FenceError::NotCommittable { .. }
        ));
    }
    h.work_done("b");
    assert!(matches!(
        fence_err(
            h.adapter
                .submit_approved("b", h.gen("b"), &c.hook, &p, &c.ws)
        ),
        FenceError::NotCommittable {
            state: TaskState::AwaitingFinalProof,
            ..
        }
    ));
    assert_eq!(
        (tree(&h.ws()), tree(&compose_dir)),
        before,
        "preparation and refused fences wrote nothing"
    );
    assert!(!h.ws().join(PATH).exists());
    // The same approval commits once the chain is final: the fence refused it.
    h.finish("a");
    h.finalize("b");
    h.complete("b");
    let out = h
        .adapter
        .submit_approved("b", h.gen("b"), &c.hook, &p, &c.ws)
        .unwrap();
    committed_or_stub(out.outcome);
    assert!(
        !h.ws().join(PATH).exists(),
        "a World commit is not an effect"
    );
}

#[test]
fn unauthorized_effects_are_still_refused_by_the_canonical_path() {
    let _home = home_guard::home_slot();
    let mut h = H::new(cfg(true, 4));
    let c = Compose::new(&h);
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    h.finish("a");
    h.finish("b");
    // The fence passes; the desk MAC does not.
    let mut p = c.approval("req-forged");
    p.approver = "drake".into();
    let out = h
        .adapter
        .submit_approved("b", 1, &c.hook, &p, &c.ws)
        .expect("fence passes");
    let e = out.outcome.expect_err("unauthenticated approval");
    assert_eq!(
        (e.refused_by.as_str(), e.name.as_str()),
        ("PROPOSAL_REFUSED", "Unauthenticated")
    );
    assert!(!h.ws().join(PATH).exists());
    // ComposeAuthorize through the fence, no desk proof and no compose
    // commit: refused by the existing authorize rules.
    h.register("d", &["a"]);
    h.finish("d");
    let req = MintRequest {
        cx_promotion: 1,
        proposal_sha256: sha(b"never committed"),
        workspace: c.ws.clone(),
        approver: "drake".into(),
        constraints: vec![],
    };
    let out = h
        .adapter
        .authorize("d", 1, &c.bridge, &req, None, None)
        .expect("fence passes");
    match out.outcome {
        ControlResponse::Error(e) => {
            if aien_omega_compose::LINKED {
                assert!(e.contains("DeskMacRequired"), "{e}");
            }
        }
        other => panic!("authorize must refuse: {other:?}"),
    }
}

#[test]
fn duplicate_commit_is_refused_and_a_restart_does_not_run_it_again() {
    let _home = home_guard::home_slot();
    let c0 = cfg(true, 4);
    let mut h = H::journaled(c0.clone());
    let c = Compose::new(&h);
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    h.prepare("b").unwrap();
    h.finish("a");
    h.finish("b");
    let p = c.approval("req-dup");
    let first = h
        .adapter
        .submit_approved("b", 1, &c.hook, &p, &c.ws)
        .unwrap();
    let report = committed_or_stub(first.outcome);
    // Same process, same task generation: refused before the hook runs.
    assert!(matches!(
        fence_err(h.adapter.submit_approved("b", 1, &c.hook, &p, &c.ws)),
        FenceError::AlreadySubmitted { .. }
    ));
    // Restart: the journal replays, the audit passes, no result survives.
    let revoked = h.restart(c0);
    c.bridge.close();
    drop(c);
    let c = Compose::reopen(&h);
    assert!(revoked.is_empty(), "{revoked:?}");
    assert_eq!(h.state("b"), TaskState::Committable);
    assert!(h.adapter.prepared("b").is_none());
    // The new process has no memory of the call; the durable replay ledger
    // answers the retry without a second compose run.
    let again = h
        .adapter
        .submit_approved("b", 1, &c.hook, &p, &c.ws)
        .unwrap();
    if let Some(first) = report {
        let r = again
            .outcome
            .expect("an exact retry gets the original result");
        assert_eq!(r.state, "ALREADY_COMMITTED");
        assert!(r.task.is_none(), "nothing ran again");
        assert_eq!(r.replay_claim, first.replay_claim);
        assert_eq!(r.grant_links, first.grant_links);
    } else {
        assert!(again.outcome.is_err());
    }
}

#[test]
fn restart_with_a_provisional_task_drops_the_result_and_prepares_again() {
    let c0 = cfg(true, 4);
    let mut h = H::journaled(c0.clone());
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    let r = h.prepare("b").unwrap();
    let revoked = h.restart(c0);
    assert!(revoked.is_empty());
    assert_eq!(h.state("b"), TaskState::Provisional);
    assert!(
        h.adapter.prepared("b").is_none(),
        "results are never journaled"
    );
    let again = h.prepare("b").unwrap();
    assert_eq!(again.plan_sha256(), r.plan_sha256());
    // A producer receipt removed from the store while the daemon was down:
    // the audit revokes the consumer at replay.
    let c1 = cfg(true, 4);
    let rid = h
        .adapter
        .inspect(|s| s.task("a").unwrap().interface.clone().unwrap().receipt_id);
    std::fs::remove_file(
        h.engine
            .completion_store
            .join(aien_proof::evidence::RECEIPT_DIR)
            .join(format!("{rid}.json")),
    )
    .unwrap();
    let revoked = h.restart(c1);
    assert!(revoked.contains(&"a".to_string()), "{revoked:?}");
    assert_eq!(h.state("b"), TaskState::Blocked);
}

#[test]
fn revocation_during_preparation_discards_the_result() {
    let mut h = H::new(cfg(true, 4));
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    let adapter = Arc::clone(&h.adapter);
    h.adapter.set_prep_hook(Some(Arc::new(move |_: &str| {
        adapter
            .deliver(Event {
                id: "fail-a-mid-prep".into(),
                kind: EventKind::ProducerFailed {
                    task: "a".into(),
                    generation: 1,
                    reason: "failed while b prepared".into(),
                },
            })
            .unwrap();
    })));
    assert!(matches!(h.prepare("b"), Err(PrepError::Revoked { .. })));
    assert!(h.adapter.prepared("b").is_none());
    assert_eq!(h.state("b"), TaskState::Blocked);
}

#[test]
fn an_interface_change_during_preparation_refuses_the_old_generation() {
    let mut h = H::new(cfg(true, 4));
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    let store_dir = h.tmp.path().join("harness");
    let adapter = Arc::clone(&h.adapter);
    // While b's operation runs, a publishes a new interface (store write,
    // then the M2 event).
    h.adapter.set_prep_hook(Some(Arc::new(move |_: &str| {
        let mut e = HarnessEngine::load(&store_dir).unwrap();
        e.completion_clock = fixed_receipt_clock;
        let v2 = e
            .publish_interface(&InterfaceSubmission {
                task_id: "a".into(),
                generation: 1,
                source_commit: COMMIT.into(),
                interface: json!({ "task": "a", "api_version": 2 }),
            })
            .unwrap()
            .claim();
        adapter
            .deliver(Event {
                id: "republish-a-mid-prep".into(),
                kind: EventKind::InterfacePublished {
                    task: "a".into(),
                    generation: 1,
                    claim: v2,
                },
            })
            .unwrap();
    })));
    // b is revoked and dispatched again at generation 2 before the operation
    // ends, so M2 would admit it again: only the generation recheck refuses
    // the result made against the old interface.
    assert!(matches!(h.prepare("b"), Err(PrepError::Revoked { .. })));
    assert_eq!((h.state("b"), h.gen("b")), (TaskState::Provisional, 2));
    assert!(h.adapter.prepared("b").is_none());
    h.adapter.set_prep_hook(None);
    assert_eq!(h.prepare("b").unwrap().generation(), 2);
}

#[test]
fn a_producer_receipt_that_stops_verifying_during_preparation_refuses_the_result() {
    let mut h = H::new(cfg(true, 4));
    h.register("a", &[]);
    h.register("b", &["a"]);
    let (v1, _) = h.publish("a", 1);
    let gen_b = h.gen("b");
    let file = h
        .engine
        .completion_store
        .join(aien_proof::evidence::RECEIPT_DIR)
        .join(format!("{}.json", v1.receipt_id));
    let original = std::fs::read(&file).unwrap();
    let (f2, o2) = (file.clone(), original.clone());
    // No event: the producer's receipt is altered in the store while b's
    // operation runs, so only the post-operation M2 check can see it.
    h.adapter.set_prep_hook(Some(Arc::new(move |_: &str| {
        let text = String::from_utf8(o2.clone()).unwrap();
        std::fs::write(&f2, text.replacen("spark-harness", "spark-harnesz", 1)).unwrap();
    })));
    assert!(matches!(h.prepare("b"), Err(PrepError::Revoked { .. })));
    assert_eq!((h.state("b"), h.gen("b")), (TaskState::Provisional, gen_b));
    assert!(h.adapter.prepared("b").is_none());
    h.adapter.set_prep_hook(None);
    std::fs::write(&file, &original).unwrap();
    h.prepare("b").unwrap();
}

#[test]
fn a_producer_completing_during_preparation_is_recorded_as_final() {
    let mut h = H::new(cfg(true, 4));
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    h.work_done("a");
    h.finalize("a");
    let claim = h.mint_completion("a");
    let gen_b = h.gen("b");
    let adapter = Arc::clone(&h.adapter);
    h.adapter.set_prep_hook(Some(Arc::new(move |_: &str| {
        adapter
            .deliver(Event {
                id: "complete-a-mid-prep".into(),
                kind: EventKind::CompletionVerified {
                    task: "a".into(),
                    generation: 1,
                    claim: claim.clone(),
                },
            })
            .unwrap();
    })));
    // Admitted on the interface, the producer went final while the
    // operation ran. Nothing was revoked, so the consumer keeps its
    // generation, and the stored result carries the readiness re-checked
    // after the operation (final), never the admission-time one.
    let r = h.prepare("b").unwrap();
    assert_eq!((h.state("b"), h.gen("b")), (TaskState::Provisional, gen_b));
    assert!(!r.is_provisional());
    assert_eq!(r.bindings()[0].level, ReadinessLevel::FinalVerified);
    let stored = h.adapter.prepared("b").unwrap();
    assert_eq!(stored.bindings()[0].level, ReadinessLevel::FinalVerified);
}

#[test]
fn one_preparation_per_task_at_a_time() {
    let mut h = H::new(cfg(true, 4));
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = std::sync::Mutex::new(release_rx);
    let calls = AtomicU64::new(0);
    // Only the first call parks; a second admitted call would run straight
    // through and fail the Busy assertion below instead of deadlocking.
    h.adapter.set_prep_hook(Some(Arc::new(move |_: &str| {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            entered_tx.send(()).unwrap();
            let _ = release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10));
        }
    })));
    let (a2, ws) = (Arc::clone(&h.adapter), h.ws());
    let first = std::thread::spawn(move || a2.prepare("b", 1, GOAL, &ws).map(|_| ()));
    entered_rx.recv().unwrap();
    assert!(matches!(h.prepare("b"), Err(PrepError::Busy(_))));
    release_tx.send(()).unwrap();
    first.join().unwrap().unwrap();
    h.adapter.set_prep_hook(None);
    h.prepare("b").unwrap();
}

#[test]
fn concurrency_is_bounded_by_the_m2_permits() {
    let mut h = H::new(cfg(true, 1));
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.register("c", &["a"]);
    h.publish("a", 1);
    // A holds the only permit: neither consumer may prepare yet.
    assert_eq!(h.adapter.permits_in_use().running, 1);
    for t in ["b", "c"] {
        assert_eq!(h.state(t), TaskState::Blocked);
        assert!(h.prepare(t).is_err());
    }
    h.work_done("a");
    // One consumer at a time, in task-id order.
    assert_eq!(
        (h.state("b"), h.state("c")),
        (TaskState::Provisional, TaskState::Blocked)
    );
    h.prepare("b").unwrap();
    assert!(h.prepare("c").is_err());
    h.work_done("b");
    assert_eq!(h.state("c"), TaskState::Provisional);
    h.prepare("c").unwrap();
    assert!(h.adapter.permits_in_use().running <= 1);
}

#[test]
fn the_commit_decision_holds_out_revocations_and_store_writers() {
    let mut h = H::new(cfg(true, 4));
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    h.finish("a");
    h.finish("b");
    let (done_tx, done_rx) = mpsc::channel::<&'static str>();
    let adapter = Arc::clone(&h.adapter);
    let store_dir = h.tmp.path().join("harness");
    let fenced = h.adapter.fenced_for_tests("b", 1, |_| {
        let tx = done_tx.clone();
        let revoker = std::thread::spawn(move || {
            let r = adapter.deliver(Event {
                id: "revoke-a-during-commit".into(),
                kind: EventKind::Revoke {
                    task: "a".into(),
                    generation: 1,
                    reason: "interface withdrawn".into(),
                },
            });
            tx.send("revoke").unwrap();
            r
        });
        let tx = done_tx.clone();
        let writer = std::thread::spawn(move || {
            let mut e = HarnessEngine::load(&store_dir).unwrap();
            e.completion_clock = fixed_receipt_clock;
            let r = e.publish_interface(&InterfaceSubmission {
                task_id: "a".into(),
                generation: 1,
                source_commit: COMMIT.into(),
                interface: json!({ "task": "a", "api_version": 9 }),
            });
            tx.send("store").unwrap();
            r.map(|_| ())
        });
        std::thread::sleep(Duration::from_millis(150));
        let during: Vec<_> = done_rx.try_iter().collect();
        (during, revoker, writer)
    });
    let out = fenced.expect("fence passes");
    let (during, revoker, writer) = out.outcome;
    assert!(
        during.is_empty(),
        "{during:?} ran inside the commit decision"
    );
    revoker.join().unwrap().unwrap();
    writer.join().unwrap().unwrap();
    // After the decision the revocation applies: the old generation is stale.
    assert_eq!(h.gen("b"), 2);
    assert!(matches!(
        fence_err(h.adapter.fenced_for_tests("b", 1, |_| ())),
        FenceError::AlreadySubmitted { .. } | FenceError::StaleGeneration { .. }
    ));
    assert!(matches!(
        fence_err(h.adapter.fenced_for_tests("b", 2, |_| ())),
        FenceError::NotCommittable { .. }
    ));
}

#[test]
fn disabled_flag_is_the_default_and_sequential() {
    assert!(!ProvisionalConfig::default().enabled);
    let mut h = H::new(cfg(false, 4));
    assert!(!h.adapter.enabled());
    h.register("a", &[]);
    h.register("b", &["a"]);
    h.publish("a", 1);
    assert_eq!(h.state("b"), TaskState::Blocked);
    assert!(h.prepare("b").is_err());
    h.finish("a");
    let r = h.prepare("b").unwrap();
    assert!(!r.is_provisional());
}

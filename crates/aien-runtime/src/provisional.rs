//! Provisional engineering preparation in front of the canonical compose
//! path (Headstart M3, issue #537). Compiled only with cargo feature
//! `provisional-prep` (default off); the default build has no trace of it and
//! the control ABI (`crate::control`) is the same in both builds: no command,
//! field or response is added, and the daemon (`server.rs`) does not call
//! this module.
//!
//! Two halves:
//!
//! 1. Early preparation. [`PrepAdapter::prepare`] runs exactly one whitelisted
//!    operation, [`PrepOperation::ComposePlanPreview`], for a task the M2
//!    scheduler (`spark_harness::scheduler`) has dispatched as `Provisional`.
//!    It maps to [`OperationClass::ReadOnlyAnalysis`], so the M2 gate
//!    `may_start` allows it on an interface-verified producer. The operation
//!    is `compose::rederive::task_decision`: it reads the workspace to build
//!    the compose prompt and destination for a goal and never writes
//!    (bridge.rs `gather_inputs` doc: "Never writes"). No compose home is
//!    opened, no model runs, no tool is called, nothing goes to the network.
//!    The only output is an in-memory [`PreparationResult`] bound to each
//!    producer's source commit, generation, artifact digest and the receipt
//!    id the store verified.
//!
//! 2. Final commit fence. [`PrepAdapter::submit_approved`] (World commit
//!    through `approved::ProposerHook::submit`) and
//!    [`PrepAdapter::authorize`] (`effects::authorize_with_objective`, the
//!    `ComposeAuthorize` path) run the existing call only when, for the
//!    task's current generation: the M2 task is `Committable`, M2
//!    `may_finalize` passes (every producer final-verified, recursively), and
//!    the task's own M1 completion receipt verifies (`verify_final`, which is
//!    M1 `verify_completion`). The fence is a prerequisite only. Desk MAC,
//!    AEGIS verify, the replay ledger, the effect ledger and the World commit
//!    stay inside the unchanged calls and still decide.
//!
//! No check-to-commit window: the fence checks and the existing call run in
//! one critical section that holds (a) the adapter mutex, the only way to
//! deliver a scheduler event, so no revocation, cancellation or new
//! generation can land in between, and (b) the harness store's interface and
//! completion locks (`<store>/interfaces/.lock`, `<store>/completions/.lock`,
//! the same aien-proof flock files `publish_interface` and
//! `complete_verified_workflow` take), so no harness writer in any process
//! can replace a receipt or record in between either.
//!
//! Duplicates: a second fenced call for the same (task, generation) is
//! refused before the existing call runs ([`FenceError::AlreadySubmitted`]).
//! That memory is per process. Across restarts the existing durable guards
//! decide: the approved replay ledger (`approved_replay::claim`) refuses a
//! claimed approval for ever and answers an exact retry with
//! `ALREADY_COMMITTED` without a second run; `ComposeAuthorize` refuses a
//! used desk nonce and a live or spent grant (effects.rs `AlreadyAuthorized`,
//! `AlreadySpent`). A model `RunComposeTask` run has no such durable guard,
//! so this adapter does not offer it.
//!
//! Invalidation: after every delivered event, preparation results whose task
//! is no longer active at the generation they were made for are dropped. M2
//! returns consumers of a failed, cancelled, restarted or changed producer to
//! `Blocked` at a new generation (or `Aborted`), so their results go. A
//! result is also refused, not stored, when the task moved while the
//! operation ran. Results are never journaled: a restart discards them all.
//!
//! Resources: preparation runs on the caller's thread without the adapter
//! mutex, the compose home lock or the runtime spine lock. Only a
//! `Provisional` task with a live lease may prepare, and at most one
//! preparation per task is in flight, so concurrent preparations are bounded
//! by the M2 permits (`ResourceLimits::max_concurrency`). With
//! `ProvisionalConfig::enabled` false (the default) M2 dispatches a consumer
//! only after every producer is final, which is the sequential order.
//!
//! Not in scope: aien-trace is not used (observation is optional); no new
//! receipt type; spark-harness, AEGIS, World, KV and inference scheduling are
//! unchanged.

use crate::approved::{ApprovedComposeReport, ApprovedProposal, ApprovedRefusal, ProposerHook};
use crate::compose::ComposeBridge;
use crate::control::{ControlResponse, DeskProof};
use crate::effects::MintRequest;
use aien_proof::lock::FileLock;
use parking_lot::Mutex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use spark_harness::completion::CONSUMED_DIR;
use spark_harness::execution::TaskState;
use spark_harness::readiness::{
    verify_final, ArtifactReadiness, OperationClass, ReadinessLevel, INTERFACE_DIR,
};
use spark_harness::scheduler::{
    Clock, Event, Outcome, PermitPool, ProvisionalConfig, SchedError, Scheduler,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

/// The whitelist of early preparation operations. One entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrepOperation {
    /// `compose::rederive::task_decision(goal, workspace)`: the compose
    /// prompt and destination a goal would get. Reads the workspace only.
    ComposePlanPreview,
}

impl PrepOperation {
    /// The M2 operation class the scheduler gates this operation as.
    pub fn class(self) -> OperationClass {
        match self {
            PrepOperation::ComposePlanPreview => OperationClass::ReadOnlyAnalysis,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PrepOperation::ComposePlanPreview => "compose_plan_preview",
        }
    }
}

/// One producer artifact a result or a fence decision rests on, copied from
/// a store-verified [`ArtifactReadiness`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadinessBinding {
    pub producer_task: String,
    pub source_commit: String,
    pub producer_generation: u64,
    pub artifact_digest: String,
    pub receipt_id: String,
    pub level: ReadinessLevel,
}

impl From<&ArtifactReadiness> for ReadinessBinding {
    fn from(r: &ArtifactReadiness) -> Self {
        ReadinessBinding {
            producer_task: r.producer_task().to_string(),
            source_commit: r.source_commit().to_string(),
            producer_generation: r.producer_generation(),
            artifact_digest: r.artifact_digest().to_string(),
            receipt_id: r.receipt_id().to_string(),
            level: r.level(),
        }
    }
}

/// A provisional preparation result. Built only by [`PrepAdapter::prepare`];
/// no public constructor and no `Deserialize`. It authorizes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreparationResult {
    task: String,
    generation: u64,
    operation: PrepOperation,
    class: OperationClass,
    plan_sha256: String,
    destination: Option<String>,
    bindings: Vec<ReadinessBinding>,
}

impl PreparationResult {
    pub fn task(&self) -> &str {
        &self.task
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn operation(&self) -> PrepOperation {
        self.operation
    }
    pub fn class(&self) -> OperationClass {
        self.class
    }
    /// sha256 hex of the compose prompt the goal would get.
    pub fn plan_sha256(&self) -> &str {
        &self.plan_sha256
    }
    pub fn destination(&self) -> Option<&str> {
        self.destination.as_deref()
    }
    pub fn bindings(&self) -> &[ReadinessBinding] {
        &self.bindings
    }
    /// True when at least one producer was only interface-verified.
    pub fn is_provisional(&self) -> bool {
        self.bindings
            .iter()
            .any(|b| b.level != ReadinessLevel::FinalVerified)
    }
}

/// What the fence verified, inside the same critical section as the call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FenceProof {
    pub task: String,
    pub generation: u64,
    /// The task's own M1 completion.
    pub completion: ReadinessBinding,
    /// Every producer, final-verified (M2 `may_finalize`).
    pub producers: Vec<ReadinessBinding>,
}

/// The existing call's own answer, plus the fence proof it ran under. The
/// answer may still be a refusal: the fence does not decide authorization.
#[derive(Debug)]
pub struct Fenced<T> {
    pub proof: FenceProof,
    pub outcome: T,
}

#[derive(Debug)]
pub enum PrepError {
    Scheduler(SchedError),
    /// The caller's generation is not the task's current one.
    StaleGeneration {
        task: String,
        expected: u64,
        current: Option<u64>,
    },
    /// A preparation for this task is already running.
    Busy(String),
    /// The task moved (revoked, cancelled, re-dispatched) while the
    /// operation ran; the result was discarded.
    Revoked {
        task: String,
        detail: String,
    },
    /// The operation itself refused (for example a workspace escape).
    Operation(String),
}

impl fmt::Display for PrepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrepError::Scheduler(e) => write!(f, "PREP_REFUSED Scheduler: {e}"),
            PrepError::StaleGeneration {
                task,
                expected,
                current,
            } => write!(
                f,
                "PREP_REFUSED StaleGeneration: task '{task}' asked at generation {expected}, scheduler has {current:?}"
            ),
            PrepError::Busy(t) => write!(f, "PREP_REFUSED Busy: task '{t}' is already preparing"),
            PrepError::Revoked { task, detail } => {
                write!(f, "PREP_REFUSED Revoked: task '{task}': {detail}; result discarded")
            }
            PrepError::Operation(e) => write!(f, "PREP_REFUSED Operation: {e}"),
        }
    }
}

impl std::error::Error for PrepError {}

#[derive(Debug)]
pub enum FenceError {
    /// M2 refused (`may_finalize`, unknown task).
    Scheduler(SchedError),
    StaleGeneration {
        task: String,
        expected: u64,
        current: Option<u64>,
    },
    NotCommittable {
        task: String,
        state: TaskState,
    },
    /// The task has no accepted M1 completion for its current generation.
    NoCompletion(String),
    /// The task's own M1 completion receipt no longer verifies.
    CompletionInvalid {
        task: String,
        error: String,
    },
    /// This process already ran a fenced call for this task generation.
    AlreadySubmitted {
        task: String,
        generation: u64,
    },
    StoreLock(String),
}

impl fmt::Display for FenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FenceError::Scheduler(e) => write!(f, "FENCE_REFUSED Scheduler: {e}"),
            FenceError::StaleGeneration {
                task,
                expected,
                current,
            } => write!(
                f,
                "FENCE_REFUSED StaleGeneration: task '{task}' asked at generation {expected}, scheduler has {current:?}"
            ),
            FenceError::NotCommittable { task, state } => write!(
                f,
                "FENCE_REFUSED NotCommittable: task '{task}' is {}",
                state.as_str()
            ),
            FenceError::NoCompletion(t) => {
                write!(f, "FENCE_REFUSED NoCompletion: task '{t}' has no accepted M1 completion")
            }
            FenceError::CompletionInvalid { task, error } => {
                write!(f, "FENCE_REFUSED CompletionInvalid: task '{task}': {error}")
            }
            FenceError::AlreadySubmitted { task, generation } => write!(
                f,
                "FENCE_REFUSED AlreadySubmitted: task '{task}' generation {generation} already ran its fenced call"
            ),
            FenceError::StoreLock(e) => write!(f, "FENCE_REFUSED StoreLock: {e}"),
        }
    }
}

impl std::error::Error for FenceError {}

#[cfg(feature = "test-support")]
type PrepHook = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

struct Inner {
    sched: Scheduler,
    prepared: BTreeMap<String, PreparationResult>,
    in_flight: BTreeSet<String>,
    submitted: BTreeSet<(String, u64)>,
}

impl Inner {
    fn current_generation(&self, task: &str, expected: u64) -> Result<(), Option<u64>> {
        match self.sched.generation(task) {
            Some(g) if g == expected => Ok(()),
            other => Err(other),
        }
    }

    /// Drop every result whose task is no longer active at its generation.
    fn prune(&mut self) {
        let sched = &self.sched;
        self.prepared.retain(|task, r| {
            sched
                .task(task)
                .is_some_and(|t| t.state.is_active() && t.generation == r.generation)
        });
    }
}

/// The adapter: owns the M2 scheduler for engineering tasks and fences the
/// existing compose commit calls. Share it behind an `Arc`; every method
/// takes `&self`.
pub struct PrepAdapter {
    inner: Mutex<Inner>,
    store: PathBuf,
    #[cfg(feature = "test-support")]
    prep_hook: Mutex<Option<PrepHook>>,
}

impl PrepAdapter {
    fn with_scheduler(sched: Scheduler, store: &Path) -> Self {
        PrepAdapter {
            inner: Mutex::new(Inner {
                sched,
                prepared: BTreeMap::new(),
                in_flight: BTreeSet::new(),
                submitted: BTreeSet::new(),
            }),
            store: store.to_path_buf(),
            #[cfg(feature = "test-support")]
            prep_hook: Mutex::new(None),
        }
    }

    /// In-memory scheduler over the harness store `store`
    /// (`HarnessEngine::completion_store`).
    pub fn new(cfg: ProvisionalConfig, store: &Path, clock: Clock) -> Result<Self, SchedError> {
        Ok(Self::with_scheduler(
            Scheduler::new(cfg, store, clock)?,
            store,
        ))
    }

    /// Journaled scheduler: create or replay `journal`, then run the M2
    /// audit before anything can use it. Returns the tasks the audit revoked.
    /// No preparation result survives: they are never journaled.
    pub fn open(
        cfg: ProvisionalConfig,
        store: &Path,
        journal: &Path,
        clock: Clock,
    ) -> Result<(Self, Vec<String>), SchedError> {
        let (unaudited, _report) = Scheduler::open(cfg, store, journal, clock)?;
        let (sched, revoked) = unaudited.audit()?;
        Ok((Self::with_scheduler(sched, store), revoked))
    }

    /// The runtime enable flag: M2 `ProvisionalConfig::enabled` (default
    /// false, which is sequential dispatch).
    pub fn enabled(&self) -> bool {
        self.inner.lock().sched.config().enabled
    }

    /// Deliver one M2 event, then drop the preparation results it
    /// invalidated.
    pub fn deliver(&self, event: Event) -> Result<Outcome, SchedError> {
        let mut g = self.inner.lock();
        let r = g.sched.deliver(event);
        g.prune();
        r
    }

    /// Read-only access to the scheduler (state, generations, invariants,
    /// timing trace). Holds the adapter mutex while `f` runs.
    pub fn inspect<T>(&self, f: impl FnOnce(&Scheduler) -> T) -> T {
        f(&self.inner.lock().sched)
    }

    pub fn state(&self, task: &str) -> Option<TaskState> {
        self.inner.lock().sched.state(task)
    }

    pub fn generation(&self, task: &str) -> Option<u64> {
        self.inner.lock().sched.generation(task)
    }

    pub fn permits_in_use(&self) -> PermitPool {
        self.inner.lock().sched.permits_in_use()
    }

    /// The task's current preparation result, if it is still valid.
    pub fn prepared(&self, task: &str) -> Option<PreparationResult> {
        self.inner.lock().prepared.get(task).cloned()
    }

    /// Test seam: called between the admission check and the operation,
    /// outside the adapter mutex.
    #[cfg(feature = "test-support")]
    pub fn set_prep_hook(&self, hook: Option<PrepHook>) {
        *self.prep_hook.lock() = hook;
    }

    /// Run the whitelisted operation for `task` at `generation`. Admission:
    /// current generation, no preparation in flight for the task, and M2
    /// `may_start(task, ReadOnlyAnalysis)` (a `Provisional` task with a live
    /// lease, every producer bound at its current generation and
    /// store-verified at a level that permits the class). The operation runs
    /// outside the mutex; the result is stored only if the task is still at
    /// the same generation and `may_start` passes again, both checked under
    /// the mutex that stores it.
    pub fn prepare(
        &self,
        task: &str,
        generation: u64,
        goal: &str,
        workspace: &Path,
    ) -> Result<PreparationResult, PrepError> {
        let op = PrepOperation::ComposePlanPreview;
        {
            let mut g = self.inner.lock();
            g.current_generation(task, generation).map_err(|current| {
                PrepError::StaleGeneration {
                    task: task.to_string(),
                    expected: generation,
                    current,
                }
            })?;
            if g.in_flight.contains(task) {
                return Err(PrepError::Busy(task.to_string()));
            }
            g.sched
                .may_start(task, op.class())
                .map_err(PrepError::Scheduler)?;
            g.in_flight.insert(task.to_string());
        }
        #[cfg(feature = "test-support")]
        {
            let hook = self.prep_hook.lock().clone();
            if let Some(h) = hook {
                h(task);
            }
        }
        let out = plan_preview(goal, workspace);
        let mut g = self.inner.lock();
        g.in_flight.remove(task);
        let (plan_sha256, destination) = out.map_err(PrepError::Operation)?;
        let revoked = |detail: String| PrepError::Revoked {
            task: task.to_string(),
            detail,
        };
        g.current_generation(task, generation).map_err(|current| {
            revoked(format!(
                "generation moved from {generation} to {current:?} during preparation"
            ))
        })?;
        // No separate binding comparison: M2 sets a task's bindings only when
        // it dispatches it, and leaving `Provisional` always moves the task to
        // a new generation first, so an unchanged generation means unchanged
        // bindings. The readiness recorded below is the fresh one.
        let readiness = g
            .sched
            .may_start(task, op.class())
            .map_err(|e| revoked(e.to_string()))?;
        let result = PreparationResult {
            task: task.to_string(),
            generation,
            operation: op,
            class: op.class(),
            plan_sha256,
            destination,
            bindings: readiness.iter().map(ReadinessBinding::from).collect(),
        };
        g.prepared.insert(task.to_string(), result.clone());
        Ok(result)
    }

    /// The harness store locks every interface and completion writer takes.
    fn store_locks(&self) -> Result<[FileLock; 2], FenceError> {
        let lock = |dir: &str| {
            let d = self.store.join(dir);
            std::fs::create_dir_all(&d)
                .and_then(|_| FileLock::acquire(&d.join(".lock")))
                .map_err(|e| FenceError::StoreLock(format!("{}: {e}", d.display())))
        };
        Ok([lock(INTERFACE_DIR)?, lock(CONSUMED_DIR)?])
    }

    /// The fence. Every check and `commit` run under the adapter mutex and
    /// the store locks (see the module docs).
    fn fenced<T>(
        &self,
        task: &str,
        generation: u64,
        commit: impl FnOnce(&FenceProof) -> T,
    ) -> Result<Fenced<T>, FenceError> {
        let mut g = self.inner.lock();
        let _store = self.store_locks()?;
        let key = (task.to_string(), generation);
        if g.submitted.contains(&key) {
            return Err(FenceError::AlreadySubmitted {
                task: task.to_string(),
                generation,
            });
        }
        g.current_generation(task, generation)
            .map_err(|current| FenceError::StaleGeneration {
                task: task.to_string(),
                expected: generation,
                current,
            })?;
        let entry = g
            .sched
            .task(task)
            .ok_or_else(|| FenceError::Scheduler(SchedError::UnknownTask(task.to_string())))?;
        if entry.state != TaskState::Committable {
            return Err(FenceError::NotCommittable {
                task: task.to_string(),
                state: entry.state,
            });
        }
        let claim = entry
            .completion
            .clone()
            .ok_or_else(|| FenceError::NoCompletion(task.to_string()))?;
        let producers = g.sched.may_finalize(task).map_err(FenceError::Scheduler)?;
        let own = verify_final(&self.store, task, generation, &claim).map_err(|e| {
            FenceError::CompletionInvalid {
                task: task.to_string(),
                error: e.to_string(),
            }
        })?;
        let proof = FenceProof {
            task: task.to_string(),
            generation,
            completion: ReadinessBinding::from(&own),
            producers: producers.iter().map(ReadinessBinding::from).collect(),
        };
        // Spent before the call: a call that fails or panics is not retried
        // at this generation (fail closed).
        g.submitted.insert(key);
        let outcome = commit(&proof);
        Ok(Fenced { proof, outcome })
    }

    /// Fenced `ComposeApprovedProposal`: the existing proposer hook (verify,
    /// desk MAC, replay claim, compose run with AEGIS verify and World
    /// commit, grant) runs unchanged once the fence passes.
    pub fn submit_approved(
        &self,
        task: &str,
        generation: u64,
        hook: &ProposerHook,
        proposal: &ApprovedProposal,
        workspace: &str,
    ) -> Result<Fenced<Result<ApprovedComposeReport, Box<ApprovedRefusal>>>, FenceError> {
        self.fenced(task, generation, |_| hook.submit(proposal, workspace))
    }

    /// Fenced `ComposeAuthorize`: the existing grant minting (desk MAC,
    /// commit record match, confinement, nonce and grant replay rules) runs
    /// unchanged once the fence passes.
    pub fn authorize(
        &self,
        task: &str,
        generation: u64,
        bridge: &ComposeBridge,
        req: &MintRequest,
        desk_proof: Option<&DeskProof>,
        objective_id: Option<&str>,
    ) -> Result<Fenced<ControlResponse>, FenceError> {
        self.fenced(task, generation, |_| {
            crate::effects::authorize_with_objective(bridge, req, desk_proof, objective_id)
        })
    }

    /// Test seam: the fence around an arbitrary call, so a test can act
    /// while the critical section is held.
    #[cfg(feature = "test-support")]
    pub fn fenced_for_tests<T>(
        &self,
        task: &str,
        generation: u64,
        commit: impl FnOnce(&FenceProof) -> T,
    ) -> Result<Fenced<T>, FenceError> {
        self.fenced(task, generation, commit)
    }
}

/// The whitelisted operation: compose prompt digest and destination.
fn plan_preview(goal: &str, workspace: &Path) -> Result<(String, Option<String>), String> {
    let ((prompt, _edit, _kind), destination) =
        crate::compose::rederive::task_decision(goal, workspace)?;
    let digest = Sha256::digest(prompt.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    Ok((hex, destination))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whitelist_is_one_reversible_preparation_class() {
        let op = PrepOperation::ComposePlanPreview;
        assert_eq!(op.class(), OperationClass::ReadOnlyAnalysis);
        assert!(op.class().is_reversible_preparation());
        assert!(ReadinessLevel::InterfaceVerified.permits(op.class()));
        assert_eq!(op.as_str(), "compose_plan_preview");
    }
}

//! Provisional dependency scheduler (Headstart M2, issue #535).
//!
//! Lets a consumer task start reversible, sandboxed preparation as soon as
//! its producers publish a harness-verified interface, while final
//! completion waits for every producer's M1 verified completion. This is an
//! engineering-workflow scheduler only; it does not touch inference
//! scheduling, KV/prefill fences, aien-trace or World commit.
//!
//! Opt-in: [`ProvisionalConfig::enabled`] defaults to `false`. Disabled means
//! no early start: a consumer stays `Blocked` until every producer is
//! final-verified and `Committable`, which is plain sequential behavior. No
//! existing spark-harness API or MCP tool uses this module.
//!
//! Model:
//! - Tasks and producer -> consumer edges form a cycle-checked DAG
//!   (`dependency.rs`). Task states and the transition table live in
//!   `execution.rs`.
//! - Everything is an [`Event`] with a caller-chosen id. Redelivering the
//!   same id with the same payload is a no-op ([`Outcome::Duplicate`]); the
//!   same id with a different payload is refused.
//! - [`Scheduler::deliver`] runs the store checks for the event, applies it
//!   to a copy of the state, journals it, then swaps the copy in. A refused
//!   event changes nothing and is not journaled.
//! - Producers have monotonic generations. Events for an older generation are
//!   refused as stale. A producer failure, a changed interface or completion
//!   at the same generation, a new generation, a lost lease or a revocation
//!   returns every active descendant to `Blocked` at a new generation and
//!   discards its provisional result.
//! - A bounded ready queue feeds dispatch. Dispatch takes CPU, memory and
//!   concurrency permits all at once and grants a lease; only `Provisional`
//!   tasks hold permits.
//!
//! Persistence choice: an append-only JSON-lines journal (`journal.rs`) of
//! accepted events, opened with [`Scheduler::open`]. Replay re-applies the
//! events through the same pure state machine, using the times recorded in
//! the journal, so it rebuilds the same state (and the same timing trace).
//! Replay does not re-read the store; instead every `may_start` and
//! `may_finalize` re-verifies from the store at call time, and
//! [`Scheduler::audit`] (call it after `open`) revokes any active task whose
//! proofs no longer verify. A task becomes `Committable` only through a
//! journaled `Finalize` event, which is journaled only after `may_finalize`
//! passed, so replay cannot turn an unfinished provisional task into a
//! committable one. The journal is trusted like the store itself
//! (filesystem ownership); it is not signed.

pub mod core;
pub mod error;
mod gates;
pub mod permits;
pub mod timing;

pub use self::core::{Binding, BindingLevel, Effects, Event, EventKind, JournalEntry, TaskEntry};
pub use self::error::SchedError;
pub use self::permits::{PermitPool, ResourceLimits, ResourceRequest};

use self::core::{payload_digest, Core};
use crate::completion::check_identifier;
use crate::execution::TaskState;
use crate::journal::Journal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Monotonic clock in ticks. Injected so tests are deterministic.
pub type Clock = Box<dyn Fn() -> u64 + Send + Sync>;

/// Nanoseconds since this clock was created, from `std::time::Instant`.
/// A new process starts a new base; the scheduler never lets event time go
/// below the last journaled time, so time stays monotonic across restarts
/// but the gap while the process was down is not counted.
pub fn monotonic_clock() -> Clock {
    let start = std::time::Instant::now();
    Box::new(move || u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX))
}

pub const JOURNAL_FORMAT: &str = "spark-harness-provisional-journal";
pub const TRACE_SCHEMA: &str = "spark-harness-provisional-trace";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvisionalConfig {
    /// Allow early start on interface-verified producers. Default `false`.
    pub enabled: bool,
    /// Maximum ready-queue length (at least 1).
    pub queue_capacity: usize,
    pub limits: ResourceLimits,
    /// Lease length in clock ticks (at least 1).
    pub lease_ttl: u64,
}

impl Default for ProvisionalConfig {
    fn default() -> Self {
        ProvisionalConfig {
            enabled: false,
            queue_capacity: 64,
            limits: ResourceLimits::default(),
            lease_ttl: 300_000_000_000,
        }
    }
}

impl ProvisionalConfig {
    fn validate(&self) -> Result<(), SchedError> {
        if self.queue_capacity == 0 || self.lease_ttl == 0 || self.limits.max_concurrency == 0 {
            return Err(SchedError::InvalidEvent(
                "config needs queue_capacity, lease_ttl and max_concurrency of at least 1"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Outcome {
    Applied(Effects),
    /// The event id was already applied with the same payload.
    Duplicate,
}

/// What [`Scheduler::open`] found in the journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayReport {
    pub entries: usize,
    pub torn_bytes: usize,
}

pub struct Scheduler {
    cfg: ProvisionalConfig,
    store: PathBuf,
    clock: Clock,
    core: Core,
    journal: Option<Journal>,
}

impl Scheduler {
    /// In-memory scheduler (no journal). `store` is the harness store that
    /// holds interface and completion receipts (`HarnessEngine::completion_store`).
    pub fn new(cfg: ProvisionalConfig, store: &Path, clock: Clock) -> Result<Self, SchedError> {
        cfg.validate()?;
        Ok(Scheduler {
            cfg,
            store: store.to_path_buf(),
            clock,
            core: Core::default(),
            journal: None,
        })
    }

    /// Journaled scheduler: create `journal` or replay it. The journal's
    /// header must carry the same config, because dispatch depends on it.
    pub fn open(
        cfg: ProvisionalConfig,
        store: &Path,
        journal: &Path,
        clock: Clock,
    ) -> Result<(Self, ReplayReport), SchedError> {
        let mut s = Scheduler::new(cfg, store, clock)?;
        let header = json!({ "format": JOURNAL_FORMAT, "version": 1, "config": s.cfg });
        let opened =
            Journal::open::<JournalEntry>(journal, &header).map_err(SchedError::Journal)?;
        for entry in &opened.entries {
            s.core.apply(&s.cfg, entry).map_err(|e| {
                SchedError::Journal(format!("replay of entry {} failed: {e}", entry.seq))
            })?;
        }
        let report = ReplayReport {
            entries: opened.entries.len(),
            torn_bytes: opened.torn_bytes,
        };
        s.journal = Some(opened.journal);
        Ok((s, report))
    }

    pub fn config(&self) -> &ProvisionalConfig {
        &self.cfg
    }

    fn now(&self) -> u64 {
        (self.clock)().max(self.core.last_at)
    }

    /// Validate, journal and apply one event. See the module docs.
    pub fn deliver(&mut self, event: Event) -> Result<Outcome, SchedError> {
        check_identifier("event id", &event.id)
            .map_err(|e| SchedError::InvalidEvent(e.to_string()))?;
        if let Some(prev) = self.core.seen.get(&event.id) {
            return if *prev == payload_digest(&event.kind) {
                Ok(Outcome::Duplicate)
            } else {
                Err(SchedError::EventIdConflict(event.id))
            };
        }
        self.verify_proofs(&event.kind)?;
        let entry = JournalEntry {
            seq: self.core.seq + 1,
            id: event.id,
            at: self.now(),
            kind: event.kind,
        };
        let mut next = self.core.clone();
        let fx = next.apply(&self.cfg, &entry)?;
        if let Some(j) = self.journal.as_mut() {
            j.append(&entry).map_err(SchedError::Journal)?;
        }
        self.core = next;
        Ok(Outcome::Applied(fx))
    }

    pub fn task(&self, id: &str) -> Option<&TaskEntry> {
        self.core.tasks.get(id)
    }

    pub fn state(&self, id: &str) -> Option<TaskState> {
        self.task(id).map(|t| t.state)
    }

    pub fn generation(&self, id: &str) -> Option<u64> {
        self.task(id).map(|t| t.generation)
    }

    pub fn queue(&self) -> Vec<String> {
        self.core.queue.iter().cloned().collect()
    }

    pub fn permits_in_use(&self) -> PermitPool {
        self.core.pool
    }

    /// Full scheduler state as JSON, for replay comparisons and debugging.
    pub fn snapshot(&self) -> Value {
        serde_json::to_value(&self.core).unwrap_or(Value::Null)
    }

    /// Structural invariants. Tests call this after every step.
    pub fn check_invariants(&self) -> Result<(), String> {
        let mut pool = PermitPool::default();
        for (id, t) in &self.core.tasks {
            if t.state.holds_permits() {
                pool.running += 1;
                pool.cpu_in_use += t.resources.cpu_slots;
                pool.memory_in_use += t.resources.memory_bytes;
                if self.core.graph.producers_of(id).count() != t.bindings.len() {
                    return Err(format!("running task '{id}' lacks a producer binding"));
                }
            } else if t.lease_expires_at.is_some() {
                return Err(format!(
                    "task '{id}' is {} but holds a lease",
                    t.state.as_str()
                ));
            }
            let in_queue = self.core.queue.iter().any(|q| q == id);
            if t.queued != in_queue || (in_queue && t.state != TaskState::Blocked) {
                return Err(format!("task '{id}' queue flag disagrees with the queue"));
            }
        }
        if pool != self.core.pool {
            return Err(format!(
                "permits in use {:?} differ from running tasks {:?}",
                self.core.pool, pool
            ));
        }
        if self.core.queue.len() > self.cfg.queue_capacity {
            return Err("ready queue exceeds its bound".to_string());
        }
        Ok(())
    }

    /// Per-task blocked and provisional time as of the last event, for the
    /// M6 A/B analysis. Derived only from journaled event times.
    pub fn timing_trace(&self) -> Value {
        let now = self.core.last_at;
        let tasks: Vec<Value> = self
            .core
            .tasks
            .iter()
            .map(|(id, t)| {
                let timing = t.timing.as_of(t.state, now);
                json!({
                    "task": id,
                    "state": t.state.as_str(),
                    "generation": t.generation,
                    "blocked": timing.blocked,
                    "provisional": timing.provisional,
                    "awaiting_final_proof": timing.awaiting_final_proof,
                    "registered_at": timing.registered_at,
                    "first_dispatch_at": timing.first_dispatch_at,
                    "committable_at": timing.committable_at,
                    "dispatches": timing.dispatches,
                    "revocations": timing.revocations,
                })
            })
            .collect();
        json!({
            "schema": TRACE_SCHEMA,
            "version": 1,
            "enabled": self.cfg.enabled,
            "clock": "injected monotonic clock ticks",
            "as_of": now,
            "tasks": tasks,
        })
    }
}

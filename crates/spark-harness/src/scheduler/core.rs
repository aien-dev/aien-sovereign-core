//! Pure scheduler state and event application.
//!
//! [`Core::apply`] is deterministic: it reads only the current state, the
//! config and the journal entry (including the entry's recorded time). It
//! never touches the harness store or the clock. That is what lets a journal
//! replay rebuild the exact same state. Store checks happen before an event
//! is journaled (`gates.rs`) and again at every `may_start` / `may_finalize`.

use super::error::SchedError;
use super::permits::{PermitPool, ResourceRequest};
use super::timing::TaskTiming;
use super::ProvisionalConfig;
use crate::completion::{artifact_digest, check_identifier};
use crate::dependency::DependencyGraph;
use crate::execution::{check_transition, TaskState};
use crate::readiness::ProofClaim;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// Everything the scheduler can be told. Producer proof events carry only a
/// [`ProofClaim`] pointer; the receipt itself is re-read from the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventKind {
    /// Register a task with its producers. A task with no producers is
    /// eligible at once, so dependencies are declared here, atomically.
    RegisterTask {
        task: String,
        resources: ResourceRequest,
        producers: Vec<String>,
    },
    /// Add a producer to a task that is still `Blocked`.
    AddDependency {
        producer: String,
        consumer: String,
    },
    InterfacePublished {
        task: String,
        generation: u64,
        claim: ProofClaim,
    },
    CompletionVerified {
        task: String,
        generation: u64,
        claim: ProofClaim,
    },
    /// The producer's body or interface changed and work restarts at a new,
    /// higher generation.
    NewGeneration {
        task: String,
        generation: u64,
    },
    ProducerFailed {
        task: String,
        generation: u64,
        reason: String,
    },
    /// The task's provisional work finished. `result_digest` names the
    /// provisional result, which is discarded on revocation.
    WorkDone {
        task: String,
        generation: u64,
        result_digest: String,
    },
    Finalize {
        task: String,
    },
    Heartbeat {
        task: String,
    },
    Cancel {
        task: String,
    },
    Retry {
        task: String,
    },
    Revoke {
        task: String,
        reason: String,
    },
    /// Lets time pass: expires leases.
    Tick,
}

/// One delivered event. Duplicate delivery is detected by `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub id: String,
    pub kind: EventKind,
}

/// One journal line: the accepted event plus its sequence number and the
/// monotonic time it was applied at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalEntry {
    pub seq: u64,
    pub id: String,
    pub at: u64,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum BindingLevel {
    Interface,
    Final,
}

/// Which producer artifact a consumer was dispatched against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Binding {
    pub generation: u64,
    pub level: BindingLevel,
    pub claim: ProofClaim,
}

/// Scheduler view of one task. Returned by shared reference only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskEntry {
    pub state: TaskState,
    /// This task's own producer generation. Starts at 1, only increases.
    pub generation: u64,
    pub resources: ResourceRequest,
    /// Accepted interface claim for the current generation.
    pub interface: Option<ProofClaim>,
    /// Accepted M1 completion claim for the current generation.
    pub completion: Option<ProofClaim>,
    pub bindings: BTreeMap<String, Binding>,
    pub lease_expires_at: Option<u64>,
    pub provisional_result: Option<String>,
    pub queued: bool,
    pub last_reason: Option<String>,
    pub timing: TaskTiming,
}

impl TaskEntry {
    fn new(resources: ResourceRequest, at: u64) -> Self {
        TaskEntry {
            state: TaskState::Blocked,
            generation: 1,
            resources,
            interface: None,
            completion: None,
            bindings: BTreeMap::new(),
            lease_expires_at: None,
            provisional_result: None,
            queued: false,
            last_reason: None,
            timing: TaskTiming::new(at),
        }
    }

    pub(crate) fn final_ready(&self) -> bool {
        self.state == TaskState::Committable && self.completion.is_some()
    }

    pub(crate) fn interface_ready(&self) -> bool {
        self.state.is_active() && self.interface.is_some()
    }

    fn lease_valid(&self, at: u64) -> bool {
        self.lease_expires_at.is_some_and(|exp| at < exp)
    }
}

/// What applying an event did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Effects {
    pub dispatched: Vec<String>,
    pub revoked: Vec<String>,
    pub aborted: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Core {
    pub(crate) graph: DependencyGraph,
    pub(crate) tasks: BTreeMap<String, TaskEntry>,
    pub(crate) queue: VecDeque<String>,
    pub(crate) pool: PermitPool,
    /// event id -> digest of its payload, for idempotent redelivery.
    pub(crate) seen: BTreeMap<String, String>,
    pub(crate) seq: u64,
    pub(crate) last_at: u64,
}

pub(crate) fn payload_digest(kind: &EventKind) -> String {
    artifact_digest(&serde_json::to_value(kind).unwrap_or_default())
}

fn bad(m: impl Into<String>) -> SchedError {
    SchedError::InvalidEvent(m.into())
}

impl Core {
    pub(crate) fn task(&self, id: &str) -> Result<&TaskEntry, SchedError> {
        self.tasks
            .get(id)
            .ok_or_else(|| SchedError::UnknownTask(id.to_string()))
    }

    fn task_mut(&mut self, id: &str) -> Result<&mut TaskEntry, SchedError> {
        self.tasks
            .get_mut(id)
            .ok_or_else(|| SchedError::UnknownTask(id.to_string()))
    }

    pub(crate) fn require_generation(&self, id: &str, generation: u64) -> Result<(), SchedError> {
        let current = self.task(id)?.generation;
        let (task, submitted) = (id.to_string(), generation);
        match generation.cmp(&current) {
            std::cmp::Ordering::Less => Err(SchedError::StaleGeneration {
                task,
                submitted,
                current,
            }),
            std::cmp::Ordering::Greater => Err(SchedError::FutureGeneration {
                task,
                submitted,
                current,
            }),
            std::cmp::Ordering::Equal => Ok(()),
        }
    }

    fn require_state(
        &self,
        id: &str,
        ok: impl Fn(TaskState) -> bool,
        action: &'static str,
    ) -> Result<(), SchedError> {
        let state = self.task(id)?.state;
        if ok(state) {
            Ok(())
        } else {
            Err(SchedError::WrongState {
                task: id.to_string(),
                state,
                action,
            })
        }
    }

    /// The only place a task changes state. Leaving `Provisional` releases
    /// the task's permits and lease.
    fn set_state(&mut self, id: &str, to: TaskState, at: u64) -> Result<(), SchedError> {
        let Core { tasks, pool, .. } = self;
        let t = tasks
            .get_mut(id)
            .ok_or_else(|| SchedError::UnknownTask(id.to_string()))?;
        check_transition(t.state, to)?;
        t.timing.close(t.state, at);
        if t.state.holds_permits() && !to.holds_permits() {
            pool.release(t.resources);
            t.lease_expires_at = None;
        }
        t.state = to;
        Ok(())
    }

    fn dequeue(&mut self, id: &str) {
        if let Some(t) = self.tasks.get_mut(id) {
            if t.queued {
                t.queued = false;
                self.queue.retain(|q| q != id);
            }
        }
    }

    fn clear_work(t: &mut TaskEntry, reason: &str) {
        t.interface = None;
        t.completion = None;
        t.bindings.clear();
        t.provisional_result = None;
        t.last_reason = Some(reason.to_string());
    }

    fn reset_to_blocked(
        &mut self,
        id: &str,
        generation: u64,
        at: u64,
        reason: &str,
        fx: &mut Effects,
    ) -> Result<(), SchedError> {
        if self.task(id)?.state != TaskState::Blocked {
            self.set_state(id, TaskState::Blocked, at)?;
        }
        let t = self.task_mut(id)?;
        t.generation = generation;
        t.timing.revocations += 1;
        Self::clear_work(t, reason);
        fx.revoked.push(id.to_string());
        Ok(())
    }

    fn abort(
        &mut self,
        id: &str,
        at: u64,
        reason: &str,
        fx: &mut Effects,
    ) -> Result<(), SchedError> {
        self.dequeue(id);
        self.set_state(id, TaskState::Aborted, at)?;
        Self::clear_work(self.task_mut(id)?, reason);
        fx.aborted.push(id.to_string());
        Ok(())
    }

    /// Return every active descendant of `producer` to `Blocked` at a new
    /// generation and discard its provisional result. Queued descendants
    /// leave the queue and are re-checked by [`Core::pump`].
    fn revoke_descendants(
        &mut self,
        producer: &str,
        at: u64,
        reason: &str,
        fx: &mut Effects,
    ) -> Result<(), SchedError> {
        for d in self.graph.descendants(producer) {
            let t = self.task(&d)?;
            if t.state.is_active() {
                let next = t.generation + 1;
                let why = format!("{reason} (upstream '{producer}')");
                self.reset_to_blocked(&d, next, at, &why, fx)?;
            } else {
                self.dequeue(&d);
            }
        }
        Ok(())
    }

    fn eligible(&self, cfg: &ProvisionalConfig, id: &str) -> bool {
        let Some(t) = self.tasks.get(id) else {
            return false;
        };
        t.state == TaskState::Blocked
            && self.graph.producers_of(id).all(|p| {
                self.tasks
                    .get(p)
                    .is_some_and(|pt| pt.final_ready() || (cfg.enabled && pt.interface_ready()))
            })
    }

    fn bindings_for(&self, id: &str) -> BTreeMap<String, Binding> {
        let mut out = BTreeMap::new();
        for p in self.graph.producers_of(id) {
            let Some(pt) = self.tasks.get(p) else {
                continue;
            };
            let (level, claim) = if pt.final_ready() {
                (BindingLevel::Final, pt.completion.clone())
            } else {
                (BindingLevel::Interface, pt.interface.clone())
            };
            if let Some(claim) = claim {
                out.insert(
                    p.clone(),
                    Binding {
                        generation: pt.generation,
                        level,
                        claim,
                    },
                );
            }
        }
        out
    }

    /// Fill the bounded ready queue in task-id order, then dispatch from its
    /// head while permits allow (head-of-line, no reordering).
    fn pump(
        &mut self,
        cfg: &ProvisionalConfig,
        at: u64,
        fx: &mut Effects,
    ) -> Result<(), SchedError> {
        loop {
            let stale: Vec<String> = self
                .queue
                .iter()
                .filter(|id| !self.eligible(cfg, id))
                .cloned()
                .collect();
            for id in stale {
                self.dequeue(&id);
            }
            let ready: Vec<String> = self
                .tasks
                .iter()
                .filter(|(id, t)| !t.queued && self.eligible(cfg, id))
                .map(|(id, _)| id.clone())
                .collect();
            for id in ready {
                if self.queue.len() >= cfg.queue_capacity {
                    break;
                }
                self.queue.push_back(id.clone());
                self.task_mut(&id)?.queued = true;
            }
            let mut progressed = false;
            while let Some(head) = self.queue.front().cloned() {
                let req = self.task(&head)?.resources;
                if !self.pool.try_acquire(&cfg.limits, req) {
                    break;
                }
                self.queue.pop_front();
                let bindings = self.bindings_for(&head);
                self.set_state(&head, TaskState::Provisional, at)?;
                let t = self.task_mut(&head)?;
                t.queued = false;
                t.bindings = bindings;
                t.lease_expires_at = Some(at.saturating_add(cfg.lease_ttl));
                t.timing.dispatches += 1;
                t.timing.first_dispatch_at.get_or_insert(at);
                fx.dispatched.push(head);
                progressed = true;
            }
            if !progressed {
                return Ok(());
            }
        }
    }

    /// Apply one journal entry. On error the caller discards this copy of
    /// the state, so a refused event changes nothing.
    pub(crate) fn apply(
        &mut self,
        cfg: &ProvisionalConfig,
        e: &JournalEntry,
    ) -> Result<Effects, SchedError> {
        if e.seq != self.seq + 1 {
            return Err(bad(format!(
                "sequence {} out of order, expected {}",
                e.seq,
                self.seq + 1
            )));
        }
        if self.seen.contains_key(&e.id) {
            return Err(SchedError::EventIdConflict(e.id.clone()));
        }
        if e.at < self.last_at {
            return Err(bad("event time is earlier than the previous event"));
        }
        self.last_at = e.at;
        let at = e.at;
        let mut fx = Effects::default();
        match &e.kind {
            EventKind::RegisterTask {
                task,
                resources,
                producers,
            } => {
                check_identifier("task", task).map_err(|err| bad(err.to_string()))?;
                if !cfg.limits.admits(*resources) {
                    return Err(SchedError::ExceedsCapacity(task.clone()));
                }
                self.graph.add_node(task)?;
                for p in producers {
                    self.graph.add_edge(p, task)?;
                }
                self.tasks
                    .insert(task.clone(), TaskEntry::new(*resources, at));
            }
            EventKind::AddDependency { producer, consumer } => {
                self.graph.check_edge(producer, consumer)?;
                self.require_state(consumer, |s| s == TaskState::Blocked, "gain a dependency")?;
                self.graph.add_edge(producer, consumer)?;
                self.dequeue(consumer);
            }
            EventKind::InterfacePublished {
                task,
                generation,
                claim,
            }
            | EventKind::CompletionVerified {
                task,
                generation,
                claim,
            } => {
                let is_interface = matches!(e.kind, EventKind::InterfacePublished { .. });
                self.require_generation(task, *generation)?;
                self.require_state(task, TaskState::is_active, "publish proof")?;
                let t = self.task_mut(task)?;
                let slot = if is_interface {
                    &mut t.interface
                } else {
                    &mut t.completion
                };
                let changed = slot.is_some() && slot.as_ref() != Some(claim);
                *slot = Some(claim.clone());
                if changed {
                    self.revoke_descendants(task, at, "producer proof changed", &mut fx)?;
                }
            }
            EventKind::NewGeneration { task, generation } => {
                let current = self.task(task)?.generation;
                if *generation <= current {
                    return Err(SchedError::StaleGeneration {
                        task: task.clone(),
                        submitted: *generation,
                        current,
                    });
                }
                self.require_state(
                    task,
                    |s| s != TaskState::Aborted,
                    "start a new generation (use retry)",
                )?;
                self.reset_to_blocked(task, *generation, at, "new generation", &mut fx)?;
                self.revoke_descendants(task, at, "producer new generation", &mut fx)?;
            }
            EventKind::ProducerFailed {
                task,
                generation,
                reason,
            } => {
                self.require_generation(task, *generation)?;
                self.require_state(task, |s| s != TaskState::Aborted, "fail")?;
                self.abort(task, at, &format!("failed: {reason}"), &mut fx)?;
                self.revoke_descendants(task, at, "producer failed", &mut fx)?;
            }
            EventKind::WorkDone {
                task,
                generation,
                result_digest,
            } => {
                self.require_generation(task, *generation)?;
                self.require_state(task, |s| s == TaskState::Provisional, "report work done")?;
                if !self.task(task)?.lease_valid(at) {
                    return Err(SchedError::LeaseExpired(task.clone()));
                }
                if result_digest.is_empty() {
                    return Err(bad("result_digest is empty"));
                }
                self.set_state(task, TaskState::AwaitingFinalProof, at)?;
                self.task_mut(task)?.provisional_result = Some(result_digest.clone());
            }
            EventKind::Finalize { task } => {
                self.require_state(task, |s| s == TaskState::AwaitingFinalProof, "finalize")?;
                for p in self.graph.producers_of(task) {
                    let pt = self.task(p)?;
                    let bound = self.task(task)?.bindings.get(p).map(|b| b.generation);
                    if bound != Some(pt.generation) || !pt.final_ready() {
                        return Err(SchedError::ProducerNotFinal {
                            producer: p.clone(),
                            detail: "not committable with a completion at the bound generation"
                                .to_string(),
                        });
                    }
                }
                self.set_state(task, TaskState::Committable, at)?;
                self.task_mut(task)?.timing.committable_at = Some(at);
            }
            EventKind::Heartbeat { task } => {
                self.require_state(task, |s| s == TaskState::Provisional, "heartbeat")?;
                let t = self.task_mut(task)?;
                if !t.lease_valid(at) {
                    return Err(SchedError::LeaseExpired(task.clone()));
                }
                t.lease_expires_at = Some(at.saturating_add(cfg.lease_ttl));
            }
            EventKind::Cancel { task } => {
                self.require_state(task, |s| s != TaskState::Aborted, "cancel")?;
                self.abort(task, at, "cancelled", &mut fx)?;
                for d in self.graph.descendants(task) {
                    if self.task(&d)?.state != TaskState::Aborted {
                        self.abort(&d, at, &format!("ancestor '{task}' cancelled"), &mut fx)?;
                    }
                }
            }
            EventKind::Retry { task } => {
                self.require_state(task, |s| s == TaskState::Aborted, "retry")?;
                self.set_state(task, TaskState::Blocked, at)?;
                let t = self.task_mut(task)?;
                t.generation += 1;
                Self::clear_work(t, "retry");
            }
            EventKind::Revoke { task, reason } => {
                self.require_state(task, TaskState::is_active, "be revoked")?;
                let next = self.task(task)?.generation + 1;
                self.reset_to_blocked(task, next, at, reason, &mut fx)?;
                self.revoke_descendants(task, at, reason, &mut fx)?;
            }
            EventKind::Tick => {
                let expired: Vec<String> = self
                    .tasks
                    .iter()
                    .filter(|(_, t)| t.state == TaskState::Provisional && !t.lease_valid(at))
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in expired {
                    if self.task(&id)?.state == TaskState::Provisional {
                        self.abort(&id, at, "lease expired", &mut fx)?;
                        self.revoke_descendants(&id, at, "producer lease expired", &mut fx)?;
                    }
                }
            }
        }
        self.pump(cfg, at, &mut fx)?;
        self.seen.insert(e.id.clone(), payload_digest(&e.kind));
        self.seq = e.seq;
        Ok(fx)
    }
}

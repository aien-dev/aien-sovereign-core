//! Bounded memory of applied event ids, for idempotent redelivery.
//!
//! Two scopes keep it bounded:
//! - Generation-bound events (`InterfacePublished`, `CompletionVerified`,
//!   `NewGeneration`, `ProducerFailed`, `WorkDone`) are remembered under
//!   their task and generation. When the task moves to a higher generation
//!   the older buckets are dropped: a redelivery of such an event is then
//!   refused as a stale generation instead of reported as a duplicate, and
//!   either way it changes nothing.
//! - Every other event is remembered in a FIFO window of
//!   `ProvisionalConfig::dedup_window` ids. A redelivery older than the
//!   window is treated as a new event, so callers must not redeliver
//!   generation-free events (`Cancel`, `Retry`, `Heartbeat`, ...) after more
//!   than that many newer ones.
//!
//! Pruning is deterministic, so a journal replay rebuilds the same memory.

use super::core::EventKind;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Seen {
    /// id -> payload digest for every remembered event.
    ids: BTreeMap<String, String>,
    /// task -> generation -> ids of generation-bound events.
    by_task: BTreeMap<String, BTreeMap<u64, Vec<String>>>,
    /// ids of generation-free events, oldest first.
    window: VecDeque<String>,
}

/// The task and generation an event is bound to, if any.
pub(crate) fn generation_scope(kind: &EventKind) -> Option<(&str, u64)> {
    match kind {
        EventKind::InterfacePublished {
            task, generation, ..
        }
        | EventKind::CompletionVerified {
            task, generation, ..
        }
        | EventKind::NewGeneration { task, generation }
        | EventKind::ProducerFailed {
            task, generation, ..
        }
        | EventKind::WorkDone {
            task, generation, ..
        } => Some((task.as_str(), *generation)),
        _ => None,
    }
}

impl Seen {
    pub fn get(&self, id: &str) -> Option<&String> {
        self.ids.get(id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.ids.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub(crate) fn record(&mut self, id: &str, digest: String, kind: &EventKind, window: usize) {
        self.ids.insert(id.to_string(), digest);
        match generation_scope(kind) {
            Some((task, generation)) => self
                .by_task
                .entry(task.to_string())
                .or_default()
                .entry(generation)
                .or_default()
                .push(id.to_string()),
            None => {
                self.window.push_back(id.to_string());
                while self.window.len() > window {
                    if let Some(old) = self.window.pop_front() {
                        self.ids.remove(&old);
                    }
                }
            }
        }
    }

    /// Forget generation-bound ids below each task's current generation.
    pub(crate) fn prune(&mut self, current: impl Fn(&str) -> Option<u64>) {
        let Seen { ids, by_task, .. } = self;
        for (task, gens) in by_task.iter_mut() {
            let Some(now) = current(task) else { continue };
            let keep = gens.split_off(&now);
            for old in std::mem::replace(gens, keep).into_values().flatten() {
                ids.remove(&old);
            }
        }
        by_task.retain(|_, gens| !gens.is_empty());
    }
}

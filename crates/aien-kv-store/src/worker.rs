//! The storage worker (cut C5, ADR 0003 sections 7 and 8).
//!
//! RED STUB: types and signatures only; every behaviour is `unimplemented!`.
#![allow(dead_code)]

use crate::identity::{ChunkKey, NamespaceId};
use crate::pin::{PinnedDest, PinnedSource};
use crate::record::ChunkHeader;
use crate::store::{
    CancelOutcome, Capacity, Completion, DrainReport, Incarnation, JobId, KvStore, PinToken,
    RecoveryProgress,
};
use aien_platform::PlatformError;
use std::string::String;
use std::sync::Arc;
use std::time::Duration;
use std::vec::Vec;

/// A setting that could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub name: String,
    pub value: String,
    pub reason: &'static str,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}={:?}: {}", self.name, self.value, self.reason)
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerConfig {
    pub max_restore_ops: u32,
    pub max_park_ops: u32,
    pub max_inflight_bytes: u64,
    pub max_waiters_per_job: u32,
    pub max_waiters_total: u32,
    pub dispatch_depth: u32,
    pub idle_poll: Duration,
    pub start_paused: bool,
    pub drain_ms: u64,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            max_restore_ops: 48,
            max_park_ops: 16,
            max_inflight_bytes: 256 << 20,
            max_waiters_per_job: 16,
            max_waiters_total: 1024,
            dispatch_depth: 4,
            idle_poll: Duration::from_micros(500),
            start_paused: false,
            drain_ms: 2000,
        }
    }
}

impl WorkerConfig {
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let _ = get;
        unimplemented!("C5 green")
    }

    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|n| std::env::var(n).ok())
    }
}

/// The two intake classes (ADR 7.1). A flush counts as a park (C2 choice).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Restore,
    Park,
}

/// How `submit_load_coalesced` placed a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadTicket {
    Started(JobId),
    Attached(JobId),
}

impl LoadTicket {
    pub fn job(&self) -> JobId {
        match *self {
            LoadTicket::Started(j) | LoadTicket::Attached(j) => j,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerStats {
    pub submitted_restore: u64,
    pub submitted_park: u64,
    pub queue_full_total: u64,
    pub dispatched_restore: u64,
    pub dispatched_park: u64,
    pub max_restore_run_with_park_waiting: u32,
    pub coalesced_total: u64,
    pub attach_refused_total: u64,
    pub store_dedup_total: u64,
    pub stale_completions_total: u64,
    pub terminal_done: u64,
    pub terminal_failed: u64,
    pub terminal_cancelled: u64,
    pub peak_completion_queue: u64,
    pub completion_overflow_total: u64,
    pub peak_restore_queue: u64,
    pub peak_park_queue: u64,
    pub inner_refusals_total: u64,
    pub inner_protocol_errors: u64,
    pub indeterminate_total: u64,
    pub shutdown_unfinished_jobs: u64,
    pub shutdown_unfinished_bytes: u64,
}

pub struct StorageWorker {
    inner: Arc<dyn KvStore>,
}

impl StorageWorker {
    pub fn spawn(
        inner: Arc<dyn KvStore>,
        config: WorkerConfig,
        drive: impl FnMut() + Send + 'static,
    ) -> Result<StorageWorker, PlatformError> {
        let _ = (config, drive);
        Ok(StorageWorker { inner })
    }

    pub fn pause_dispatch(&self) {
        unimplemented!("C5 green")
    }

    pub fn resume_dispatch(&self) {
        unimplemented!("C5 green")
    }

    pub fn submit_load_coalesced(
        &self,
        key: ChunkKey,
        expect: &ChunkHeader,
        make_dst: impl FnOnce() -> PinnedDest,
        inc: Incarnation,
    ) -> Result<LoadTicket, PlatformError> {
        let _ = (key, expect, make_dst, inc);
        unimplemented!("C5 green")
    }

    pub fn load_in_flight(&self, key: &ChunkKey) -> Option<JobId> {
        let _ = key;
        unimplemented!("C5 green")
    }

    pub fn stats(&self) -> WorkerStats {
        unimplemented!("C5 green")
    }

    pub fn dispatch_order(&self) -> Vec<(JobId, Class)> {
        unimplemented!("C5 green")
    }

    pub fn inner_job(&self, job: JobId) -> Option<JobId> {
        let _ = job;
        unimplemented!("C5 green")
    }

    pub fn state_lock_is_free(&self) -> bool {
        unimplemented!("C5 green")
    }
}

impl KvStore for StorageWorker {
    fn namespace(&self) -> NamespaceId {
        unimplemented!("C5 green")
    }
    fn capacity(&self) -> Capacity {
        unimplemented!("C5 green")
    }
    fn lookup(&self, keys: &[ChunkKey]) -> Vec<Option<u64>> {
        let _ = keys;
        unimplemented!("C5 green")
    }
    fn submit_store(
        &self,
        header: ChunkHeader,
        src: PinnedSource,
        inc: Incarnation,
    ) -> Result<JobId, PlatformError> {
        let _ = (header, src, inc);
        unimplemented!("C5 green")
    }
    fn submit_load(
        &self,
        key: ChunkKey,
        expect: &ChunkHeader,
        dst: PinnedDest,
        inc: Incarnation,
    ) -> Result<JobId, PlatformError> {
        let _ = (key, expect, dst, inc);
        unimplemented!("C5 green")
    }
    fn submit_flush(&self, inc: Incarnation) -> Result<JobId, PlatformError> {
        let _ = inc;
        unimplemented!("C5 green")
    }
    fn attach_waiter(&self, job: JobId) -> Result<(), PlatformError> {
        let _ = job;
        unimplemented!("C5 green")
    }
    fn poll(&self, max: usize) -> Vec<Completion> {
        let _ = max;
        unimplemented!("C5 green")
    }
    fn cancel(&self, job: JobId) -> CancelOutcome {
        let _ = job;
        unimplemented!("C5 green")
    }
    fn pin(&self, keys: &[ChunkKey]) -> Result<PinToken, PlatformError> {
        let _ = keys;
        unimplemented!("C5 green")
    }
    fn unpin(&self, token: PinToken) {
        let _ = token;
        unimplemented!("C5 green")
    }
    fn delete(&self, keys: &[ChunkKey]) -> u64 {
        let _ = keys;
        unimplemented!("C5 green")
    }
    fn recover_step(&self, slice_ms: u64) -> RecoveryProgress {
        let _ = slice_ms;
        unimplemented!("C5 green")
    }
    fn shutdown(&self, intake_deadline_ms: u64) -> DrainReport {
        let _ = intake_deadline_ms;
        unimplemented!("C5 green")
    }
}

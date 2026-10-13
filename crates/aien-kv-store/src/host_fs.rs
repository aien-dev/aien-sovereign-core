//! RED STUB (cut C4): the public surface of the host filesystem adapter with
//! unimplemented bodies. The implementation lands in the green commit.

use crate::identity::{ChunkKey, LayoutStrides, NamespaceFields, NamespaceId};
use crate::pin::{PinnedDest, PinnedSource};
use crate::record::ChunkHeader;
use crate::store::{
    CancelOutcome, Capacity, Completion, DrainReport, Incarnation, JobId, KvStore, PinToken,
    RecoveryProgress,
};
use aien_platform::PlatformError;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const ENOSPC: i32 = 28;
pub const EIO: i32 = 5;

/// Who runs the worker steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerMode {
    Manual,
    Thread,
}

/// Where a fault can be injected (ADR 2.4 P1 to P5, plus the other I/O).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Boundary {
    P1Write,
    P2FsyncFile,
    P3Rename,
    P4FsyncDir,
    P5IndexInsert,
    Unlink,
    LoadRead,
    FlushSync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultAction {
    Fail(i32),
    TornWrite { after_bytes: u64, crash: bool },
    Crash,
    Stall(Duration),
}

pub trait FaultHook: Send + Sync {
    fn at(&self, boundary: Boundary, key: &ChunkKey) -> Option<FaultAction>;
}

#[derive(Default)]
struct Script {
    armed: BTreeMap<Boundary, Vec<FaultAction>>,
    fired: BTreeMap<Boundary, u64>,
    seen: Vec<Boundary>,
}

/// One-shot faults per boundary, consumed in the order they were armed.
#[derive(Default)]
pub struct ScriptedFaults {
    inner: Mutex<Script>,
}

impl ScriptedFaults {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn arm(&self, boundary: Boundary, action: FaultAction) {
        let mut s = self.inner.lock().unwrap();
        s.armed.entry(boundary).or_default().push(action);
    }

    pub fn fired(&self, boundary: Boundary) -> u64 {
        self.inner
            .lock()
            .unwrap()
            .fired
            .get(&boundary)
            .copied()
            .unwrap_or(0)
    }

    /// Every boundary the adapter consulted, in order.
    pub fn seen(&self) -> Vec<Boundary> {
        self.inner.lock().unwrap().seen.clone()
    }

    pub fn armed(&self, boundary: Boundary) -> usize {
        self.inner
            .lock()
            .unwrap()
            .armed
            .get(&boundary)
            .map_or(0, Vec::len)
    }
}

impl FaultHook for ScriptedFaults {
    fn at(&self, boundary: Boundary, _key: &ChunkKey) -> Option<FaultAction> {
        let mut s = self.inner.lock().unwrap();
        s.seen.push(boundary);
        let list = s.armed.get_mut(&boundary)?;
        if list.is_empty() {
            return None;
        }
        let a = list.remove(0);
        *s.fired.entry(boundary).or_insert(0) += 1;
        Some(a)
    }
}

#[derive(Clone)]
pub struct HostFsConfig {
    pub root: PathBuf,
    pub namespace: NamespaceId,
    pub model_sha256: [u8; 32],
    pub layout: LayoutStrides,
    pub num_layers: u32,
    pub num_kv_heads: u32,
    pub head_dim: u32,
    pub block_size: u32,
    pub chunk_tokens: u32,
    pub dtype: u8,
    pub backend_format: u8,
    pub quota_bytes: u64,
    pub max_objects: u64,
    pub max_restore_ops: u32,
    pub max_park_ops: u32,
    pub max_inflight_bytes: u64,
    pub max_waiters_per_job: u32,
    pub max_waiters_total: u32,
    pub recovery_slice_ms: u64,
    pub purge_after_recovery: bool,
    pub worker: WorkerMode,
    pub faults: Option<Arc<dyn FaultHook>>,
}

impl HostFsConfig {
    #[allow(unused_variables)]
    pub fn from_fields(root: PathBuf, f: &NamespaceFields<'_>) -> Result<Self, PlatformError> {
        unimplemented!("C4 red stub")
    }

    #[allow(unused_variables)]
    pub fn apply_env(&mut self, get: impl Fn(&str) -> Option<String>) -> Result<(), PlatformError> {
        unimplemented!("C4 red stub")
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostFsStats {
    pub io_ops: u64,
    pub reads: u64,
    pub read_bytes: u64,
    pub writes: u64,
    pub write_bytes: u64,
    pub file_fsyncs: u64,
    pub dir_fsyncs: u64,
    pub renames: u64,
    pub unlinks: u64,
    pub metadata_calls: u64,
    pub mkdirs: u64,
    pub dir_entries: u64,
    pub queue_full_total: u64,
    pub disk_full_total: u64,
    pub cancelled_total: u64,
    pub evicted_bytes_total: u64,
    pub dedup_total: u64,
    pub indeterminate_total: u64,
    pub published_total: u64,
    pub terminal_done: u64,
    pub terminal_failed: u64,
    pub terminal_cancelled: u64,
    pub peak_completion_queue: u64,
    pub completion_overflow_total: u64,
    pub released_source_violations: u64,
    pub corrupt_deleted_total: u64,
    pub foreign_files: u64,
    pub foreign_file_bytes: u64,
    pub tmp_bytes_deleted: u64,
    pub corrupt_bytes_deleted: u64,
    pub purge_unlinked: u64,
    pub tail_scratch_bytes: u64,
    pub over_cap: bool,
    pub shutdown_unfinished_jobs: u64,
    pub shutdown_unfinished_bytes: u64,
}

pub struct HostFsStore {
    _private: (),
}

#[allow(unused_variables)]
impl HostFsStore {
    pub fn open(config: HostFsConfig) -> Result<Self, PlatformError> {
        unimplemented!("C4 red stub")
    }
    pub fn pump(&self) -> bool {
        unimplemented!("C4 red stub")
    }
    pub fn run_until_idle(&self) {
        unimplemented!("C4 red stub")
    }
    pub fn stats(&self) -> HostFsStats {
        unimplemented!("C4 red stub")
    }
    pub fn store_id(&self) -> Option<[u8; 16]> {
        unimplemented!("C4 red stub")
    }
    pub fn root(&self) -> &Path {
        unimplemented!("C4 red stub")
    }
    pub fn object_path(&self, key: &ChunkKey) -> PathBuf {
        unimplemented!("C4 red stub")
    }
    pub fn tmp_dir(&self) -> PathBuf {
        unimplemented!("C4 red stub")
    }
    pub fn purge_pending(&self) -> bool {
        unimplemented!("C4 red stub")
    }
    pub fn start_purge(&self) {
        unimplemented!("C4 red stub")
    }
    pub fn audit(&self) -> Result<(), String> {
        unimplemented!("C4 red stub")
    }
}

#[allow(unused_variables)]
impl KvStore for HostFsStore {
    fn namespace(&self) -> NamespaceId {
        unimplemented!("C4 red stub")
    }
    fn capacity(&self) -> Capacity {
        unimplemented!("C4 red stub")
    }
    fn lookup(&self, keys: &[ChunkKey]) -> Vec<Option<u64>> {
        unimplemented!("C4 red stub")
    }
    fn submit_store(
        &self,
        header: ChunkHeader,
        src: PinnedSource,
        inc: Incarnation,
    ) -> Result<JobId, PlatformError> {
        unimplemented!("C4 red stub")
    }
    fn submit_load(
        &self,
        key: ChunkKey,
        expect: &ChunkHeader,
        dst: PinnedDest,
        inc: Incarnation,
    ) -> Result<JobId, PlatformError> {
        unimplemented!("C4 red stub")
    }
    fn submit_flush(&self, inc: Incarnation) -> Result<JobId, PlatformError> {
        unimplemented!("C4 red stub")
    }
    fn attach_waiter(&self, job: JobId) -> Result<(), PlatformError> {
        unimplemented!("C4 red stub")
    }
    fn poll(&self, max: usize) -> Vec<Completion> {
        unimplemented!("C4 red stub")
    }
    fn cancel(&self, job: JobId) -> CancelOutcome {
        unimplemented!("C4 red stub")
    }
    fn pin(&self, keys: &[ChunkKey]) -> Result<PinToken, PlatformError> {
        unimplemented!("C4 red stub")
    }
    fn unpin(&self, token: PinToken) {
        unimplemented!("C4 red stub")
    }
    fn delete(&self, keys: &[ChunkKey]) -> u64 {
        unimplemented!("C4 red stub")
    }
    fn recover_step(&self, slice_ms: u64) -> RecoveryProgress {
        unimplemented!("C4 red stub")
    }
    fn shutdown(&self, intake_deadline_ms: u64) -> DrainReport {
        unimplemented!("C4 red stub")
    }
}

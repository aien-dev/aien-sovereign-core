//! Deterministic in-memory `KvStore` (ADR 0003 section 13): a virtual clock,
//! scripted latencies and faults, no threads, no real time, no I/O. The same
//! script gives the same completions in the same order.
//!
//! The virtual clock is the **only** place I/O and eviction happen: `advance`
//! (and `run_until_idle`, `shutdown`) run the worker's steps. `submit_*`
//! never does, which a test can assert by counting I/O calls (`FakeStats::io_ops`).

use crate::identity::{ChunkKey, NamespaceId};
use crate::pin::{PinnedDest, PinnedSource};
use crate::record::ChunkHeader;
use crate::store::{
    CancelOutcome, Capacity, Completion, DrainReport, Incarnation, JobId, JobStage, KvStore,
    PinToken, RecoveryProgress, StoreState,
};
use aien_platform::PlatformError;
use alloc::vec::Vec;

/// What `Durable` means (ADR 3.3 table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurabilityMode {
    /// Native-like: a store ends `Published` (not durable); a later flush job
    /// makes it so.
    OnFlush,
    /// Host-like: publication includes the directory sync; the store's
    /// terminal completion carries `Durable` (or `Indeterminate`).
    WithPublish,
}

/// One injectable misbehaviour, attached to the next job through `JobScript`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// `submit_*` refuses with `QueueFull` regardless of the real occupancy.
    QueueFull,
    /// The write fails with `OutOfMemory`: the filesystem refuses despite the quota (P1).
    DiskFull,
    /// A flush job fails (`HardwareFault`); the store fail-stops (`Failed`) on
    /// `OnFlush`, goes `Degraded` on `WithPublish`.
    FlushFail,
    /// The write is cut short: the job fails and a partial temporary object
    /// remains until `recover_step` (P1, crash style).
    TornWrite,
    /// `fsync(file)` fails (P2): the job fails, nothing is published, the temp
    /// is unlinked, and the store enters `Degraded` (`Failed` on `OnFlush`).
    FsyncFileFail,
    /// Host-only (ignored on `OnFlush`): rename succeeds, `fsync(dir)` fails
    /// (P4): the object is published and served, the job ends `Indeterminate`,
    /// the store goes `Degraded`.
    RenameOkDirsyncFail,
    /// `fsync(file)` takes this many extra virtual microseconds and cannot be
    /// cancelled while it runs.
    StallFsync(u64),
    /// Reclamation of this store job takes this many extra microseconds.
    StallEviction(u64),
    /// The first victim unlink of this store job fails: the victim stays, the
    /// job fails with `HardwareFault`.
    UnlinkFail,
    /// Do not start until `FakeKvStore::release(job)`.
    Hold,
    /// Deliberate adapter misbehaviour for testing callers: every completion
    /// of this job is reported twice (the terminal count becomes 2).
    DuplicateCompletion,
    /// Deliberate adapter misbehaviour for testing callers: after `cancel`
    /// answered `Cancelled` and the terminal `Cancelled` was reported, a stale
    /// `Completed` progress completion is still reported.
    CompletionAfterCancel,
}

/// Latency and faults for the next job submitted. Scripts are consumed in FIFO
/// order, one per `submit_*` call, whatever the call's outcome.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobScript {
    /// Virtual microseconds from start to `Completed` (write plus file sync).
    pub completed_after_us: u64,
    /// Virtual microseconds from `Completed` to publication (rename plus dir sync).
    pub published_after_us: u64,
    pub faults: Vec<Fault>,
}

impl JobScript {
    pub fn latency(completed_after_us: u64, published_after_us: u64) -> Self {
        Self {
            completed_after_us,
            published_after_us,
            faults: Vec::new(),
        }
    }

    pub fn fault(fault: Fault) -> Self {
        Self::default().with(fault)
    }

    pub fn with(mut self, fault: Fault) -> Self {
        self.faults.push(fault);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FakeConfig {
    pub namespace: NamespaceId,
    pub quota_bytes: u64,
    pub max_objects: u64,
    pub max_restore_ops: u32,
    pub max_park_ops: u32,
    pub max_inflight_bytes: u64,
    pub max_waiters_per_job: u32,
    pub max_waiters_total: u32,
    pub durability: DurabilityMode,
    /// Virtual microseconds one unlink takes.
    pub unlink_us: u64,
    /// Virtual microseconds a flush barrier takes.
    pub flush_us: u64,
    /// Default latencies for jobs with no script.
    pub default_completed_after_us: u64,
    pub default_published_after_us: u64,
    /// Start in `Recovering`; the unindexed disk entries are walked by `recover_step`.
    pub start_recovering: bool,
    /// Disk entries one millisecond of `recover_step` slice accounts for.
    pub recovery_entries_per_ms: u64,
}

impl FakeConfig {
    /// Defaults: 64 MiB quota, 1024 objects, 48 restore and 16 park ops
    /// (ADR 7.1), 256 MiB in flight, 16 waiters per job and 1024 in total,
    /// `WithPublish`, instant unlink and flush, 100 us to complete and 100 us
    /// to publish, open at once.
    pub fn new(namespace: NamespaceId) -> Self {
        let _ = namespace;
        unimplemented!("C2 red stub")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FakeStats {
    /// Every I/O-like step: payload write, file sync, rename, dir sync, payload
    /// read, unlink, flush. `submit_*` never changes it.
    pub io_ops: u64,
    pub reads: u64,
    pub writes: u64,
    pub unlinks: u64,
    pub queue_full_total: u64,
    pub disk_full_total: u64,
    pub cancelled_total: u64,
    pub evicted_bytes_total: u64,
    pub dedup_total: u64,
    pub indeterminate_total: u64,
    /// Terminal completions generated, by kind.
    pub terminal_done: u64,
    pub terminal_failed: u64,
    pub terminal_cancelled: u64,
    /// High-water mark of unpolled completions.
    pub peak_completion_queue: u64,
    /// Completions queued beyond the reserved capacity: only the two
    /// deliberate-misbehaviour faults can raise it.
    pub completion_overflow_total: u64,
    /// Reads or writes of pool memory whose holder had already been released.
    pub poison_violations: u64,
    pub shutdown_unfinished_jobs: u64,
    pub shutdown_unfinished_bytes: u64,
}

/// A file found on the fake disk by `recover_step`.
#[derive(Debug, Clone)]
pub enum DiskEntry {
    /// A complete object: indexed when the namespace matches, counted always.
    Object {
        header: ChunkHeader,
        payload: Vec<u8>,
        segment_bytes: usize,
    },
    /// A leftover `tmp/*.part`: unlinked, not counted.
    Tmp,
    /// An object whose header region fails validation: unlinked.
    CorruptHeader { physical_bytes: u64 },
}

pub struct FakeKvStore {
    _config: FakeConfig,
}

impl FakeKvStore {
    pub fn new(config: FakeConfig) -> Self {
        Self { _config: config }
    }

    /// Queue a script for the next `submit_*` call.
    pub fn script(&self, script: JobScript) {
        let _ = script;
        unimplemented!("C2 red stub")
    }

    pub fn now_us(&self) -> u64 {
        unimplemented!("C2 red stub")
    }

    /// Move the virtual clock forward, running every due step in time order.
    pub fn advance(&self, us: u64) {
        let _ = us;
        unimplemented!("C2 red stub")
    }

    /// Run every scheduled step, jumping the clock to each; held jobs stay held.
    pub fn run_until_idle(&self) {
        unimplemented!("C2 red stub")
    }

    /// End a `Hold`: the job starts at the current time.
    pub fn release(&self, job: JobId) -> bool {
        let _ = job;
        unimplemented!("C2 red stub")
    }

    pub fn stats(&self) -> FakeStats {
        unimplemented!("C2 red stub")
    }

    pub fn payload_of(&self, key: &ChunkKey) -> Option<Vec<u8>> {
        let _ = key;
        unimplemented!("C2 red stub")
    }

    /// `Some(true)` after publication on `WithPublish` or after a flush that
    /// started later than the publication on `OnFlush`.
    pub fn is_durable(&self, key: &ChunkKey) -> Option<bool> {
        let _ = key;
        unimplemented!("C2 red stub")
    }

    pub fn is_indeterminate(&self, key: &ChunkKey) -> Option<bool> {
        let _ = key;
        unimplemented!("C2 red stub")
    }

    /// Flip one payload byte of a published object (a hash failure on next load).
    pub fn corrupt_payload(&self, key: &ChunkKey) -> bool {
        let _ = key;
        unimplemented!("C2 red stub")
    }

    /// Put a published object in place without a job (test setup). It is durable.
    pub fn inject_object(&self, header: ChunkHeader, payload: Vec<u8>, segment_bytes: usize) {
        let _ = (header, payload, segment_bytes);
        unimplemented!("C2 red stub")
    }

    /// Put a file on the fake disk that only `recover_step` will find.
    pub fn inject_disk_entry(&self, entry: DiskEntry) {
        let _ = entry;
        unimplemented!("C2 red stub")
    }

    /// Partial temporary objects left by torn writes, until `recover_step`.
    pub fn torn_residue(&self) -> u64 {
        unimplemented!("C2 red stub")
    }

    /// Terminal completions generated for `job` so far (1 once it ended, in
    /// every case but the duplicate-completion fault).
    pub fn terminal_count(&self, job: JobId) -> u32 {
        let _ = job;
        unimplemented!("C2 red stub")
    }

    /// Jobs that were accepted and have not reached their terminal completion.
    pub fn live_jobs(&self) -> Vec<JobId> {
        unimplemented!("C2 red stub")
    }

    /// Stage a live job has reached.
    pub fn job_stage(&self, job: JobId) -> Option<JobStage> {
        let _ = job;
        unimplemented!("C2 red stub")
    }

    /// Completions generated and not yet polled.
    pub fn pending_completions(&self) -> usize {
        unimplemented!("C2 red stub")
    }

    /// Completion queue capacity: three slots per possible in-flight op.
    pub fn completion_capacity(&self) -> usize {
        unimplemented!("C2 red stub")
    }

    /// Acknowledge recorded poison violations (they otherwise fail the test
    /// when the store is dropped); returns how many there were.
    pub fn take_poison_violations(&self) -> u64 {
        unimplemented!("C2 red stub")
    }

    /// True when the internal lock is free right now. A holder-release
    /// callback calls this to prove it runs outside the store's lock (ADR 8).
    pub fn lock_is_free(&self) -> bool {
        unimplemented!("C2 red stub")
    }
}

impl KvStore for FakeKvStore {
    fn namespace(&self) -> NamespaceId {
        unimplemented!("C2 red stub")
    }
    fn capacity(&self) -> Capacity {
        unimplemented!("C2 red stub")
    }
    fn lookup(&self, keys: &[ChunkKey]) -> Vec<Option<u64>> {
        let _ = keys;
        unimplemented!("C2 red stub")
    }
    fn submit_store(
        &self,
        header: ChunkHeader,
        src: PinnedSource,
        inc: Incarnation,
    ) -> Result<JobId, PlatformError> {
        let _ = (header, src, inc);
        unimplemented!("C2 red stub")
    }
    fn submit_load(
        &self,
        key: ChunkKey,
        expect: &ChunkHeader,
        dst: PinnedDest,
        inc: Incarnation,
    ) -> Result<JobId, PlatformError> {
        let _ = (key, expect, dst, inc);
        unimplemented!("C2 red stub")
    }
    fn submit_flush(&self, inc: Incarnation) -> Result<JobId, PlatformError> {
        let _ = inc;
        unimplemented!("C2 red stub")
    }
    fn attach_waiter(&self, job: JobId) -> Result<(), PlatformError> {
        let _ = job;
        unimplemented!("C2 red stub")
    }
    fn poll(&self, max: usize) -> Vec<Completion> {
        let _ = max;
        unimplemented!("C2 red stub")
    }
    fn cancel(&self, job: JobId) -> CancelOutcome {
        let _ = job;
        unimplemented!("C2 red stub")
    }
    fn pin(&self, keys: &[ChunkKey]) -> Result<PinToken, PlatformError> {
        let _ = keys;
        unimplemented!("C2 red stub")
    }
    fn unpin(&self, token: PinToken) {
        let _ = token;
        unimplemented!("C2 red stub")
    }
    fn delete(&self, keys: &[ChunkKey]) -> u64 {
        let _ = keys;
        unimplemented!("C2 red stub")
    }
    fn recover_step(&self, slice_ms: u64) -> RecoveryProgress {
        let _ = slice_ms;
        unimplemented!("C2 red stub")
    }
    fn shutdown(&self, intake_deadline_ms: u64) -> DrainReport {
        let _ = intake_deadline_ms;
        unimplemented!("C2 red stub")
    }
}

#[allow(dead_code)]
fn _state_is_used(_: StoreState) {}

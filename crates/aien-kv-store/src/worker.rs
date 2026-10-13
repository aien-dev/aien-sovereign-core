//! The storage worker (cut C5, ADR 0003 sections 7 and 8).
//!
//! Two bounded intake queues (restore and park) and one bounded completion
//! queue in front of an inner `KvStore`. Caller threads never touch the inner
//! store on the hot path (`submit_*`, `poll`, `cancel` of queued jobs); a
//! single background worker thread dispatches by 3:1 credit fairness, advances
//! the inner store, and reaps completions.

use crate::coalesce::{AttachRefused, CoalesceLimits, CoalesceTable, Detach, Share};
use crate::identity::{ChunkKey, NamespaceId};
use crate::pin::{HolderGuard, PinnedDest, PinnedSource};
use crate::record::{ChunkHeader, HEADER_REGION_LEN, MAX_SEGMENTS};
use crate::store::{
    CancelOutcome, Capacity, Completion, DrainReport, Incarnation, JobId, JobStage, KvStore,
    PinToken, PublishOutcome, RecoveryProgress, StoreState, Terminal, UnfinishedJob,
};
use aien_platform::{ObjectId, PlatformError};
use std::collections::{BTreeMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::string::String;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, TryLockError};
use std::thread;
use std::time::{Duration, Instant};
use std::vec::Vec;

const HEADER_REGION: u64 = HEADER_REGION_LEN as u64;

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

fn parse_pos_u32(
    get: &impl Fn(&str) -> Option<String>,
    name: &'static str,
    default: u32,
) -> Result<u32, ConfigError> {
    let Some(raw) = get(name) else {
        return Ok(default);
    };
    let v = raw.parse::<u32>().map_err(|_| ConfigError {
        name: String::from(name),
        value: raw.clone(),
        reason: "must be a positive integer",
    })?;
    if v == 0 {
        return Err(ConfigError {
            name: String::from(name),
            value: raw,
            reason: "must be greater than zero",
        });
    }
    Ok(v)
}

fn parse_pos_u64(
    get: &impl Fn(&str) -> Option<String>,
    name: &'static str,
    default: u64,
) -> Result<u64, ConfigError> {
    let Some(raw) = get(name) else {
        return Ok(default);
    };
    let v = raw.parse::<u64>().map_err(|_| ConfigError {
        name: String::from(name),
        value: raw.clone(),
        reason: "must be a positive integer",
    })?;
    if v == 0 {
        return Err(ConfigError {
            name: String::from(name),
            value: raw,
            reason: "must be greater than zero",
        });
    }
    Ok(v)
}

fn parse_nonneg_u64(
    get: &impl Fn(&str) -> Option<String>,
    name: &'static str,
    default: u64,
) -> Result<u64, ConfigError> {
    let Some(raw) = get(name) else {
        return Ok(default);
    };
    raw.parse::<u64>().map_err(|_| ConfigError {
        name: String::from(name),
        value: raw,
        reason: "must be a non-negative integer",
    })
}

impl WorkerConfig {
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let mut c = Self::default();
        c.max_restore_ops =
            parse_pos_u32(&get, "AIEN_KV_STORE_MAX_RESTORE_OPS", c.max_restore_ops)?;
        c.max_park_ops = parse_pos_u32(&get, "AIEN_KV_STORE_MAX_PARK_OPS", c.max_park_ops)?;
        c.max_waiters_per_job = parse_pos_u32(
            &get,
            "AIEN_KV_STORE_MAX_WAITERS_PER_JOB",
            c.max_waiters_per_job,
        )?;
        c.max_inflight_bytes = parse_pos_u64(
            &get,
            "AIEN_KV_STORE_MAX_INFLIGHT_BYTES",
            c.max_inflight_bytes,
        )?;
        c.drain_ms = parse_nonneg_u64(&get, "AIEN_KV_STORE_DRAIN_MS", c.drain_ms)?;
        Ok(c)
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobKind {
    Store,
    Load,
    Flush,
}

enum Payload {
    Store {
        header: ChunkHeader,
        src: PinnedSource,
    },
    Load {
        expect: ChunkHeader,
        dst: PinnedDest,
    },
    Flush,
}

impl Payload {
    fn into_guard(self) -> Option<HolderGuard> {
        match self {
            Payload::Store { src, .. } => Some(src.into_guard()),
            Payload::Load { dst, .. } => Some(dst.into_guard()),
            Payload::Flush => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Queued,
    Dispatching,
    Dispatched(JobId),
}

struct WorkerJob {
    id: JobId,
    class: Class,
    kind: JobKind,
    inc: Incarnation,
    key: ChunkKey,
    bytes: u64,
    payload_len: u64,
    reserved: u64,
    submitted_at: Instant,
    stage: JobStage,
    status: Status,
    cancelling: bool,
    payload: Option<Payload>,
    progress_seen: bool,
}

type HandedJob = (JobId, Class, Incarnation, ChunkKey, Payload);

struct State {
    cfg: WorkerConfig,
    paused: bool,
    closed: bool,
    worker_dropped: bool,
    panicked: bool,
    next_job: u64,
    restore_queue: VecDeque<JobId>,
    park_queue: VecDeque<JobId>,
    restore_credits: u32,
    restore_run_with_park_waiting: u32,
    restore_ops: u32,
    park_ops: u32,
    restore_bytes: u64,
    park_bytes: u64,
    reserved_bytes: u64,
    dispatched_count: u32,
    jobs: BTreeMap<JobId, WorkerJob>,
    by_inner: BTreeMap<JobId, JobId>,
    coalesce: CoalesceTable,
    completions: VecDeque<Completion>,
    dispatch_log: Vec<(JobId, Class)>,
    /// Jobs taken from the queues (status `Dispatching`) and waiting for the
    /// worker thread to hand them to the inner store. Filled under the lock.
    handed: VecDeque<HandedJob>,
    stats: WorkerStats,
    published_total: u64,
    shutdown_cancelled: u64,
    shutdown_published_before: Option<u64>,
    inner_shutdown_requested: bool,
    inner_shutdown_called: bool,
    graveyard: Vec<HolderGuard>,
}

impl State {
    fn new(cfg: WorkerConfig) -> Self {
        let limits = CoalesceLimits {
            max_waiters_per_job: cfg.max_waiters_per_job,
            max_waiters_total: cfg.max_waiters_total,
        };
        let paused = cfg.start_paused;
        Self {
            cfg,
            paused,
            closed: false,
            worker_dropped: false,
            panicked: false,
            next_job: 1,
            restore_queue: VecDeque::new(),
            park_queue: VecDeque::new(),
            restore_credits: 3,
            restore_run_with_park_waiting: 0,
            restore_ops: 0,
            park_ops: 0,
            restore_bytes: 0,
            park_bytes: 0,
            reserved_bytes: 0,
            dispatched_count: 0,
            jobs: BTreeMap::new(),
            by_inner: BTreeMap::new(),
            coalesce: CoalesceTable::new(limits),
            completions: VecDeque::new(),
            dispatch_log: Vec::new(),
            handed: VecDeque::new(),
            stats: WorkerStats::default(),
            published_total: 0,
            shutdown_cancelled: 0,
            shutdown_published_before: None,
            inner_shutdown_requested: false,
            inner_shutdown_called: false,
            graveyard: Vec::new(),
        }
    }

    fn completion_capacity(&self) -> usize {
        3 * (self.cfg.max_restore_ops as usize + self.cfg.max_park_ops as usize)
    }

    fn park_byte_limit(&self) -> u64 {
        self.cfg.max_inflight_bytes / 4
    }

    fn restore_byte_limit(&self) -> u64 {
        self.cfg.max_inflight_bytes - self.park_byte_limit()
    }

    fn queue_full(&mut self) -> PlatformError {
        self.stats.queue_full_total += 1;
        PlatformError::QueueFull
    }

    fn check_store_state(
        &mut self,
        store_state: StoreState,
        needs_open: bool,
    ) -> Result<(), PlatformError> {
        if self.closed || self.panicked {
            return Err(PlatformError::DeviceUnavailable);
        }
        if store_state == StoreState::Recovering {
            return Err(self.queue_full());
        }
        if store_state == StoreState::Failed || (needs_open && store_state == StoreState::Degraded)
        {
            return Err(PlatformError::DeviceUnavailable);
        }
        Ok(())
    }

    fn credit_check(&mut self, class: Class, bytes: u64) -> Result<(), PlatformError> {
        let (ops, max_ops, used_bytes, limit) = match class {
            Class::Park => (
                self.park_ops,
                self.cfg.max_park_ops,
                self.park_bytes,
                self.park_byte_limit(),
            ),
            Class::Restore => (
                self.restore_ops,
                self.cfg.max_restore_ops,
                self.restore_bytes,
                self.restore_byte_limit(),
            ),
        };
        let slots = self.completions.len() + 3 * self.jobs.len() + 3;
        if ops >= max_ops || used_bytes + bytes > limit || slots > self.completion_capacity() {
            return Err(self.queue_full());
        }
        Ok(())
    }

    fn push_completion(&mut self, c: Completion) {
        self.completions.push_back(c);
        let len = self.completions.len();
        if len > self.completion_capacity() {
            self.stats.completion_overflow_total += 1;
        }
        self.stats.peak_completion_queue = self.stats.peak_completion_queue.max(len as u64);
    }

    fn finish_job(
        &mut self,
        mut job: WorkerJob,
        terminal: Terminal,
        stage: JobStage,
        publish: PublishOutcome,
        elapsed_us: Option<u64>,
    ) {
        match job.class {
            Class::Park => {
                self.park_ops -= 1;
                self.park_bytes -= job.bytes;
            }
            Class::Restore => {
                self.restore_ops -= 1;
                self.restore_bytes -= job.bytes;
            }
        }
        self.reserved_bytes -= job.reserved;
        job.reserved = 0;
        if matches!(job.status, Status::Dispatching | Status::Dispatched(_)) {
            self.dispatched_count -= 1;
        }
        if let Status::Dispatched(inner_id) = job.status {
            self.by_inner.remove(&inner_id);
        }
        if job.kind != JobKind::Flush {
            self.coalesce.close(job.id);
        }
        if publish == PublishOutcome::Indeterminate {
            self.stats.indeterminate_total += 1;
        }
        if matches!(stage, JobStage::Published | JobStage::Durable) && job.kind == JobKind::Store {
            self.published_total += 1;
        }
        match &terminal {
            Terminal::Done(_) => self.stats.terminal_done += 1,
            Terminal::Failed(_) => self.stats.terminal_failed += 1,
            Terminal::Cancelled => {
                self.stats.terminal_cancelled += 1;
                if self.closed {
                    self.shutdown_cancelled += 1;
                }
            }
        }
        if let Some(p) = job.payload.take() {
            if let Some(g) = p.into_guard() {
                self.graveyard.push(g);
            }
        }
        let elapsed = elapsed_us.unwrap_or_else(|| job.submitted_at.elapsed().as_micros() as u64);
        self.push_completion(Completion {
            job: job.id,
            incarnation: job.inc,
            key: job.key,
            stage,
            terminal: Some(terminal),
            publish,
            bytes: job.payload_len,
            elapsed_us: elapsed,
        });
    }

    /// Move queued jobs into the hand-over slot, as many as the dispatch depth
    /// allows. Runs under the lock; never touches the inner store.
    fn hand_over(&mut self) {
        while let Some(h) = self.pop_next_to_dispatch() {
            self.handed.push_back(h);
        }
    }

    fn pop_next_to_dispatch(&mut self) -> Option<HandedJob> {
        if self.paused || self.closed || self.panicked {
            return None;
        }
        if self.dispatched_count >= self.cfg.dispatch_depth.max(1) {
            return None;
        }
        loop {
            let has_r = !self.restore_queue.is_empty();
            let has_p = !self.park_queue.is_empty();
            let class = match (has_r, has_p) {
                (false, false) => return None,
                (true, false) => Class::Restore,
                (false, true) => {
                    self.restore_credits = 3;
                    self.restore_run_with_park_waiting = 0;
                    Class::Park
                }
                (true, true) => {
                    if self.restore_credits > 0 {
                        self.restore_credits -= 1;
                        self.restore_run_with_park_waiting += 1;
                        self.stats.max_restore_run_with_park_waiting = self
                            .stats
                            .max_restore_run_with_park_waiting
                            .max(self.restore_run_with_park_waiting);
                        Class::Restore
                    } else {
                        self.restore_credits = 3;
                        self.restore_run_with_park_waiting = 0;
                        Class::Park
                    }
                }
            };
            let id = match class {
                Class::Restore => self.restore_queue.pop_front()?,
                Class::Park => self.park_queue.pop_front()?,
            };
            let Some(job) = self.jobs.get_mut(&id) else {
                continue;
            };
            let payload = job.payload.take().expect("queued job payload");
            job.status = Status::Dispatching;
            self.dispatched_count += 1;
            match class {
                Class::Restore => self.stats.dispatched_restore += 1,
                Class::Park => self.stats.dispatched_park += 1,
            }
            self.dispatch_log.push((id, class));
            return Some((id, class, job.inc, job.key, payload));
        }
    }
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let (r, garbage) = {
            let mut g = self.lock();
            let r = f(&mut g);
            (r, std::mem::take(&mut g.graveyard))
        };
        drop(garbage);
        r
    }
}

pub struct StorageWorker {
    inner: Arc<dyn KvStore>,
    shared: Arc<Shared>,
}

impl Drop for StorageWorker {
    fn drop(&mut self) {
        let to_cancel = self.shared.with(|s| {
            s.worker_dropped = true;
            s.closed = true;
            s.inner_shutdown_requested = true;
            let mut queued = Vec::new();
            while let Some(id) = s.restore_queue.pop_front() {
                queued.push(id);
            }
            while let Some(id) = s.park_queue.pop_front() {
                queued.push(id);
            }
            for id in queued {
                if let Some(job) = s.jobs.remove(&id) {
                    let stage = job.stage;
                    s.finish_job(
                        job,
                        Terminal::Cancelled,
                        stage,
                        PublishOutcome::NotApplicable,
                        None,
                    );
                }
            }
            let mut disp = Vec::new();
            for j in s.jobs.values_mut() {
                if j.kind != JobKind::Flush && !j.cancelling {
                    if let Status::Dispatched(inner_id) = j.status {
                        j.cancelling = true;
                        disp.push((j.id, inner_id));
                    }
                }
            }
            self.shared.cv.notify_all();
            disp
        });
        for (w_id, inner_id) in to_cancel {
            if self.inner.cancel(inner_id) == CancelOutcome::TooLate {
                self.shared.with(|s| {
                    if let Some(j) = s.jobs.get_mut(&w_id) {
                        j.cancelling = false;
                    }
                });
            }
        }
        self.shared.cv.notify_all();
    }
}

fn validate_store_header(
    ns: &NamespaceId,
    header: &ChunkHeader,
    src: &PinnedSource,
) -> Result<(), PlatformError> {
    if header.namespace_id != *ns
        || header.payload_len == 0
        || src.total_bytes() != Some(header.payload_len)
        || header.block_size == 0
        || src.block_count() == 0
        || src.block_count() > MAX_SEGMENTS
    {
        return Err(PlatformError::InvalidLayout);
    }
    let full = (src.block_count() as u64 - 1) * header.block_size as u64;
    let tail = (header.token_count as u64).checked_sub(full);
    match tail {
        Some(t)
            if t >= 1 && t <= header.block_size as u64 && t == src.tail_valid_tokens() as u64 =>
        {
            Ok(())
        }
        _ => Err(PlatformError::InvalidLayout),
    }
}

fn validate_load_dest(
    key: ChunkKey,
    payload_len: Option<u64>,
    expect: &ChunkHeader,
    dst: &PinnedDest,
) -> Result<u64, PlatformError> {
    let len = payload_len.ok_or(PlatformError::ObjectNotFound(key.0))?;
    if expect.chunk_key != key
        || expect.payload_len != len
        || dst.total_bytes() != Some(len)
        || dst.block_bytes() == 0
    {
        return Err(PlatformError::InvalidLayout);
    }
    Ok(len)
}

fn worker_loop(
    inner: Arc<dyn KvStore>,
    shared: Arc<Shared>,
    mut drive: impl FnMut() + Send + 'static,
) {
    loop {
        let step = catch_unwind(AssertUnwindSafe(|| {
            // 1. Dispatch up to depth.
            while let Some((w_id, _class, inc, key, payload)) = shared.with(|s| {
                s.hand_over();
                s.handed.pop_front()
            }) {
                let res = match payload {
                    Payload::Store { header, src } => inner.submit_store(header, src, inc),
                    Payload::Load { expect, dst } => inner.submit_load(key, &expect, dst, inc),
                    Payload::Flush => inner.submit_flush(inc),
                };
                let mut cancel_inner = None;
                shared.with(|s| match res {
                    Ok(inner_id) => {
                        let Some(job) = s.jobs.get_mut(&w_id) else {
                            return;
                        };
                        job.status = Status::Dispatched(inner_id);
                        s.by_inner.insert(inner_id, w_id);
                        if job.cancelling {
                            cancel_inner = Some(inner_id);
                        }
                    }
                    Err(e) => {
                        s.stats.inner_refusals_total += 1;
                        if let Some(job) = s.jobs.remove(&w_id) {
                            let stage = job.stage;
                            s.finish_job(
                                job,
                                Terminal::Failed(e),
                                stage,
                                PublishOutcome::NotApplicable,
                                None,
                            );
                        }
                        shared.cv.notify_all();
                    }
                });
                if let Some(inner_id) = cancel_inner {
                    let _ = inner.cancel(inner_id);
                }
            }

            // 2. Call inner.shutdown(0) on the worker thread once requested.
            let call_inner_shutdown = shared.with(|s| {
                if s.inner_shutdown_requested && !s.inner_shutdown_called {
                    s.inner_shutdown_called = true;
                    true
                } else {
                    false
                }
            });
            if call_inner_shutdown {
                let _ = inner.shutdown(0);
            }

            // 3. Drive the inner store and reap completions.
            // Drive only when something can progress. A paused, idle worker still polls
            // the store (cheap, and it does not advance the store's clock or start
            // background work), so a stalled store is felt by the worker thread
            // alone and never by callers.
            let should_drive = shared
                .with(|s| !s.paused || s.dispatched_count > 0 || s.closed || s.worker_dropped);
            if should_drive {
                drive();
            }
            // Reap completions.
            {
                let comps = inner.poll(64);
                if !comps.is_empty() {
                    shared.with(|s| {
                        for c in comps {
                            let Some(&w_id) = s.by_inner.get(&c.job) else {
                                s.stats.stale_completions_total += 1;
                                continue;
                            };
                            let Some(job) = s.jobs.get(&w_id) else {
                                s.stats.stale_completions_total += 1;
                                continue;
                            };
                            if job.inc != c.incarnation {
                                s.stats.stale_completions_total += 1;
                                continue;
                            }
                            match &c.terminal {
                                None => {
                                    let job = s.jobs.get_mut(&w_id).expect("job");
                                    if job.progress_seen || c.stage <= job.stage {
                                        s.stats.stale_completions_total += 1;
                                        continue;
                                    }
                                    job.progress_seen = true;
                                    job.stage = c.stage;
                                    if c.stage >= JobStage::Completed && job.kind == JobKind::Store
                                    {
                                        s.reserved_bytes -= job.reserved;
                                        job.reserved = 0;
                                    }
                                    let out = Completion {
                                        job: w_id,
                                        incarnation: job.inc,
                                        key: job.key,
                                        stage: c.stage,
                                        terminal: None,
                                        publish: c.publish,
                                        bytes: c.bytes,
                                        elapsed_us: c.elapsed_us,
                                    };
                                    s.push_completion(out);
                                }
                                Some(term) => {
                                    let job = s.jobs.remove(&w_id).expect("job");
                                    s.finish_job(
                                        job,
                                        term.clone(),
                                        c.stage,
                                        c.publish,
                                        Some(c.elapsed_us),
                                    );
                                }
                            }
                        }
                        shared.cv.notify_all();
                    });
                }
            }
            if call_inner_shutdown {
                shared.with(|_| {
                    shared.cv.notify_all();
                });
            }
        }));

        if step.is_err() {
            shared.with(|s| {
                s.panicked = true;
                s.closed = true;
                s.restore_queue.clear();
                s.park_queue.clear();
                let ids: Vec<JobId> = s.jobs.keys().copied().collect();
                for id in ids {
                    if let Some(job) = s.jobs.remove(&id) {
                        let stage = job.stage;
                        s.finish_job(
                            job,
                            Terminal::Failed(PlatformError::HardwareFault(String::from(
                                "storage worker panicked",
                            ))),
                            stage,
                            PublishOutcome::NotApplicable,
                            None,
                        );
                    }
                }
                shared.cv.notify_all();
            });
            return;
        }

        // Exit once dropped and all dispatched jobs have finished.
        let (done, idle_wait) = shared.with(|s| {
            if s.worker_dropped && s.dispatched_count == 0 {
                return (true, None);
            }
            let can_dispatch = !s.paused
                && !s.closed
                && s.dispatched_count < s.cfg.dispatch_depth.max(1)
                && (!s.restore_queue.is_empty() || !s.park_queue.is_empty());
            if can_dispatch || s.dispatched_count > 0 {
                (false, Some(s.cfg.idle_poll))
            } else {
                (false, Some(s.cfg.idle_poll.max(Duration::from_micros(200))))
            }
        });
        if done {
            return;
        }
        if let Some(dur) = idle_wait {
            if dur.is_zero() {
                thread::yield_now();
            } else {
                let g = shared.lock();
                if !g.worker_dropped
                    && (!g.inner_shutdown_requested || g.inner_shutdown_called)
                    && (g.paused || (g.restore_queue.is_empty() && g.park_queue.is_empty()))
                    && g.dispatched_count == 0
                {
                    let _ = shared.cv.wait_timeout(g, dur);
                }
            }
        }
    }
}

impl StorageWorker {
    pub fn spawn(
        inner: Arc<dyn KvStore>,
        config: WorkerConfig,
        drive: impl FnMut() + Send + 'static,
    ) -> Result<StorageWorker, PlatformError> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::new(config)),
            cv: Condvar::new(),
        });
        let inner_w = inner.clone();
        let shared_w = shared.clone();
        thread::Builder::new()
            .name(String::from("aien-kv-worker"))
            .spawn(move || worker_loop(inner_w, shared_w, drive))
            .map_err(|_| PlatformError::DeviceUnavailable)?;
        Ok(StorageWorker { inner, shared })
    }

    pub fn pause_dispatch(&self) {
        self.shared.with(|s| {
            s.paused = true;
        });
    }

    pub fn resume_dispatch(&self) {
        self.shared.with(|s| {
            s.paused = false;
            s.hand_over();
            self.shared.cv.notify_all();
        });
    }

    pub fn submit_load_coalesced(
        &self,
        key: ChunkKey,
        expect: &ChunkHeader,
        make_dst: impl FnOnce() -> PinnedDest,
        inc: Incarnation,
    ) -> Result<LoadTicket, PlatformError> {
        let cap = self.inner.capacity();
        // Fast path: attach to an in-flight load if one exists.
        let attached = self.shared.with(|s| {
            s.check_store_state(cap.state, false)?;
            if let Some(job) = s.coalesce.find(Share::Load, &key) {
                match s.coalesce.attach(job) {
                    Ok(_) => {
                        s.stats.coalesced_total += 1;
                        return Ok(Some(LoadTicket::Attached(job)));
                    }
                    Err(_) => {
                        s.stats.attach_refused_total += 1;
                        return Err(s.queue_full());
                    }
                }
            }
            Ok(None)
        })?;
        if let Some(t) = attached {
            return Ok(t);
        }

        let found = self.inner.lookup(&[key]);
        let payload_len = found.into_iter().next().flatten();
        let mut dst_slot = Some(make_dst());
        let res = self.shared.with(|s| {
            s.check_store_state(cap.state, false)?;
            if let Some(job) = s.coalesce.find(Share::Load, &key) {
                match s.coalesce.attach(job) {
                    Ok(_) => {
                        s.stats.coalesced_total += 1;
                        return Ok(LoadTicket::Attached(job));
                    }
                    Err(_) => {
                        s.stats.attach_refused_total += 1;
                        return Err(s.queue_full());
                    }
                }
            }
            let dst = dst_slot.as_ref().ok_or(PlatformError::InvalidLayout)?;
            let len = validate_load_dest(key, payload_len, expect, dst)?;
            let bytes = len + HEADER_REGION;
            s.credit_check(Class::Restore, bytes)?;
            let id = JobId(s.next_job);
            s.next_job += 1;
            s.restore_ops += 1;
            s.restore_bytes += bytes;
            s.stats.submitted_restore += 1;
            s.restore_queue.push_back(id);
            s.stats.peak_restore_queue =
                s.stats.peak_restore_queue.max(s.restore_queue.len() as u64);
            s.coalesce.open(Share::Load, &key, id);
            let dst = dst_slot.take().expect("dst");
            s.jobs.insert(
                id,
                WorkerJob {
                    id,
                    class: Class::Restore,
                    kind: JobKind::Load,
                    inc,
                    key,
                    bytes,
                    payload_len: len,
                    reserved: 0,
                    submitted_at: Instant::now(),
                    stage: JobStage::Submitted,
                    status: Status::Queued,
                    cancelling: false,
                    payload: Some(Payload::Load {
                        expect: *expect,
                        dst,
                    }),
                    progress_seen: false,
                },
            );
            self.shared.cv.notify_all();
            Ok(LoadTicket::Started(id))
        });
        drop(dst_slot);
        res
    }

    pub fn load_in_flight(&self, key: &ChunkKey) -> Option<JobId> {
        self.shared.with(|s| s.coalesce.find(Share::Load, key))
    }

    pub fn stats(&self) -> WorkerStats {
        self.shared.with(|s| s.stats.clone())
    }

    pub fn dispatch_order(&self) -> Vec<(JobId, Class)> {
        self.shared.with(|s| s.dispatch_log.clone())
    }

    pub fn inner_job(&self, job: JobId) -> Option<JobId> {
        self.shared.with(|s| match s.jobs.get(&job)?.status {
            Status::Dispatched(inner_id) => Some(inner_id),
            _ => None,
        })
    }

    pub fn state_lock_is_free(&self) -> bool {
        match self.shared.state.try_lock() {
            Ok(_) => true,
            Err(TryLockError::Poisoned(_)) => true,
            Err(TryLockError::WouldBlock) => false,
        }
    }
}

impl KvStore for StorageWorker {
    fn namespace(&self) -> NamespaceId {
        self.inner.namespace()
    }

    fn capacity(&self) -> Capacity {
        let mut cap = self.inner.capacity();
        self.shared.with(|s| {
            if s.panicked {
                cap.state = StoreState::Failed;
            }
            cap.reserved_bytes = s.reserved_bytes;
            cap.restore_ops = s.restore_ops;
            cap.max_restore_ops = s.cfg.max_restore_ops;
            cap.park_ops = s.park_ops;
            cap.max_park_ops = s.cfg.max_park_ops;
            cap.inflight_bytes = s.restore_bytes + s.park_bytes;
            cap.max_inflight_bytes = s.cfg.max_inflight_bytes;
            cap.max_waiters_per_job = s.cfg.max_waiters_per_job;
        });
        cap
    }

    fn lookup(&self, keys: &[ChunkKey]) -> Vec<Option<u64>> {
        self.inner.lookup(keys)
    }

    fn submit_store(
        &self,
        header: ChunkHeader,
        src: PinnedSource,
        inc: Incarnation,
    ) -> Result<JobId, PlatformError> {
        let ns = self.inner.namespace();
        let cap = self.inner.capacity();
        let mut src_slot = Some(src);
        let res = self.shared.with(|s| {
            s.check_store_state(cap.state, true)?;
            let src_ref = src_slot.as_ref().ok_or(PlatformError::InvalidLayout)?;
            validate_store_header(&ns, &header, src_ref)?;
            let key = header.chunk_key;
            if let Some(existing) = s.coalesce.find(Share::Store, &key) {
                let job_ok = s.jobs.get(&existing).is_some_and(|j| !j.cancelling);
                if job_ok && s.coalesce.attach(existing).is_ok() {
                    s.stats.store_dedup_total += 1;
                    return Ok(existing);
                }
            }
            let bytes = header.payload_len + HEADER_REGION;
            s.credit_check(Class::Park, bytes)?;
            let id = JobId(s.next_job);
            s.next_job += 1;
            s.park_ops += 1;
            s.park_bytes += bytes;
            s.reserved_bytes += bytes;
            s.stats.submitted_park += 1;
            s.park_queue.push_back(id);
            s.stats.peak_park_queue = s.stats.peak_park_queue.max(s.park_queue.len() as u64);
            s.coalesce.open(Share::Store, &key, id);
            let src = src_slot.take().expect("src");
            s.jobs.insert(
                id,
                WorkerJob {
                    id,
                    class: Class::Park,
                    kind: JobKind::Store,
                    inc,
                    key,
                    bytes,
                    payload_len: header.payload_len,
                    reserved: bytes,
                    submitted_at: Instant::now(),
                    stage: JobStage::Submitted,
                    status: Status::Queued,
                    cancelling: false,
                    payload: Some(Payload::Store { header, src }),
                    progress_seen: false,
                },
            );
            self.shared.cv.notify_all();
            Ok(id)
        });
        drop(src_slot);
        res
    }

    fn submit_load(
        &self,
        key: ChunkKey,
        expect: &ChunkHeader,
        dst: PinnedDest,
        inc: Incarnation,
    ) -> Result<JobId, PlatformError> {
        let cap = self.inner.capacity();
        let found = self.inner.lookup(&[key]);
        let payload_len = found.into_iter().next().flatten();
        let mut dst_slot = Some(dst);
        let res = self.shared.with(|s| {
            s.check_store_state(cap.state, false)?;
            let dst_ref = dst_slot.as_ref().ok_or(PlatformError::InvalidLayout)?;
            let len = validate_load_dest(key, payload_len, expect, dst_ref)?;
            let bytes = len + HEADER_REGION;
            s.credit_check(Class::Restore, bytes)?;
            let id = JobId(s.next_job);
            s.next_job += 1;
            s.restore_ops += 1;
            s.restore_bytes += bytes;
            s.stats.submitted_restore += 1;
            s.restore_queue.push_back(id);
            s.stats.peak_restore_queue =
                s.stats.peak_restore_queue.max(s.restore_queue.len() as u64);
            s.coalesce.open(Share::Load, &key, id);
            let dst = dst_slot.take().expect("dst");
            s.jobs.insert(
                id,
                WorkerJob {
                    id,
                    class: Class::Restore,
                    kind: JobKind::Load,
                    inc,
                    key,
                    bytes,
                    payload_len: len,
                    reserved: 0,
                    submitted_at: Instant::now(),
                    stage: JobStage::Submitted,
                    status: Status::Queued,
                    cancelling: false,
                    payload: Some(Payload::Load {
                        expect: *expect,
                        dst,
                    }),
                    progress_seen: false,
                },
            );
            self.shared.cv.notify_all();
            Ok(id)
        });
        drop(dst_slot);
        res
    }

    fn submit_flush(&self, inc: Incarnation) -> Result<JobId, PlatformError> {
        let cap = self.inner.capacity();
        self.shared.with(|s| {
            s.check_store_state(cap.state, false)?;
            s.credit_check(Class::Park, 0)?;
            let id = JobId(s.next_job);
            s.next_job += 1;
            s.park_ops += 1;
            s.stats.submitted_park += 1;
            s.park_queue.push_back(id);
            s.stats.peak_park_queue = s.stats.peak_park_queue.max(s.park_queue.len() as u64);
            s.jobs.insert(
                id,
                WorkerJob {
                    id,
                    class: Class::Park,
                    kind: JobKind::Flush,
                    inc,
                    key: ChunkKey(ObjectId([0; 32])),
                    bytes: 0,
                    payload_len: 0,
                    reserved: 0,
                    submitted_at: Instant::now(),
                    stage: JobStage::Submitted,
                    status: Status::Queued,
                    cancelling: false,
                    payload: Some(Payload::Flush),
                    progress_seen: false,
                },
            );
            self.shared.cv.notify_all();
            Ok(id)
        })
    }

    fn attach_waiter(&self, job: JobId) -> Result<(), PlatformError> {
        self.shared.with(|s| {
            let is_load = s
                .jobs
                .get(&job)
                .is_some_and(|j| j.kind == JobKind::Load && !j.cancelling);
            if !is_load {
                return Err(PlatformError::TaskRejected(String::from(
                    "not an in-flight load",
                )));
            }
            match s.coalesce.attach(job) {
                Ok(_) => Ok(()),
                Err(AttachRefused::JobFull | AttachRefused::TotalFull) => {
                    s.stats.attach_refused_total += 1;
                    Err(s.queue_full())
                }
                Err(AttachRefused::NoSuchJob) => Err(PlatformError::TaskRejected(String::from(
                    "not an in-flight load",
                ))),
            }
        })
    }

    fn poll(&self, max: usize) -> Vec<Completion> {
        self.shared.with(|s| {
            let n = max.min(s.completions.len());
            s.completions.drain(..n).collect()
        })
    }

    fn cancel(&self, job: JobId) -> CancelOutcome {
        let action = self.shared.with(|s| {
            let Some(j) = s.jobs.get(&job) else {
                return Err(CancelOutcome::TooLate);
            };
            if j.kind == JobKind::Flush {
                return Err(CancelOutcome::TooLate);
            }
            if j.cancelling {
                return Err(CancelOutcome::Cancelled);
            }
            if matches!(j.status, Status::Dispatching) {
                return Err(CancelOutcome::TooLate);
            }
            match s.coalesce.detach(job) {
                Detach::Remaining(_) => return Err(CancelOutcome::TooLate),
                Detach::Unknown | Detach::Last => {}
            }
            let j = s.jobs.get_mut(&job).expect("job");
            match j.status {
                Status::Queued => {
                    let id = j.id;
                    let class = j.class;
                    match class {
                        Class::Restore => s.restore_queue.retain(|&x| x != id),
                        Class::Park => s.park_queue.retain(|&x| x != id),
                    }
                    let job = s.jobs.remove(&id).expect("queued job");
                    let stage = job.stage;
                    s.finish_job(
                        job,
                        Terminal::Cancelled,
                        stage,
                        PublishOutcome::NotApplicable,
                        None,
                    );
                    self.shared.cv.notify_all();
                    Err(CancelOutcome::Cancelled)
                }
                Status::Dispatched(inner_id) => {
                    j.cancelling = true;
                    Ok(inner_id)
                }
                Status::Dispatching => Err(CancelOutcome::TooLate),
            }
        });
        match action {
            Err(outcome) => outcome,
            Ok(inner_id) => {
                let out = self.inner.cancel(inner_id);
                if out == CancelOutcome::TooLate {
                    self.shared.with(|s| {
                        if let Some(j) = s.jobs.get_mut(&job) {
                            j.cancelling = false;
                        }
                    });
                } else {
                    self.shared.cv.notify_all();
                }
                out
            }
        }
    }

    fn pin(&self, keys: &[ChunkKey]) -> Result<PinToken, PlatformError> {
        self.inner.pin(keys)
    }

    fn unpin(&self, token: PinToken) {
        self.inner.unpin(token);
    }

    fn delete(&self, keys: &[ChunkKey]) -> u64 {
        let filtered: Vec<ChunkKey> = self.shared.with(|s| {
            keys.iter()
                .copied()
                .filter(|k| !s.coalesce.has(Share::Load, k) && !s.coalesce.has(Share::Store, k))
                .collect()
        });
        if filtered.is_empty() {
            return 0;
        }
        self.inner.delete(&filtered)
    }

    fn recover_step(&self, slice_ms: u64) -> RecoveryProgress {
        self.inner.recover_step(slice_ms)
    }

    fn shutdown(&self, intake_deadline_ms: u64) -> DrainReport {
        let deadline = Instant::now() + Duration::from_millis(intake_deadline_ms);
        let to_cancel = self.shared.with(|s| {
            s.closed = true;
            s.inner_shutdown_requested = true;
            if s.shutdown_published_before.is_none() {
                s.shutdown_published_before = Some(s.published_total);
            }
            let mut queued = Vec::new();
            while let Some(id) = s.restore_queue.pop_front() {
                queued.push(id);
            }
            while let Some(id) = s.park_queue.pop_front() {
                queued.push(id);
            }
            for id in queued {
                if let Some(job) = s.jobs.remove(&id) {
                    let stage = job.stage;
                    s.finish_job(
                        job,
                        Terminal::Cancelled,
                        stage,
                        PublishOutcome::NotApplicable,
                        None,
                    );
                }
            }
            let mut disp = Vec::new();
            for j in s.jobs.values() {
                if let Status::Dispatched(inner_id) = j.status {
                    let cancellable = match j.kind {
                        JobKind::Flush => false,
                        JobKind::Load => true,
                        JobKind::Store => j.stage < JobStage::Completed,
                    };
                    if cancellable && !j.cancelling {
                        disp.push((j.id, inner_id));
                    }
                }
            }
            self.shared.cv.notify_all();
            disp
        });

        for (w_id, inner_id) in to_cancel {
            if self.inner.cancel(inner_id) == CancelOutcome::Cancelled {
                self.shared.with(|s| {
                    if let Some(j) = s.jobs.get_mut(&w_id) {
                        if !j.cancelling {
                            j.cancelling = true;
                        }
                    }
                });
            }
        }
        self.shared.cv.notify_all();

        loop {
            let done = self
                .shared
                .with(|s| s.jobs.is_empty() && s.inner_shutdown_called);
            if done {
                break;
            }
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let wait = (deadline - now).min(Duration::from_millis(5));
            let g = self.shared.lock();
            if !g.jobs.is_empty() || !g.inner_shutdown_called {
                let _ = self.shared.cv.wait_timeout(g, wait);
            }
        }

        self.shared.with(|s| {
            let unfinished: Vec<UnfinishedJob> = s
                .jobs
                .values()
                .map(|j| UnfinishedJob {
                    job: j.id,
                    key: j.key,
                    bytes: j.bytes,
                    stage: j.stage,
                })
                .collect();
            s.stats.shutdown_unfinished_jobs = unfinished.len() as u64;
            s.stats.shutdown_unfinished_bytes = unfinished.iter().map(|u| u.bytes).sum();
            let base = s.shutdown_published_before.unwrap_or(s.published_total);
            DrainReport {
                published: s.published_total.saturating_sub(base),
                cancelled: s.shutdown_cancelled,
                unfinished,
            }
        })
    }
}

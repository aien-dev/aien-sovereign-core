//! Landing-buffer coalescing table (ADR 0003 section 7.3, review finding 6).
//!
//! Same-key loads coalesce at the **job and landing-buffer level**: one
//! in-flight load per chunk key lands in that chunk's holder, later requests
//! register as waiters on the job. The table is pure bookkeeping (no I/O, no
//! locks); the worker keeps it inside its own critical section.
//!
//! Counting rules (consistent with the deterministic fake, C2):
//!
//! * The request that opened the job is waiter 1, so `max_waiters_per_job`
//!   counts it; attaching is refused once `waiters >= max_waiters_per_job`.
//! * `max_waiters_total` bounds the **attached** waiters (waiters beyond the
//!   first) over all jobs.
//! * Stores share the table to deduplicate a store of a key that is already
//!   in flight: the duplicate is an additional owner of the same job, so one
//!   owner cancelling cannot cancel the others' store.

use crate::identity::ChunkKey;
use crate::store::JobId;
use alloc::collections::BTreeMap;

/// What kind of work a key is shared for. Loads and stores of one key never
/// share a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Share {
    Load,
    Store,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoalesceLimits {
    /// ADR 7.1: default 16 (the settings table lists 8, see the C5 report).
    pub max_waiters_per_job: u32,
    /// ADR 7.1: default 1024.
    pub max_waiters_total: u32,
}

impl Default for CoalesceLimits {
    fn default() -> Self {
        Self {
            max_waiters_per_job: 16,
            max_waiters_total: 1024,
        }
    }
}

/// Why `attach` refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachRefused {
    /// No such job, or every waiter already left (the job is being torn down).
    NoSuchJob,
    /// The job already has `max_waiters_per_job` waiters.
    JobFull,
    /// `max_waiters_total` attached waiters exist over all jobs.
    TotalFull,
}

/// What `detach` left behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detach {
    /// The job is not in the table (a flush, or already closed).
    Unknown,
    /// Other waiters remain: the shared work must go on.
    Remaining(u32),
    /// That was the last waiter: nobody wants the job any more.
    Last,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    share: Share,
    key: [u8; 32],
    waiters: u32,
}

/// Key to job bookkeeping with per-job and total waiter bounds.
#[derive(Debug, Clone)]
pub struct CoalesceTable {
    limits: CoalesceLimits,
    by_key: BTreeMap<(Share, [u8; 32]), JobId>,
    jobs: BTreeMap<JobId, Entry>,
    attached_total: u32,
}

impl CoalesceTable {
    pub fn new(limits: CoalesceLimits) -> Self {
        let _ = limits;
        unimplemented!("C5 green")
    }

    pub fn limits(&self) -> CoalesceLimits {
        unimplemented!("C5 green")
    }

    /// Register `job` as the owner of `(share, key)` with its opener as waiter 1.
    /// If another job with waiters already owns the key, that job stays the
    /// coalescing target; `job` is still tracked for `attach`/`detach`/`close`.
    pub fn open(&mut self, share: Share, key: &ChunkKey, job: JobId) {
        let _ = (share, key, job);
        unimplemented!("C5 green")
    }

    /// The job a new request for `(share, key)` may join: it exists and still
    /// has at least one waiter.
    pub fn find(&self, share: Share, key: &ChunkKey) -> Option<JobId> {
        let _ = (share, key);
        unimplemented!("C5 green")
    }

    /// True while any job (even one whose waiters all left) is registered for the key.
    pub fn has(&self, share: Share, key: &ChunkKey) -> bool {
        let _ = (share, key);
        unimplemented!("C5 green")
    }

    /// Add one waiter; returns the waiter count now. Bounded.
    pub fn attach(&mut self, job: JobId) -> Result<u32, AttachRefused> {
        let _ = job;
        unimplemented!("C5 green")
    }

    /// Remove one waiter (the caller cancelled its own interest).
    pub fn detach(&mut self, job: JobId) -> Detach {
        let _ = job;
        unimplemented!("C5 green")
    }

    /// The job reached its terminal completion: forget it and release its
    /// attached waiters from the total.
    pub fn close(&mut self, job: JobId) {
        let _ = job;
        unimplemented!("C5 green")
    }

    pub fn waiters(&self, job: JobId) -> u32 {
        let _ = job;
        unimplemented!("C5 green")
    }

    /// Attached waiters (beyond each job's first) over all jobs.
    pub fn attached_total(&self) -> u32 {
        unimplemented!("C5 green")
    }

    pub fn jobs(&self) -> usize {
        unimplemented!("C5 green")
    }
}

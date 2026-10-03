//! Jobs: what the operator wants done.

use serde::Deserialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    Code,
    Review,
    Research,
}

impl JobKind {
    pub const ALL: [JobKind; 3] = [JobKind::Code, JobKind::Review, JobKind::Research];

    pub fn as_str(self) -> &'static str {
        match self {
            JobKind::Code => "code",
            JobKind::Review => "review",
            JobKind::Research => "research",
        }
    }
}

impl fmt::Display for JobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Rough size of the job. v1 routing does not use it yet; it is carried so
/// the type is stable when size-aware rules arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JobSize {
    Small,
    Medium,
    Large,
}

/// `needs` (what a job requires of an account) is deliberately absent: no
/// first-party source on this machine says what each program can or cannot
/// do, so adding it now would be a guess. UNKNOWN, tracked for a later cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub kind: JobKind,
    pub size: JobSize,
}

impl Job {
    pub fn new(kind: JobKind, size: JobSize) -> Self {
        Job { kind, size }
    }
}

//! Task execution states for the provisional scheduler (Headstart M2,
//! issue #535).
//!
//! ```text
//!   Blocked --dispatch--> Provisional --work done--> AwaitingFinalProof --finalize--> Committable
//!      ^                      |                           |                               |
//!      +------ revoke --------+---------------------------+-------------------------------+
//!   any non-Aborted --cancel / failure / lease expiry--> Aborted --retry--> Blocked
//! ```
//!
//! There is no `Committed` state. `Committable` means "every dependency is
//! final-verified for its current generation"; committing is done by the
//! existing effect/World machinery in aien-runtime, which this crate does not
//! touch.
//!
//! Revocation returns a task to `Blocked` rather than `Aborted`. The task did
//! nothing wrong: an upstream producer failed, changed its interface or
//! started a new generation. Its provisional result is discarded and it waits
//! to be dispatched again against the new upstream state, without a manual
//! retry. A task fails into `Aborted` only for its own reasons (its own
//! failure, cancellation, lost lease).
//!
//! Only `Provisional` holds resource permits. Leaving `Provisional` releases
//! them, so `Blocked`, `AwaitingFinalProof`, `Committable` and `Aborted`
//! tasks hold none.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TaskState {
    Blocked,
    Provisional,
    AwaitingFinalProof,
    Committable,
    Aborted,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskState::Blocked => "blocked",
            TaskState::Provisional => "provisional",
            TaskState::AwaitingFinalProof => "awaiting_final_proof",
            TaskState::Committable => "committable",
            TaskState::Aborted => "aborted",
        }
    }

    /// States whose work or proof is built on upstream artifacts and must be
    /// revoked when upstream changes.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            TaskState::Provisional | TaskState::AwaitingFinalProof | TaskState::Committable
        )
    }

    /// Only a running task holds resource permits.
    pub fn holds_permits(self) -> bool {
        self == TaskState::Provisional
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IllegalTransition {
    pub from: TaskState,
    pub to: TaskState,
}

impl fmt::Display for IllegalTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "illegal task transition {} -> {}",
            self.from.as_str(),
            self.to.as_str()
        )
    }
}

impl std::error::Error for IllegalTransition {}

/// The complete transition table. Anything not listed is refused.
pub fn check_transition(from: TaskState, to: TaskState) -> Result<(), IllegalTransition> {
    use TaskState::*;
    let ok = matches!(
        (from, to),
        (Blocked, Provisional)
            | (Provisional, AwaitingFinalProof)
            | (AwaitingFinalProof, Committable)
            | (Provisional | AwaitingFinalProof | Committable, Blocked)
            | (
                Blocked | Provisional | AwaitingFinalProof | Committable,
                Aborted
            )
            | (Aborted, Blocked)
    );
    if ok {
        Ok(())
    } else {
        Err(IllegalTransition { from, to })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TaskState::*;

    #[test]
    fn table_refuses_shortcuts() {
        for (from, to) in [
            (Blocked, AwaitingFinalProof),
            (Blocked, Committable),
            (Provisional, Committable),
            (Aborted, Provisional),
            (Aborted, Committable),
            (Committable, Provisional),
            (Blocked, Blocked),
        ] {
            assert!(check_transition(from, to).is_err(), "{from:?} -> {to:?}");
        }
        for (from, to) in [
            (Blocked, Provisional),
            (Provisional, AwaitingFinalProof),
            (AwaitingFinalProof, Committable),
            (Committable, Blocked),
            (Aborted, Blocked),
        ] {
            assert!(check_transition(from, to).is_ok(), "{from:?} -> {to:?}");
        }
    }
}

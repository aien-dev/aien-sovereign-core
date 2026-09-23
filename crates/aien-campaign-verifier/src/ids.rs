use serde::{Deserialize, Serialize};
use std::fmt;

/// Runtime incarnation handle as serialized in campaign journals.
///
/// Opaque and 128 bits. Equality compares every field. There is deliberately no
/// conversion to a narrower integer (SPEC.md Section 2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SequenceId {
    pub boot_epoch: u64,
    pub slot: u32,
    pub generation: u32,
}

impl fmt::Display for SequenceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.boot_epoch, self.slot, self.generation)
    }
}

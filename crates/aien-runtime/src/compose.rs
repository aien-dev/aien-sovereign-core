//! The compose bridge (NEXT-PHASE-1 cut 1b): omega COMPOSITION-2 through
//! aien-omega-compose. A goal becomes one proposed file change, checked,
//! recorded in the Cortex journal and committed (or refused) by AEGIS.
//! One composition home per process, opened on first use and kept open: the
//! Cortex journal takes an exclusive writer lock.
//!
//! Lifted out of `spine.rs`, which keeps the runtime daemon's step loop.

mod bridge;

pub use bridge::*;

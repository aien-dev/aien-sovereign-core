//! The compose bridge (NEXT-PHASE-1 cut 1b): omega COMPOSITION-2 through
//! aien-omega-compose. A goal becomes one proposed file change, checked,
//! recorded in the Cortex journal and committed (or refused) by AEGIS.
//! One composition home per process, opened on first use and kept open: the
//! Cortex journal takes an exclusive writer lock.
//!
//! Lifted out of `spine.rs`, which keeps the runtime daemon's step loop.

mod bridge;

pub use bridge::*;

/// The exact bytes the bridge hands the model and the proposal it makes of a
/// reply, for offline re-derivation tools that reproduce a recorded run
/// without a compose home (examples/np1_reference.rs, np1_edit_merge.rs,
/// qwen3_8b_specverify.rs) and for the prompt-size measurements. They
/// reproduce; they do not run a task: callers and tests run tasks through
/// [`ComposeBridge`](crate::compose::ComposeBridge).
pub mod rederive {
    pub use super::bridge::{
        classify_destination, compose_assistant_prefix, corrective_retry_prompt, edit_proposal,
        new_document_block, proposal_prompt, retry_prompt, task_decision, task_prompt,
        ProposalKind, TargetClass, TaskPrompt, COMPOSE_ASSISTANT_PREFIX,
    };
}

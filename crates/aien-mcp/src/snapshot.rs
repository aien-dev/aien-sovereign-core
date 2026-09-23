use std::sync::Arc;
use std::time::SystemTime;

use aien_capability::{Digest32, ProviderId, ToolDescriptor};

/// Immutable view J-Space plans against. It is not a session handle.
#[derive(Clone, Debug)]
pub struct CapabilitySnapshot {
    pub provider: ProviderId,
    pub session_epoch: u64,
    pub catalog_digest: Digest32,
    pub tools: Arc<[ToolDescriptor]>,
    pub generated_at: SystemTime,
    pub expires_at: SystemTime,
}

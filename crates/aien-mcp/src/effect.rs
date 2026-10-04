use aien_capability::{Digest32, EffectId, JNodeId, ProviderId, WorldId};
use serde_json::Value;

/// Irreversible work J-Space may record. Staging does not call the provider.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectIntent {
    pub provider: ProviderId,
    pub tool_name: String,
    pub arguments: Value,
    pub capability_digest: Digest32,
}

/// Result of one speculative-safe `tools/call`.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolResult {
    pub output: Value,
}

/// Proof that a committed effect finished. The output is the provider result,
/// not a credential.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectReceipt {
    pub effect_id: EffectId,
    pub world_id: WorldId,
    pub winning_jnode: JNodeId,
    pub policy_digest: Digest32,
    pub provider: ProviderId,
    pub tool_name: String,
    pub capability_digest: Digest32,
    pub output: Value,
}

/// An effect the policy boundary has allowed.
///
/// The only production mint is crate-private and reachable solely through
/// [`crate::EffectLane::authorize`], which asks an [`crate::EffectAuthority`]. Fields stay
/// private, so another crate cannot build the value with a struct literal:
///
/// ```compile_fail
/// use aien_capability::{Digest32, EffectId, JNodeId, WorldId};
/// use aien_mcp::AuthorizedEffect;
/// let _ = AuthorizedEffect {
///     intent: (),
///     world_id: WorldId(1),
///     winning_jnode: JNodeId(1),
///     policy_digest: Digest32::of(b"x"),
///     capability_digest: Digest32::of(b"x"),
///     idempotency_key: EffectId::from_label("x"),
/// };
/// ```
///
/// A bare intent cannot be executed:
///
/// ```compile_fail
/// use aien_mcp::{EffectIntent, EffectLane, McpBroker};
/// async fn f(lane: EffectLane, intent: EffectIntent) {
///     let _ = lane.execute_effect(intent).await;
/// }
/// ```
#[derive(Clone, Debug)]
pub struct AuthorizedEffect<T> {
    intent: T,
    world_id: WorldId,
    winning_jnode: JNodeId,
    policy_digest: Digest32,
    capability_digest: Digest32,
    idempotency_key: EffectId,
}

impl<T> AuthorizedEffect<T> {
    pub fn intent(&self) -> &T {
        &self.intent
    }

    pub fn world_id(&self) -> WorldId {
        self.world_id
    }

    pub fn winning_jnode(&self) -> JNodeId {
        self.winning_jnode
    }

    pub fn policy_digest(&self) -> Digest32 {
        self.policy_digest
    }

    pub fn capability_digest(&self) -> Digest32 {
        self.capability_digest
    }

    pub fn idempotency_key(&self) -> EffectId {
        self.idempotency_key
    }
}

/// Test-only mint. Absent from production builds.
#[cfg(test)]
pub(crate) fn authorize_for_test<T>(
    intent: T,
    world_id: WorldId,
    winning_jnode: JNodeId,
    policy_digest: Digest32,
    capability_digest: Digest32,
    idempotency_key: EffectId,
) -> AuthorizedEffect<T> {
    AuthorizedEffect {
        intent,
        world_id,
        winning_jnode,
        policy_digest,
        capability_digest,
        idempotency_key,
    }
}

/// Production mint. Crate-private: reached only from `EffectLane::authorize` after an
/// `EffectAuthority` returned `Allow` or `AllowRestricted`.
pub(crate) fn mint<T>(
    intent: T,
    world_id: WorldId,
    winning_jnode: JNodeId,
    policy_digest: Digest32,
    capability_digest: Digest32,
    idempotency_key: EffectId,
) -> AuthorizedEffect<T> {
    AuthorizedEffect {
        intent,
        world_id,
        winning_jnode,
        policy_digest,
        capability_digest,
        idempotency_key,
    }
}

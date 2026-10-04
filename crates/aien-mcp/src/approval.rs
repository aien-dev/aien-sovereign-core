//! Single-use approvals for the host-owned authority seam.
//!
//! When an [`crate::EffectAuthority`] answers `RequireApproval`, a host approver (a human or an
//! approval service, holding an [`ApprovalDesk`]) may issue one [`ApprovalGrant`]. The grant is
//! bound to one effect identity: the intent digest (tool, arguments, catalog), the idempotency
//! key, the world and the winning J-node. [`crate::EffectLane::authorize_approved`] spends the
//! grant when it mints the `AuthorizedEffect`. A second mint with the same grant is refused.
//!
//! Replay of a finished request does not need a second approval: the existing effect ledger
//! (keyed by the idempotency key) already returns the original receipt, and
//! [`crate::EffectLane::authorize_and_execute_approved`] consults it for a spent grant that is
//! bound to the same effect.
//!
//! Expiry uses a host-supplied `now` and `expires_at` in the same unit. The crate reads no clock.

use aien_capability::{Digest32, EffectId, JNodeId, WorldId};

use crate::authority::{intent_digest, EffectScope};
use crate::broker::McpBroker;
use crate::EffectIntent;

/// Why an approval could not be spent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalError {
    /// The grant was not issued by this broker's desk.
    Unknown,
    /// The grant is bound to a different effect than the one presented. Nothing is spent.
    Mismatch,
    /// The grant was already spent by an earlier mint.
    Consumed,
    /// `now` is at or past `expires_at`. Nothing is spent.
    Expired,
}

/// Proof that a host approver approved exactly one effect. Fields are private and only an
/// [`ApprovalDesk`] can create one. Cloning does not duplicate it: spending is tracked by the
/// broker, not by the value.
#[derive(Clone, Debug)]
pub struct ApprovalGrant {
    id: Digest32,
}

#[derive(Debug)]
pub(crate) struct ApprovalRecord {
    pub(crate) intent_digest: Digest32,
    pub(crate) idempotency_key: EffectId,
    pub(crate) world_id: WorldId,
    pub(crate) winning_jnode: JNodeId,
    pub(crate) expires_at: u64,
    pub(crate) consumed: bool,
}

impl ApprovalRecord {
    pub(crate) fn binds(&self, intent: &EffectIntent, scope: &EffectScope) -> bool {
        self.intent_digest == intent_digest(intent)
            && self.idempotency_key == scope.idempotency_key
            && self.world_id == scope.world_id
            && self.winning_jnode == scope.winning_jnode
    }
}

impl ApprovalGrant {
    pub(crate) fn id(&self) -> Digest32 {
        self.id
    }
}

/// The approver's handle. Held by the host's approval service, not by J-Space or the effect
/// lane's callers. Limit: any code holding a `McpBroker` or `SessionManager` can obtain one, so
/// the separation is by handle discipline, not by type.
#[derive(Clone)]
pub struct ApprovalDesk {
    broker: McpBroker,
}

impl ApprovalDesk {
    pub fn new(broker: McpBroker) -> Self {
        Self { broker }
    }

    /// Issue one grant for exactly this intent and scope, valid while `now < expires_at`.
    pub fn issue(
        &self,
        intent: &EffectIntent,
        scope: EffectScope,
        expires_at: u64,
    ) -> ApprovalGrant {
        let mut inner = self.broker.lock();
        inner.next_approval = inner.next_approval.saturating_add(1);
        let digest = intent_digest(intent);
        let mut bytes = inner.next_approval.to_le_bytes().to_vec();
        bytes.extend_from_slice(&digest.0);
        bytes.extend_from_slice(&scope.idempotency_key.0);
        let id = Digest32::of(&bytes);
        inner.approvals.insert(
            id,
            ApprovalRecord {
                intent_digest: digest,
                idempotency_key: scope.idempotency_key,
                world_id: scope.world_id,
                winning_jnode: scope.winning_jnode,
                expires_at,
                consumed: false,
            },
        );
        ApprovalGrant { id }
    }
}

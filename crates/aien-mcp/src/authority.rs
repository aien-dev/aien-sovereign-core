//! Host-owned authority seam: the only production path to an [`AuthorizedEffect`].
//!
//! An [`EffectIntent`] carries no authority. It becomes an `AuthorizedEffect` only when
//! [`crate::EffectLane::authorize`] (or `authorize_and_execute`) asks an [`EffectAuthority`] and
//! the answer is `Allow` or `AllowRestricted`. The mint is crate-private, so a caller cannot
//! build the value, and the lane, not the caller, supplies the live tool descriptor the
//! authority decides from.
//!
//! [`AuthorityDecision`] mirrors aegis-runtime `DoctrineDecision` one-to-one
//! (`Allow, AllowRestricted, RequireApproval, Deny, Contain`). It is not a new policy language.
//! aien-mcp cannot depend on aegis-runtime, so the shape is repeated here. Consolidating
//! `SafetyEngine`, `ProbePolicyGuard` and `DoctrineDecision` is a separate AIEN milestone.
//!
//! [`EffectClassAuthority`] is the one production implementation. It decides only from the
//! `aien_capability::ToolEffects` bits the catalog already declares (ADR 0005, ADR 0007:
//! irreversible effects are intents only).

use aien_capability::{Digest32, EffectId, JNodeId, ToolDescriptor, ToolEffects, WorldId};

use crate::approval::ApprovalError;
use crate::{EffectIntent, Error};

/// Verdict of an [`EffectAuthority`]. Same five variants as aegis-runtime `DoctrineDecision`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthorityDecision {
    Allow,
    /// Allowed under the listed restrictions (recorded in the policy digest).
    AllowRestricted(Vec<String>),
    /// A human or approval authority must decide. Nothing runs and nothing is consumed until an approval is spent.
    RequireApproval(String),
    Deny(String),
    Contain(String),
}

/// World facts the committer supplies. Identifiers only: they carry no authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectScope {
    pub world_id: WorldId,
    pub winning_jnode: JNodeId,
    pub idempotency_key: EffectId,
}

/// What an authority sees. Built by the lane from the live session, never by the caller.
#[derive(Clone, Debug)]
pub struct AuthorityContext {
    scope: EffectScope,
    descriptor: Option<ToolDescriptor>,
    live_catalog_digest: Digest32,
}

impl AuthorityContext {
    pub(crate) fn new(
        scope: EffectScope,
        descriptor: Option<ToolDescriptor>,
        live_catalog_digest: Digest32,
    ) -> Self {
        Self {
            scope,
            descriptor,
            live_catalog_digest,
        }
    }

    pub fn scope(&self) -> EffectScope {
        self.scope
    }

    /// The descriptor in the live catalog for the intent's tool, or `None` if unknown.
    pub fn descriptor(&self) -> Option<&ToolDescriptor> {
        self.descriptor.as_ref()
    }

    pub fn live_catalog_digest(&self) -> Digest32 {
        self.live_catalog_digest
    }
}

/// Decides whether an intent may become an `AuthorizedEffect`.
pub trait EffectAuthority {
    fn authorize(&self, intent: &EffectIntent, ctx: &AuthorityContext) -> AuthorityDecision;
}

/// Why no `AuthorizedEffect` was produced (or why execution failed afterwards).
#[derive(Debug, PartialEq, Eq)]
pub enum AuthorityOutcome {
    /// Approval is required. Nothing executed. `intent_digest` names the intent to approve.
    /// Issue one with [`crate::ApprovalDesk`] and spend it with [`crate::EffectLane::authorize_approved`].
    Pending {
        intent_digest: Digest32,
        reason: String,
    },
    /// Approval was presented but could not be spent. See [`ApprovalError`].
    Approval(ApprovalError),
    Denied(String),
    Contained(String),
    /// Authorization succeeded but the effect lane failed (stale catalog, provider error, ...).
    Execution(Error),
}

/// Digest that names one intent: tool, arguments and the catalog it was staged against.
pub fn intent_digest(intent: &EffectIntent) -> Digest32 {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(intent.provider.as_str().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(intent.tool_name.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(intent.arguments.to_string().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&intent.capability_digest.0);
    Digest32::of(&bytes)
}

/// AIEN's production authority. Decides from the declared `ToolEffects` of the live descriptor.
///
/// | effects | decision |
/// |---|---|
/// | `EXTERNAL_IRREVERSIBLE` | `Deny` (irreversible effects stay intents, ADR 0005/0007) |
/// | `EXTERNAL_WRITE`, `WORLD_MUTATION`, `SECRET_BEARING`, `SPAWN_PROCESS` | `RequireApproval` |
/// | `PURE`, `READ_FILESYSTEM`, `READ_NETWORK`, `LOCAL_EPHEMERAL` | `Allow` |
/// | unknown tool, unknown effect bits | `Deny` |
#[derive(Clone, Copy, Debug, Default)]
pub struct EffectClassAuthority;

const KNOWN_BITS: u32 = 0xFF;

impl EffectAuthority for EffectClassAuthority {
    fn authorize(&self, intent: &EffectIntent, ctx: &AuthorityContext) -> AuthorityDecision {
        let Some(desc) = ctx.descriptor() else {
            return AuthorityDecision::Deny(format!(
                "tool `{}` is not in the live catalog",
                intent.tool_name
            ));
        };
        let e = desc.effects();
        if e.bits() & !KNOWN_BITS != 0 {
            return AuthorityDecision::Deny("tool declares unknown effect bits".into());
        }
        if e.intersects(ToolEffects::EXTERNAL_IRREVERSIBLE) {
            return AuthorityDecision::Deny(
                "irreversible external effects are intents only".into(),
            );
        }
        if e.intersects(
            ToolEffects::EXTERNAL_WRITE
                | ToolEffects::WORLD_MUTATION
                | ToolEffects::SECRET_BEARING
                | ToolEffects::SPAWN_PROCESS,
        ) {
            return AuthorityDecision::RequireApproval(format!(
                "tool `{}` needs approval (effect bits {:#x})",
                intent.tool_name,
                e.bits()
            ));
        }
        AuthorityDecision::Allow
    }
}

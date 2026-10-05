use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use aien_capability::{
    catalog_digest, speculation_safe, Digest32, EffectId, JNodeId, ProviderId, ToolDescriptor,
    ToolEffects, WorldId,
};
use serde_json::Value;

use crate::approval::{ApprovalError, ApprovalGrant, ApprovalRecord};
use crate::authority::{
    intent_digest, AuthorityContext, AuthorityDecision, AuthorityOutcome, EffectAuthority,
    EffectScope, Exposure,
};
use crate::effect::mint;
use crate::{
    AuthorizedEffect, CallOutcome, CapabilitySnapshot, EffectIntent, EffectReceipt, Error, McpWire,
    ToolResult,
};

const SNAPSHOT_TTL: Duration = Duration::from_secs(60);

/// Live MCP connection. Private to this crate: callers receive snapshots and receipts.
struct McpSession {
    epoch: u64,
    wire: Arc<dyn McpWire>,
    tools: Vec<ToolDescriptor>,
    catalog_digest: Digest32,
}

enum LedgerEntry {
    InFlight,
    Completed(EffectReceipt),
    Uncertain,
}

/// Which effect owns a ledger key: the intent digest (provider, tool, arguments, catalog), the
/// world and the winning J-node. The same four things an approval binds, so a key cannot be
/// replayed as a receipt for a different effect.
#[derive(Clone, Copy, PartialEq, Eq)]
struct EffectIdentity {
    intent: Digest32,
    world_id: WorldId,
    winning_jnode: JNodeId,
}

struct LedgerRow {
    identity: EffectIdentity,
    entry: LedgerEntry,
}

pub(crate) struct Inner {
    sessions: HashMap<String, McpSession>,
    ledger: HashMap<EffectId, LedgerRow>,
    pub(crate) approvals: HashMap<Digest32, ApprovalRecord>,
    pub(crate) next_approval: u64,
}

/// Owns admitted MCP sessions and the idempotency ledger.
#[derive(Clone)]
pub struct McpBroker {
    inner: Arc<Mutex<Inner>>,
}

impl McpBroker {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                sessions: HashMap::new(),
                ledger: HashMap::new(),
                approvals: HashMap::new(),
                next_approval: 0,
            })),
        }
    }

    /// Enroll a provider. Speculative branches cannot do this.
    pub async fn admit(&self, provider: ProviderId, wire: Arc<dyn McpWire>) -> Result<(), Error> {
        let tools = wire.list_tools().await?;
        let session = McpSession {
            epoch: 1,
            catalog_digest: catalog_digest(&tools),
            tools,
            wire,
        };
        let mut inner = self.lock();
        inner
            .sessions
            .insert(provider.as_str().to_string(), session);
        Ok(())
    }

    /// Replace the cached catalog after the provider's schema changes.
    pub async fn replace_catalog(
        &self,
        provider: &ProviderId,
        tools: Vec<ToolDescriptor>,
    ) -> Result<(), Error> {
        let mut inner = self.lock();
        let session = inner
            .sessions
            .get_mut(provider.as_str())
            .ok_or_else(|| Error::NotAdmitted(provider.clone()))?;
        let digest = catalog_digest(&tools);
        if digest != session.catalog_digest {
            session.epoch = session.epoch.saturating_add(1);
            session.catalog_digest = digest;
            session.tools = tools;
        }
        Ok(())
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for McpBroker {
    fn default() -> Self {
        Self::new()
    }
}

/// Lane J-Space is allowed to hold.
#[derive(Clone)]
pub struct SpeculativeLane {
    broker: McpBroker,
}

impl SpeculativeLane {
    pub fn new(broker: McpBroker) -> Self {
        Self { broker }
    }

    /// Refresh an already admitted provider and return the snapshot.
    ///
    /// This does not dial. A provider missing from the session map is
    /// `NotAdmitted`. Opening a transport is enrollment on [`crate::SessionManager`].
    pub async fn discovery_snapshot(
        &self,
        provider: &ProviderId,
    ) -> Result<CapabilitySnapshot, Error> {
        let wire = {
            let inner = self.broker.lock();
            let session = inner
                .sessions
                .get(provider.as_str())
                .ok_or_else(|| Error::NotAdmitted(provider.clone()))?;
            session.wire.clone()
        };
        let tools = wire.list_tools().await?;
        let mut inner = self.broker.lock();
        let session = inner
            .sessions
            .get_mut(provider.as_str())
            .ok_or_else(|| Error::NotAdmitted(provider.clone()))?;
        let digest = catalog_digest(&tools);
        if digest != session.catalog_digest {
            session.epoch = session.epoch.saturating_add(1);
            session.catalog_digest = digest;
        }
        session.tools = tools;
        let generated_at = SystemTime::now();
        Ok(CapabilitySnapshot {
            provider: provider.clone(),
            session_epoch: session.epoch,
            catalog_digest: session.catalog_digest,
            tools: session.tools.clone().into(),
            generated_at,
            expires_at: generated_at + SNAPSHOT_TTL,
        })
    }

    pub async fn invoke_speculative(&self, call: SpeculativeToolCall) -> Result<ToolResult, Error> {
        let wire = {
            let inner = self.broker.lock();
            let session = inner
                .sessions
                .get(call.provider.as_str())
                .ok_or_else(|| Error::NotAdmitted(call.provider.clone()))?;
            let tool = find_tool(session, &call.tool_name).ok_or_else(|| Error::UnknownTool {
                provider: call.provider.clone(),
                tool: call.tool_name.clone(),
            })?;
            if !speculation_safe(tool.effects()) {
                return Err(Error::EffectRequiresCommit);
            }
            if tool
                .effects()
                .intersects(ToolEffects::SPAWN_PROCESS | ToolEffects::LOCAL_EPHEMERAL)
                && !call.sandboxed
            {
                return Err(Error::SandboxRequired);
            }
            session.wire.clone()
        };
        match wire.call_tool(&call.tool_name, &call.arguments).await? {
            CallOutcome::Finished(output) => Ok(ToolResult { output }),
            CallOutcome::Rejected(reason) => Err(Error::Rejected(reason)),
            CallOutcome::Uncertain => Err(Error::ReconciliationRequired),
        }
    }

    pub async fn stage_effect_intent(
        &self,
        provider: &ProviderId,
        tool_name: &str,
        arguments: Value,
    ) -> Result<EffectIntent, Error> {
        let inner = self.broker.lock();
        let session = inner
            .sessions
            .get(provider.as_str())
            .ok_or_else(|| Error::NotAdmitted(provider.clone()))?;
        let tool = find_tool(session, tool_name).ok_or_else(|| Error::UnknownTool {
            provider: provider.clone(),
            tool: tool_name.to_string(),
        })?;
        if speculation_safe(tool.effects()) {
            return Err(Error::SpeculationSafe);
        }
        Ok(EffectIntent {
            provider: provider.clone(),
            tool_name: tool_name.to_string(),
            arguments,
            capability_digest: session.catalog_digest,
        })
    }
}

/// Request J-Space may make on the speculative lane.
pub struct SpeculativeToolCall {
    pub provider: ProviderId,
    pub tool_name: String,
    pub arguments: Value,
    pub sandboxed: bool,
}

/// Lane the Effect Broker holds. `execute_effect` accepts only an authorized effect.
#[derive(Clone)]
pub struct EffectLane {
    broker: McpBroker,
    exposure: Option<Exposure>,
}

impl EffectLane {
    pub fn new(broker: McpBroker) -> Self {
        Self {
            broker,
            exposure: None,
        }
    }

    /// A lane that tells authorities what the model was exposed to. Host/runtime call only:
    /// an intent cannot carry exposure. A lane built with [`Self::new`] has no exposure, which
    /// [`crate::EffectClassAuthority`] treats as `external_untrusted` for effect classes (fail closed).
    pub fn with_exposure(&self, exposure: Exposure) -> Self {
        Self {
            broker: self.broker.clone(),
            exposure: Some(exposure),
        }
    }

    /// Ask `authority` whether `intent` may run, and mint an `AuthorizedEffect` only on
    /// `Allow` or `AllowRestricted`. This is the only production path to an `AuthorizedEffect`.
    ///
    /// The lane, not the caller, reads the live descriptor for the authority. A stale intent
    /// (digest differs from the live catalog) is denied before the authority is asked.
    /// `RequireApproval` returns `Pending` with the intent digest; no approval is consumed here
    /// (see [`Self::authorize_approved`]). `Deny` and `Contain` return errors.
    pub fn authorize(
        &self,
        intent: EffectIntent,
        scope: EffectScope,
        authority: &dyn EffectAuthority,
    ) -> Result<AuthorizedEffect<EffectIntent>, AuthorityOutcome> {
        self.authorize_inner(intent, scope, authority, None)
    }

    /// Like [`Self::authorize`], but a `RequireApproval` verdict is satisfied by spending `grant`.
    ///
    /// The grant must have been issued by an [`crate::ApprovalDesk`] for exactly this intent and
    /// scope, must be unspent, unrevoked and unexpired (`now < expires_at`). A refused grant is not spent
    /// unless it was already spent. `Deny` and `Contain` stay refused whatever the grant says,
    /// and an `Allow` verdict never touches the grant.
    pub fn authorize_approved(
        &self,
        intent: EffectIntent,
        scope: EffectScope,
        authority: &dyn EffectAuthority,
        grant: &ApprovalGrant,
        now: u64,
    ) -> Result<AuthorizedEffect<EffectIntent>, AuthorityOutcome> {
        self.authorize_inner(intent, scope, authority, Some((grant, now)))
    }

    fn authorize_inner(
        &self,
        intent: EffectIntent,
        scope: EffectScope,
        authority: &dyn EffectAuthority,
        approval: Option<(&ApprovalGrant, u64)>,
    ) -> Result<AuthorizedEffect<EffectIntent>, AuthorityOutcome> {
        let (descriptor, live) = {
            let inner = self.broker.lock();
            let session = inner
                .sessions
                .get(intent.provider.as_str())
                .ok_or_else(|| AuthorityOutcome::Denied("provider is not admitted".into()))?;
            (
                find_tool(session, &intent.tool_name).cloned(),
                session.catalog_digest,
            )
        };
        if intent.capability_digest != live {
            return Err(AuthorityOutcome::Denied(
                "intent was staged against a stale catalog".into(),
            ));
        }
        let ctx = AuthorityContext::new(scope, descriptor, live, self.exposure.clone());
        let decision = authority.authorize(&intent, &ctx);
        let policy_digest = {
            let mut bytes = format!("{decision:?}|{}|", intent.tool_name).into_bytes();
            bytes.extend_from_slice(&live.0);
            Digest32::of(&bytes)
        };
        match decision {
            AuthorityDecision::Allow | AuthorityDecision::AllowRestricted(_) => Ok(mint(
                intent,
                scope.world_id,
                scope.winning_jnode,
                policy_digest,
                live,
                scope.idempotency_key,
            )),
            AuthorityDecision::RequireApproval(reason) => {
                let Some((grant, now)) = approval else {
                    return Err(AuthorityOutcome::Pending {
                        intent_digest: intent_digest(&intent),
                        reason,
                    });
                };
                self.spend(grant, &intent, &scope, now)
                    .map_err(AuthorityOutcome::Approval)?;
                Ok(mint(
                    intent,
                    scope.world_id,
                    scope.winning_jnode,
                    policy_digest,
                    live,
                    scope.idempotency_key,
                ))
            }
            AuthorityDecision::Deny(reason) => Err(AuthorityOutcome::Denied(reason)),
            AuthorityDecision::Contain(reason) => Err(AuthorityOutcome::Contained(reason)),
        }
    }

    /// `authorize`, then `execute_effect`. Nothing reaches the provider unless authorized.
    pub async fn authorize_and_execute(
        &self,
        intent: EffectIntent,
        scope: EffectScope,
        authority: &dyn EffectAuthority,
    ) -> Result<EffectReceipt, AuthorityOutcome> {
        let effect = self.authorize(intent, scope, authority)?;
        self.execute_effect(effect)
            .await
            .map_err(AuthorityOutcome::Execution)
    }

    /// `authorize_approved`, then `execute_effect`.
    ///
    /// If `grant` is already spent for this same effect (same intent, key, world, J-node), the
    /// request is a replay: the ledger's existing outcome is returned and nothing is minted.
    /// Any other use of a spent grant is refused with `ApprovalError::Consumed`.
    pub async fn authorize_and_execute_approved(
        &self,
        intent: EffectIntent,
        scope: EffectScope,
        authority: &dyn EffectAuthority,
        grant: &ApprovalGrant,
        now: u64,
    ) -> Result<EffectReceipt, AuthorityOutcome> {
        if let Some(existing) = self.replay_of(grant, &intent, &scope) {
            return existing.map_err(AuthorityOutcome::Execution);
        }
        let effect = self.authorize_approved(intent, scope, authority, grant, now)?;
        self.execute_effect(effect)
            .await
            .map_err(AuthorityOutcome::Execution)
    }

    /// Ledger outcome for a spent grant presented again for the effect it was spent on.
    fn replay_of(
        &self,
        grant: &ApprovalGrant,
        intent: &EffectIntent,
        scope: &EffectScope,
    ) -> Option<Result<EffectReceipt, Error>> {
        let inner = self.broker.lock();
        let record = inner.approvals.get(&grant.id())?;
        if !record.consumed || !record.binds(intent, scope) {
            return None;
        }
        let row = inner.ledger.get(&scope.idempotency_key)?;
        if row.identity != identity(intent, scope.world_id, scope.winning_jnode) {
            return Some(Err(Error::IdempotencyConflict));
        }
        match &row.entry {
            LedgerEntry::Completed(receipt) => Some(Ok(receipt.clone())),
            LedgerEntry::Uncertain => Some(Err(Error::ReconciliationRequired)),
            LedgerEntry::InFlight => Some(Err(Error::EffectInFlight)),
        }
    }

    /// Check and spend `grant` atomically under the broker lock.
    fn spend(
        &self,
        grant: &ApprovalGrant,
        intent: &EffectIntent,
        scope: &EffectScope,
        now: u64,
    ) -> Result<(), ApprovalError> {
        let mut inner = self.broker.lock();
        let record = inner
            .approvals
            .get_mut(&grant.id())
            .ok_or(ApprovalError::Unknown)?;
        if !record.binds(intent, scope) {
            return Err(ApprovalError::Mismatch);
        }
        if record.consumed {
            return Err(ApprovalError::Consumed);
        }
        if record.revoked {
            return Err(ApprovalError::Revoked);
        }
        if now >= record.expires_at {
            return Err(ApprovalError::Expired);
        }
        record.consumed = true;
        Ok(())
    }

    pub async fn execute_effect(
        &self,
        effect: AuthorizedEffect<EffectIntent>,
    ) -> Result<EffectReceipt, Error> {
        let key = effect.idempotency_key();
        let provider = effect.intent().provider.clone();
        let tool_name = effect.intent().tool_name.clone();
        let arguments = effect.intent().arguments.clone();
        let staged_catalog = effect.intent().capability_digest;
        let who = identity(effect.intent(), effect.world_id(), effect.winning_jnode());
        let wire = {
            let mut inner = self.broker.lock();
            if let Some(existing) = inner.ledger.get(&key) {
                // A key names one effect. A request for another effect under the same key did not
                // run and must not be answered with the first effect's receipt.
                if existing.identity != who {
                    return Err(Error::IdempotencyConflict);
                }
                return match &existing.entry {
                    LedgerEntry::Completed(receipt) => Ok(receipt.clone()),
                    LedgerEntry::Uncertain => Err(Error::ReconciliationRequired),
                    LedgerEntry::InFlight => Err(Error::EffectInFlight),
                };
            }
            let wire = {
                let session = inner
                    .sessions
                    .get(provider.as_str())
                    .ok_or_else(|| Error::NotAdmitted(provider.clone()))?;
                if session.catalog_digest != effect.capability_digest()
                    || staged_catalog != effect.capability_digest()
                {
                    return Err(Error::StaleCapability);
                }
                if find_tool(session, &tool_name).is_none() {
                    return Err(Error::UnknownTool {
                        provider: provider.clone(),
                        tool: tool_name.clone(),
                    });
                }
                session.wire.clone()
            };
            inner.ledger.insert(
                key,
                LedgerRow {
                    identity: who,
                    entry: LedgerEntry::InFlight,
                },
            );
            wire
        };

        let outcome = wire.call_tool(&tool_name, &arguments).await;
        let mut inner = self.broker.lock();
        match outcome {
            Ok(CallOutcome::Finished(output)) => {
                let receipt = EffectReceipt {
                    effect_id: key,
                    world_id: effect.world_id(),
                    winning_jnode: effect.winning_jnode(),
                    policy_digest: effect.policy_digest(),
                    provider,
                    tool_name,
                    capability_digest: effect.capability_digest(),
                    output,
                };
                inner.ledger.insert(
                    key,
                    LedgerRow {
                        identity: who,
                        entry: LedgerEntry::Completed(receipt.clone()),
                    },
                );
                Ok(receipt)
            }
            Ok(CallOutcome::Uncertain) | Err(_) => {
                inner.ledger.insert(
                    key,
                    LedgerRow {
                        identity: who,
                        entry: LedgerEntry::Uncertain,
                    },
                );
                Err(Error::ReconciliationRequired)
            }
            Ok(CallOutcome::Rejected(reason)) => {
                inner.ledger.remove(&key);
                Err(Error::Rejected(reason))
            }
        }
    }
}

fn identity(intent: &EffectIntent, world_id: WorldId, winning_jnode: JNodeId) -> EffectIdentity {
    EffectIdentity {
        intent: intent_digest(intent),
        world_id,
        winning_jnode,
    }
}

fn find_tool<'a>(session: &'a McpSession, name: &str) -> Option<&'a ToolDescriptor> {
    session.tools.iter().find(|tool| tool.name() == name)
}

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use aien_capability::{
    catalog_digest, speculation_safe, Digest32, EffectId, ProviderId, ToolDescriptor, ToolEffects,
};
use serde_json::Value;

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

struct Inner {
    sessions: HashMap<String, McpSession>,
    ledger: HashMap<EffectId, LedgerEntry>,
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

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
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
}

impl EffectLane {
    pub fn new(broker: McpBroker) -> Self {
        Self { broker }
    }

    pub async fn execute_effect(
        &self,
        effect: AuthorizedEffect<EffectIntent>,
    ) -> Result<EffectReceipt, Error> {
        let key = effect.idempotency_key();
        let provider = effect.intent().provider.clone();
        let tool_name = effect.intent().tool_name.clone();
        let arguments = effect.intent().arguments.clone();
        let intent_digest = effect.intent().capability_digest;
        let wire = {
            let mut inner = self.broker.lock();
            if let Some(existing) = inner.ledger.get(&key) {
                return match existing {
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
                    || intent_digest != effect.capability_digest()
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
            inner.ledger.insert(key, LedgerEntry::InFlight);
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
                inner
                    .ledger
                    .insert(key, LedgerEntry::Completed(receipt.clone()));
                Ok(receipt)
            }
            Ok(CallOutcome::Uncertain) | Err(_) => {
                inner.ledger.insert(key, LedgerEntry::Uncertain);
                Err(Error::ReconciliationRequired)
            }
            Ok(CallOutcome::Rejected(reason)) => {
                inner.ledger.remove(&key);
                Err(Error::Rejected(reason))
            }
        }
    }
}

fn find_tool<'a>(session: &'a McpSession, name: &str) -> Option<&'a ToolDescriptor> {
    session.tools.iter().find(|tool| tool.name() == name)
}

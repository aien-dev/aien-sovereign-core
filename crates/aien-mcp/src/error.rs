use aien_capability::ProviderId;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("provider `{0}` is not admitted to the capability graph")]
    NotAdmitted(ProviderId),
    #[error("provider `{provider}` has no tool `{tool}`")]
    UnknownTool { provider: ProviderId, tool: String },
    #[error("tool effect requires a committed winning world")]
    EffectRequiresCommit,
    #[error("local ephemeral tool requires a sandbox")]
    SandboxRequired,
    #[error("tool is speculation-safe and is not an external effect")]
    SpeculationSafe,
    #[error("authorized capability digest does not match the live catalog")]
    StaleCapability,
    #[error("effect outcome is uncertain and must be reconciled, not retried")]
    ReconciliationRequired,
    #[error("effect is already in flight")]
    EffectInFlight,
    #[error("tool call rejected: {0}")]
    Rejected(String),
}

//! Enrollment owner. J-Space receives a [`crate::SpeculativeLane`], not this type.

use std::collections::HashMap;
use std::sync::Arc;

use aien_capability::{ProviderId, ToolEffects};
use rmcp::service::RoleClient;
use rmcp::transport::IntoTransport;

use crate::transport::RmcpClientWire;
use crate::{EffectLane, Error, McpBroker, McpWire, SpeculativeLane};

/// Admits MCP providers and hands out the two lanes.
///
/// `enroll` and `enroll_transport` are the only ways to attach a server.
/// [`SpeculativeLane`] can refresh a provider that is already here. It cannot dial.
#[derive(Clone)]
pub struct SessionManager {
    broker: McpBroker,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            broker: McpBroker::new(),
        }
    }

    pub fn speculative_lane(&self) -> SpeculativeLane {
        SpeculativeLane::new(self.broker.clone())
    }

    pub fn effect_lane(&self) -> EffectLane {
        EffectLane::new(self.broker.clone())
    }

    /// Enroll a provider that is already on an [`McpWire`].
    pub async fn enroll(&self, provider: ProviderId, wire: Arc<dyn McpWire>) -> Result<(), Error> {
        self.broker.admit(provider, wire).await
    }

    /// Dial one `rmcp` transport and enroll it.
    ///
    /// `effects` is the capability-graph classification for tool names this
    /// provider is allowed to advertise. A name missing from the map is not
    /// given a class from the server.
    pub async fn enroll_transport<T, E, A>(
        &self,
        provider: ProviderId,
        transport: T,
        effects: HashMap<String, ToolEffects>,
    ) -> Result<(), Error>
    where
        T: IntoTransport<RoleClient, E, A>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let wire = RmcpClientWire::connect(transport, effects).await?;
        self.enroll(provider, Arc::new(wire)).await
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

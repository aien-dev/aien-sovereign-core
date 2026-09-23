use aien_capability::ToolDescriptor;
use serde_json::Value;

use crate::Error;

/// What the session observed from one `tools/call`.
///
/// `Uncertain` means the request may have reached the server. The broker must
/// not send it again.
#[derive(Clone, Debug, PartialEq)]
pub enum CallOutcome {
    Finished(Value),
    Rejected(String),
    Uncertain,
}

/// Provider face used by an admitted session.
///
/// The methods are async so an `rmcp` peer can sit behind the same seam as an
/// in-process wire. The trait stays object-safe: the session stores
/// `Arc<dyn McpWire>` and never hands that handle to J-Space.
#[async_trait::async_trait]
pub trait McpWire: Send + Sync {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, Error>;
    async fn call_tool(&self, name: &str, arguments: &Value) -> Result<CallOutcome, Error>;
}

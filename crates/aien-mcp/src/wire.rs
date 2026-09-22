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

pub trait McpWire: Send + Sync {
    fn list_tools(&self) -> Result<Vec<ToolDescriptor>, Error>;
    fn call_tool(&self, name: &str, arguments: &Value) -> Result<CallOutcome, Error>;
}

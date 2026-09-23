use std::sync::Arc;

use aien_capability::ToolDescriptor;
use serde_json::Value;

use crate::{CallOutcome, Error, McpWire};

type ToolHandler = dyn Fn(&str, &Value) -> CallOutcome + Send + Sync;

/// In-process provider used by tests and by local dry runs of the lanes.
pub struct MemoryWire {
    tools: Vec<ToolDescriptor>,
    call: Arc<ToolHandler>,
}

impl MemoryWire {
    pub fn new<F>(tools: Vec<ToolDescriptor>, call: F) -> Self
    where
        F: Fn(&str, &Value) -> CallOutcome + Send + Sync + 'static,
    {
        Self {
            tools,
            call: Arc::new(call),
        }
    }
}

#[async_trait::async_trait]
impl McpWire for MemoryWire {
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, Error> {
        Ok(self.tools.clone())
    }

    async fn call_tool(&self, name: &str, arguments: &Value) -> Result<CallOutcome, Error> {
        Ok((self.call)(name, arguments))
    }
}

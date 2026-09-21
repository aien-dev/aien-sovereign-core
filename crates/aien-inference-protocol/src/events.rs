use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum InferenceEvent {
    Token(String),
    Reasoning(String),
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    Completed {
        total_tokens: usize,
        prompt_tokens: usize,
        completion_tokens: usize,
    },
    Error(String),
}

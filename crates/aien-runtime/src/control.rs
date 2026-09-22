//! Typed Operator Control Protocol and RuntimeController
//! Connects external operator CLI tools to the in-process runtime over local domain sockets.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchSwarmReq {
    pub model_handle: u64,
    pub branch_count: usize,
    pub max_active_sequences: usize,
    pub max_tokens_per_branch: usize,
    pub root_world_id: u64,
    pub priority: u8,
    pub prompt_tokens: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

/// TinyLlama chat template. Generation always continues from an assistant header.
pub fn format_tinyllama_chat(messages: &[ChatTurn]) -> String {
    let mut out = String::new();
    for message in messages {
        let body = message.content.trim();
        if body.is_empty() {
            continue;
        }
        let tag = match message.role.trim().to_ascii_lowercase().as_str() {
            "system" => "system",
            "assistant" => "assistant",
            _ => "user",
        };
        out.push_str(&format!("<|{tag}|>\n{body}</s>\n"));
    }
    let ends_in_assistant = messages.last().is_some_and(|message| {
        message.role.trim().eq_ignore_ascii_case("assistant") && !message.content.trim().is_empty()
    });
    if !ends_in_assistant {
        out.push_str("<|assistant|>\n");
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlCommand {
    LaunchSwarm(LaunchSwarmReq),
    InspectSwarm(u64),
    CancelSwarm(u64),
    GetRuntimeStatus,
    Shutdown,
    /// One operator turn on the in-process PR #68 spine. The server streams
    /// `TurnDelta` lines and finishes with `TurnFinished`.
    StreamTurn {
        messages: Vec<ChatTurn>,
        max_tokens: usize,
        temperature: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlEnvelope {
    pub protocol_version: u16,
    pub request_id: u64,
    pub operation_id: u128,
    pub operator_session: u64,
    pub command: ControlCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatusReport {
    pub active_sequences: usize,
    pub active_swarms: usize,
    pub active_worlds: usize,
    pub free_kv_blocks: usize,
    pub total_kv_blocks: usize,
    pub shared_kv_pages: usize,
    pub cow_faults: usize,
    pub gpu_utilization_pct: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlResponse {
    SwarmAccepted { swarm_id: u64, operation_id: u128 },
    SwarmCancelled { swarm_id: u64 },
    Status(RuntimeStatusReport),
    ShutdownAck,
    TurnDelta { text: String },
    TurnFinished { text: String, total_tokens: usize },
    Error(String),
}

/// Actor managing operator sessions and enforcing idempotent command execution.
/// Processed operation IDs persist to disk so idempotency survives restarts.
pub struct RuntimeController {
    processed_operations: HashSet<u128>,
}

fn operations_state_path() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("AIEN_RUNTIME_STATE_DIR") {
        if !dir.trim().is_empty() {
            return std::path::PathBuf::from(dir.trim()).join("processed_operations.json");
        }
    }
    std::path::PathBuf::from("/tmp/aien-runtime-processed-ops.json")
}

impl Default for RuntimeController {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeController {
    pub fn new() -> Self {
        let mut processed_operations = HashSet::new();
        if let Ok(bytes) = std::fs::read(operations_state_path()) {
            if let Ok(ids) = serde_json::from_slice::<Vec<u128>>(&bytes) {
                processed_operations.extend(ids);
            }
        }
        Self {
            processed_operations,
        }
    }

    pub fn is_operation_processed(&self, op_id: u128) -> bool {
        self.processed_operations.contains(&op_id)
    }

    pub fn mark_operation_processed(&mut self, op_id: u128) {
        self.processed_operations.insert(op_id);
        let ids: Vec<u128> = self.processed_operations.iter().copied().collect();
        if let Ok(bytes) = serde_json::to_vec(&ids) {
            let _ = std::fs::write(operations_state_path(), bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idempotency_survives_restart() {
        let dir = std::env::temp_dir().join(format!("aien-ops-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("AIEN_RUNTIME_STATE_DIR", &dir);
        let op_id = 0xC0FFEEu128;

        {
            let mut first = RuntimeController::new();
            assert!(!first.is_operation_processed(op_id));
            first.mark_operation_processed(op_id);
            assert!(first.is_operation_processed(op_id));
        }

        // Simulate a process restart: a fresh controller reloads from disk.
        {
            let second = RuntimeController::new();
            assert!(
                second.is_operation_processed(op_id),
                "replayed operation ID must be rejected after restart"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn chat_template_opens_an_assistant_turn() {
        let prompt = format_tinyllama_chat(&[
            ChatTurn {
                role: "system".into(),
                content: "You are AIEN.".into(),
            },
            ChatTurn {
                role: "user".into(),
                content: "Status?".into(),
            },
        ]);
        assert!(prompt.starts_with("<|system|>\nYou are AIEN.</s>\n"));
        assert!(prompt.contains("<|user|>\nStatus?</s>\n"));
        assert!(prompt.ends_with("<|assistant|>\n"));
    }
}

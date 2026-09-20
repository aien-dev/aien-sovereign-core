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
pub enum ControlCommand {
    LaunchSwarm(LaunchSwarmReq),
    InspectSwarm(u64),
    CancelSwarm(u64),
    GetRuntimeStatus,
    Shutdown,
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
    Error(String),
}

/// Actor managing operator sessions and enforcing idempotent command execution.
pub struct RuntimeController {
    processed_operations: HashSet<u128>,
}

impl Default for RuntimeController {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeController {
    pub fn new() -> Self {
        Self {
            processed_operations: HashSet::new(),
        }
    }

    pub fn is_operation_processed(&self, op_id: u128) -> bool {
        self.processed_operations.contains(&op_id)
    }

    pub fn mark_operation_processed(&mut self, op_id: u128) {
        self.processed_operations.insert(op_id);
    }
}

//! SwarmManager and Branch-Native Swarm Coordination
//! Manages first-class SwarmRecord entities and atomic branch operations.

use crate::sequence::{SequenceArena, SequenceId, SequenceState};
use crate::world::WorldStore;
use aien_kv_cache::AienKvManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwarmState {
    Created,
    Running,
    Paused,
    Cancelling,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmConfig {
    pub model_handle: u64,
    pub branch_count: usize,
    pub max_active_sequences: usize,
    pub max_tokens_per_branch: usize,
    pub root_world_id: u64,
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmRecord {
    pub id: u64,
    pub config: SwarmConfig,
    pub root_sequence_id: SequenceId,
    pub branch_sequences: Vec<SequenceId>,
    pub state: SwarmState,
    pub created_at: u64,
}

pub struct SwarmManager {
    swarms: HashMap<u64, SwarmRecord>,
    next_swarm_id: u64,
}

impl Default for SwarmManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SwarmManager {
    pub fn new() -> Self {
        Self {
            swarms: HashMap::new(),
            next_swarm_id: 1,
        }
    }

    /// Launches a swarm: creates root sequence, preallocates root KV blocks, and forks N branches.
    pub fn launch_swarm(
        &mut self,
        config: SwarmConfig,
        arena: &mut SequenceArena,
        kv: &mut AienKvManager,
        world_store: &mut WorldStore,
        prompt_tokens: &[u32],
        timestamp: u64,
    ) -> Result<u64, String> {
        let swarm_id = self.next_swarm_id;
        self.next_swarm_id += 1;

        // 1. Allocate root sequence in arena
        let root_seq = arena.allocate(
            config.root_world_id,
            1, // Initial root context revision
            config.priority,
            timestamp,
        )?;

        // 2. Allocate root prompt tokens in physical KV manager
        let root_u64 = root_seq.as_u64();
        kv.allocate_sequence(root_u64, prompt_tokens)?;

        if let Some(record) = arena.get_mut(root_seq) {
            record.prefill_cursor = prompt_tokens.len();
            record.state = SequenceState::Prefill;
        }

        // 3. Atomically fork N branches sharing the root World and root KV blocks
        let mut branch_sequences = Vec::with_capacity(config.branch_count);
        for _ in 0..config.branch_count {
            let child_world = world_store.fork_world(config.root_world_id, timestamp)?;
            let child_seq = arena.fork(root_seq, child_world, timestamp)?;
            let child_u64 = child_seq.as_u64();

            // Zero-copy KV fork: increments refcount on parent physical blocks
            kv.fork_sequence(root_u64, child_u64)?;
            branch_sequences.push(child_seq);
        }

        let record = SwarmRecord {
            id: swarm_id,
            config,
            root_sequence_id: root_seq,
            branch_sequences,
            state: SwarmState::Running,
            created_at: timestamp,
        };

        self.swarms.insert(swarm_id, record);
        Ok(swarm_id)
    }

    pub fn get_swarm(&self, swarm_id: u64) -> Option<&SwarmRecord> {
        self.swarms.get(&swarm_id)
    }

    pub fn cancel_swarm(&mut self, swarm_id: u64, arena: &mut SequenceArena) -> Result<(), String> {
        let swarm = self
            .swarms
            .get_mut(&swarm_id)
            .ok_or_else(|| format!("Swarm {} not found", swarm_id))?;

        swarm.state = SwarmState::Cancelling;

        if let Some(root_rec) = arena.get_mut(swarm.root_sequence_id) {
            root_rec.state = SequenceState::Cancelled;
        }

        for &child_id in &swarm.branch_sequences {
            if let Some(child_rec) = arena.get_mut(child_id) {
                child_rec.state = SequenceState::Cancelled;
            }
        }

        swarm.state = SwarmState::Completed;
        Ok(())
    }

    pub fn active_swarm_count(&self) -> usize {
        self.swarms
            .values()
            .filter(|s| s.state == SwarmState::Running)
            .count()
    }
}

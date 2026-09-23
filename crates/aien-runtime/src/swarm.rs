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
    /// World IDs recorded at fork time. Cancel must drop these even when the
    /// arena records were already freed by natural completion.
    pub branch_worlds: Vec<u64>,
    /// Branches that finished naturally. Completion of all branches triggers
    /// swarm-wide reclamation: root arena slot, root KV, and branch worlds.
    pub finished_branches: Vec<SequenceId>,
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

        // Ensure a root world exists
        let root_world_id = if world_store.get_world(config.root_world_id).is_some() {
            config.root_world_id
        } else {
            world_store.create_root_world(1, 1, 1, timestamp)
        };

        // 1. Allocate root sequence in arena
        let root_seq = arena.allocate(
            root_world_id,
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
        let mut branch_worlds = Vec::with_capacity(config.branch_count);
        for _ in 0..config.branch_count {
            let child_world = world_store.fork_world(root_world_id, timestamp)?;
            let child_seq = arena.fork(root_seq, child_world, timestamp)?;
            let child_u64 = child_seq.as_u64();

            // Zero-copy KV fork: increments refcount on parent physical blocks
            kv.fork_sequence(root_u64, child_u64)?;
            branch_sequences.push(child_seq);
            branch_worlds.push(child_world);
        }

        let record = SwarmRecord {
            id: swarm_id,
            config,
            root_sequence_id: root_seq,
            branch_sequences,
            branch_worlds,
            finished_branches: Vec::new(),
            state: SwarmState::Running,
            created_at: timestamp,
        };

        self.swarms.insert(swarm_id, record);
        Ok(swarm_id)
    }

    pub fn get_swarm(&self, swarm_id: u64) -> Option<&SwarmRecord> {
        self.swarms.get(&swarm_id)
    }

    /// Cancels a swarm and reclaims its resources: KV sequences, arena slots,
    /// and branch worlds. The root world is retained; branch worlds are dropped.
    pub fn cancel_swarm(
        &mut self,
        swarm_id: u64,
        arena: &mut SequenceArena,
        kv: &mut AienKvManager,
        worlds: &mut WorldStore,
    ) -> Result<(), String> {
        let swarm = self
            .swarms
            .get_mut(&swarm_id)
            .ok_or_else(|| format!("Swarm {} not found", swarm_id))?;

        swarm.state = SwarmState::Cancelling;

        if let Some(root_rec) = arena.get_mut(swarm.root_sequence_id) {
            root_rec.state = SequenceState::Cancelled;
        }
        let _ = kv.free_sequence(swarm.root_sequence_id.as_u64());

        for &child_id in &swarm.branch_sequences {
            if let Some(child_rec) = arena.get_mut(child_id) {
                child_rec.state = SequenceState::Cancelled;
            }
            let _ = kv.free_sequence(child_id.as_u64());
        }

        for &child_id in &swarm.branch_sequences {
            arena.free(child_id);
        }
        arena.free(swarm.root_sequence_id);

        // Drop from the launch-time ledger, not from arena lookups: branch
        // sequences may have completed naturally and already left the arena.
        let branch_worlds = swarm.branch_worlds.clone();
        for world_id in branch_worlds {
            worlds.drop_world(world_id);
        }
        worlds.collect_garbage();

        swarm.state = SwarmState::Completed;
        Ok(())
    }

    /// Records a branch that finished naturally. Once every branch of a
    /// running swarm has finished, reclaims the whole swarm: root arena slot,
    /// root KV blocks, and all branch worlds. Without this, the root sequence
    /// pins the arena forever and finished branch worlds leak.
    pub fn note_sequence_finished(
        &mut self,
        seq_id: SequenceId,
        arena: &mut SequenceArena,
        kv: &mut AienKvManager,
        worlds: &mut WorldStore,
    ) {
        let swarm_id = self.swarms.iter().find_map(|(id, swarm)| {
            (swarm.state == SwarmState::Running
                && swarm.branch_sequences.contains(&seq_id)
                && !swarm.finished_branches.contains(&seq_id))
            .then_some(*id)
        });
        let Some(swarm_id) = swarm_id else {
            return;
        };

        let swarm = self
            .swarms
            .get_mut(&swarm_id)
            .expect("swarm id from live iterator must exist");
        swarm.finished_branches.push(seq_id);
        if swarm.finished_branches.len() < swarm.branch_sequences.len() {
            return;
        }

        if let Some(root_rec) = arena.get_mut(swarm.root_sequence_id) {
            root_rec.state = SequenceState::Completed;
        }
        let _ = kv.free_sequence(swarm.root_sequence_id.as_u64());
        arena.free(swarm.root_sequence_id);

        for world_id in swarm.branch_worlds.clone() {
            worlds.drop_world(world_id);
        }
        worlds.collect_garbage();
        swarm.state = SwarmState::Completed;
    }

    pub fn active_swarm_count(&self) -> usize {
        self.swarms
            .values()
            .filter(|s| s.state == SwarmState::Running)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::WorldStore;

    fn test_config(branch_count: usize) -> SwarmConfig {
        SwarmConfig {
            model_handle: 1,
            branch_count,
            max_active_sequences: branch_count,
            max_tokens_per_branch: 8,
            root_world_id: 0,
            priority: 1,
        }
    }

    #[test]
    fn cancel_drops_branch_worlds_after_natural_completion() {
        let mut arena = SequenceArena::new(256);
        let kv_shared = aien_kv_cache::create_shared_kv_manager(256, 16);
        let mut kv = kv_shared.write();
        let mut worlds = WorldStore::new();
        let mut manager = SwarmManager::new();

        let prompt = vec![1u32, 2, 3];
        let swarm_id = manager
            .launch_swarm(test_config(4), &mut arena, &mut kv, &mut worlds, &prompt, 1)
            .expect("swarm launch must succeed");
        assert_eq!(worlds.active_world_count(), 5, "root plus 4 branch worlds");

        // Branches finish naturally: arena records leave the arena, worlds stay.
        let record = manager.get_swarm(swarm_id).expect("record").clone();
        for &child in &record.branch_sequences {
            arena.free(child);
        }
        arena.free(record.root_sequence_id);

        manager
            .cancel_swarm(swarm_id, &mut arena, &mut kv, &mut worlds)
            .expect("cancel must succeed");

        assert_eq!(
            worlds.active_world_count(),
            1,
            "branch worlds must drop even after arena records were freed"
        );
    }

    #[test]
    fn natural_completion_reclaims_root_and_branch_worlds() {
        let mut arena = SequenceArena::new(256);
        let kv_shared = aien_kv_cache::create_shared_kv_manager(256, 16);
        let mut kv = kv_shared.write();
        let mut worlds = WorldStore::new();
        let mut manager = SwarmManager::new();

        let prompt = vec![7u32, 8, 9];
        let swarm_id = manager
            .launch_swarm(test_config(3), &mut arena, &mut kv, &mut worlds, &prompt, 1)
            .expect("swarm launch must succeed");
        assert_eq!(worlds.active_world_count(), 4, "root plus 3 branch worlds");

        // Every branch finishes through the scheduler; the spine reports each
        // finish to the swarm manager.
        let record = manager.get_swarm(swarm_id).expect("record").clone();
        for &child in &record.branch_sequences {
            manager.note_sequence_finished(child, &mut arena, &mut kv, &mut worlds);
        }

        assert_eq!(manager.active_swarm_count(), 0, "swarm must complete");
        assert_eq!(
            worlds.active_world_count(),
            1,
            "only the root world remains"
        );
        assert!(
            arena.get(record.root_sequence_id).is_none(),
            "root arena slot must be reclaimed on natural completion"
        );
    }
}

//! Immutable World Manifests and Structural State Storage
//! Implements path-copying persistent world branching, staged effect journals, and reachability tracking.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EffectIntent {
    WriteFile {
        path: String,
        content_hash: [u8; 32],
    },
    ExecuteCommand {
        command: String,
        args: Vec<String>,
    },
    SendMessage {
        recipient: String,
        message_hash: [u8; 32],
    },
    PublishGit {
        remote: String,
        branch: String,
        commit_sha: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldManifest {
    pub id: u64,
    pub parent: Option<u64>,
    pub object_root: [u8; 32],
    pub filesystem_root: [u8; 32],
    pub token_root: u64,
    pub memory_root: u64,
    pub capability_set: u64,
    pub effect_log: Vec<EffectIntent>,
    pub created_at: u64,
}

pub struct WorldDraft {
    pub base_world_id: u64,
    pub staged_objects: HashMap<String, Vec<u8>>,
    pub staged_effects: Vec<EffectIntent>,
}

impl WorldDraft {
    pub fn new(base_world_id: u64) -> Self {
        Self {
            base_world_id,
            staged_objects: HashMap::new(),
            staged_effects: Vec::new(),
        }
    }

    pub fn stage_object(&mut self, key: String, data: Vec<u8>) {
        self.staged_objects.insert(key, data);
    }

    pub fn record_effect(&mut self, effect: EffectIntent) {
        self.staged_effects.push(effect);
    }
}

/// Content-addressed in-memory persistent object store and world hierarchy.
pub struct WorldStore {
    worlds: HashMap<u64, WorldManifest>,
    objects: HashMap<[u8; 32], Vec<u8>>,
    retained_roots: HashSet<u64>,
    next_world_id: u64,
}

impl Default for WorldStore {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldStore {
    pub fn new() -> Self {
        Self {
            worlds: HashMap::new(),
            objects: HashMap::new(),
            retained_roots: HashSet::new(),
            next_world_id: 1,
        }
    }

    pub fn create_root_world(
        &mut self,
        token_root: u64,
        memory_root: u64,
        capability_set: u64,
        timestamp: u64,
    ) -> u64 {
        let world_id = self.next_world_id;
        self.next_world_id += 1;

        let manifest = WorldManifest {
            id: world_id,
            parent: None,
            object_root: [0u8; 32],
            filesystem_root: [0u8; 32],
            token_root,
            memory_root,
            capability_set,
            effect_log: Vec::new(),
            created_at: timestamp,
        };

        self.worlds.insert(world_id, manifest);
        self.retained_roots.insert(world_id);
        world_id
    }

    /// Fast constant-time fork: duplicates root pointers into a new WorldManifest.
    pub fn fork_world(&mut self, parent_id: u64, timestamp: u64) -> Result<u64, String> {
        let parent = self
            .worlds
            .get(&parent_id)
            .ok_or_else(|| format!("Parent world {} not found", parent_id))?
            .clone();

        let child_id = self.next_world_id;
        self.next_world_id += 1;

        let child = WorldManifest {
            id: child_id,
            parent: Some(parent_id),
            object_root: parent.object_root,
            filesystem_root: parent.filesystem_root,
            token_root: parent.token_root,
            memory_root: parent.memory_root,
            capability_set: parent.capability_set,
            effect_log: Vec::new(), // Staged effects begin empty for child
            created_at: timestamp,
        };

        self.worlds.insert(child_id, child);
        self.retained_roots.insert(child_id);
        Ok(child_id)
    }

    pub fn commit_draft(&mut self, draft: WorldDraft, timestamp: u64) -> Result<u64, String> {
        let parent = self
            .worlds
            .get(&draft.base_world_id)
            .ok_or_else(|| format!("Base world {} not found", draft.base_world_id))?
            .clone();

        // Compute simulated content hash for staged objects
        let mut object_hash = parent.object_root;
        for (key, data) in draft.staged_objects {
            let mut hasher_val = 0u64;
            for b in key.as_bytes().iter().chain(data.iter()) {
                hasher_val = hasher_val.wrapping_mul(31).wrapping_add(*b as u64);
            }
            let key_bytes = hasher_val.to_le_bytes();
            for i in 0..8 {
                object_hash[i] ^= key_bytes[i];
            }
            // Store content-addressed blob
            let mut blob_hash = [0u8; 32];
            blob_hash[0..8].copy_from_slice(&key_bytes);
            self.objects.insert(blob_hash, data);
        }

        let new_world_id = self.next_world_id;
        self.next_world_id += 1;

        let manifest = WorldManifest {
            id: new_world_id,
            parent: Some(draft.base_world_id),
            object_root: object_hash,
            filesystem_root: parent.filesystem_root,
            token_root: parent.token_root,
            memory_root: parent.memory_root,
            capability_set: parent.capability_set,
            effect_log: draft.staged_effects,
            created_at: timestamp,
        };

        self.worlds.insert(new_world_id, manifest);
        self.retained_roots.insert(new_world_id);
        Ok(new_world_id)
    }

    pub fn get_world(&self, world_id: u64) -> Option<&WorldManifest> {
        self.worlds.get(&world_id)
    }

    pub fn drop_world(&mut self, world_id: u64) -> bool {
        self.retained_roots.remove(&world_id);
        self.worlds.remove(&world_id).is_some()
    }

    pub fn active_world_count(&self) -> usize {
        self.worlds.len()
    }
}

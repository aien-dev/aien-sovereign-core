//! SequenceArena and Generational Sequence Allocation
//! Enforces single-mutable-owner discipline for the scheduler thread.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SequenceId {
    pub slot: u32,
    pub generation: u32,
}

impl SequenceId {
    pub fn new(slot: u32, generation: u32) -> Self {
        Self { slot, generation }
    }

    #[inline]
    pub fn as_u64(&self) -> u64 {
        ((self.generation as u64) << 32) | (self.slot as u64)
    }

    #[inline]
    pub fn from_u64(val: u64) -> Self {
        Self {
            slot: (val & 0xFFFF_FFFF) as u32,
            generation: (val >> 32) as u32,
        }
    }
}

impl std::fmt::Display for SequenceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "seq_{}:{}", self.slot, self.generation)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SequenceState {
    Ready,
    Prefill,
    Decode,
    BlockedOnTool,
    Cancelled,
    Completed,
}

#[repr(C, align(64))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceRecord {
    pub id: SequenceId,
    pub parent_id: Option<SequenceId>,
    pub world_id: u64,
    pub priority: u8,
    pub state: SequenceState,
    pub context_id: u64,
    pub block_table_id: u32,
    pub prefill_cursor: usize,
    pub generated_tokens: usize,
    pub arrival_ticks: u64,
    pub deadline_ticks: Option<u64>,
}

pub struct SequenceSlot {
    pub generation: u32,
    pub record: Option<SequenceRecord>,
}

/// Contiguous SequenceArena owned exclusively by the Scheduler thread.
pub struct SequenceArena {
    slots: Vec<SequenceSlot>,
    free_slots: Vec<u32>,
    active_count: usize,
    capacity: usize,
}

impl SequenceArena {
    pub fn new(capacity: usize) -> Self {
        let mut slots = Vec::with_capacity(capacity);
        let mut free_slots = Vec::with_capacity(capacity);

        for i in 0..capacity {
            slots.push(SequenceSlot {
                generation: 1,
                record: None,
            });
            free_slots.push(i as u32);
        }

        Self {
            slots,
            free_slots,
            active_count: 0,
            capacity,
        }
    }

    pub fn allocate(
        &mut self,
        world_id: u64,
        context_id: u64,
        priority: u8,
        arrival_ticks: u64,
    ) -> Result<SequenceId, String> {
        let slot_idx = self
            .free_slots
            .pop()
            .ok_or_else(|| format!("SequenceArena capacity exhausted ({})", self.capacity))?;

        let slot = &mut self.slots[slot_idx as usize];
        let id = SequenceId::new(slot_idx, slot.generation);

        slot.record = Some(SequenceRecord {
            id,
            parent_id: None,
            world_id,
            priority,
            state: SequenceState::Ready,
            context_id,
            block_table_id: slot_idx,
            prefill_cursor: 0,
            generated_tokens: 0,
            arrival_ticks,
            deadline_ticks: None,
        });

        self.active_count += 1;
        Ok(id)
    }

    pub fn fork(
        &mut self,
        parent_id: SequenceId,
        child_world_id: u64,
        arrival_ticks: u64,
    ) -> Result<SequenceId, String> {
        let (parent_context_id, parent_priority, parent_prefill, parent_generated) = {
            let parent = self
                .get(parent_id)
                .ok_or_else(|| format!("Parent sequence {} not found or stale", parent_id))?;
            (
                parent.context_id,
                parent.priority,
                parent.prefill_cursor,
                parent.generated_tokens,
            )
        };

        let child_slot = self.free_slots.pop().ok_or_else(|| {
            format!(
                "SequenceArena capacity exhausted on fork ({})",
                self.capacity
            )
        })?;

        let slot = &mut self.slots[child_slot as usize];
        let child_id = SequenceId::new(child_slot, slot.generation);

        slot.record = Some(SequenceRecord {
            id: child_id,
            parent_id: Some(parent_id),
            world_id: child_world_id,
            priority: parent_priority,
            state: SequenceState::Decode,
            context_id: parent_context_id,
            block_table_id: child_slot,
            prefill_cursor: parent_prefill,
            generated_tokens: parent_generated,
            arrival_ticks,
            deadline_ticks: None,
        });

        self.active_count += 1;
        Ok(child_id)
    }

    #[inline]
    pub fn get(&self, id: SequenceId) -> Option<&SequenceRecord> {
        let slot = self.slots.get(id.slot as usize)?;
        if slot.generation == id.generation {
            slot.record.as_ref()
        } else {
            None
        }
    }

    #[inline]
    pub fn get_mut(&mut self, id: SequenceId) -> Option<&mut SequenceRecord> {
        let slot = self.slots.get_mut(id.slot as usize)?;
        if slot.generation == id.generation {
            slot.record.as_mut()
        } else {
            None
        }
    }

    pub fn free(&mut self, id: SequenceId) -> bool {
        if let Some(slot) = self.slots.get_mut(id.slot as usize) {
            if slot.generation == id.generation && slot.record.is_some() {
                slot.record = None;
                // Increment generation counter to invalidate any in-flight references (ABA protection)
                slot.generation = slot.generation.wrapping_add(1).max(1);
                self.free_slots.push(id.slot);
                self.active_count = self.active_count.saturating_sub(1);
                return true;
            }
        }
        false
    }

    pub fn validate(&self, id: SequenceId) -> bool {
        self.get(id).is_some()
    }

    pub fn active_count(&self) -> usize {
        self.active_count
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn iter_active(&self) -> impl Iterator<Item = &SequenceRecord> {
        self.slots.iter().filter_map(|s| s.record.as_ref())
    }

    pub fn iter_active_mut(&mut self) -> impl Iterator<Item = &mut SequenceRecord> {
        self.slots.iter_mut().filter_map(|s| s.record.as_mut())
    }
}

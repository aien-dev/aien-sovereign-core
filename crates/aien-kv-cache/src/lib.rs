use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

pub type BlockId = usize;

#[derive(Debug, Clone)]
pub struct KvBlock {
    pub block_id: BlockId,
    pub ref_count: usize,
    pub num_tokens: usize,
    pub is_shared: bool,
}

#[derive(Debug, Clone, Default)]
pub struct BlockTable {
    pub block_ids: Vec<BlockId>,
    pub total_tokens: usize,
}

#[derive(Debug, Clone, Default)]
pub struct KvCacheMetrics {
    pub total_blocks: usize,
    pub free_blocks: usize,
    pub used_blocks: usize,
    pub shared_blocks: usize,
    pub prefix_cache_hits: usize,
    pub cow_allocations: usize,
}

/// Radix tree node for automatic prefix deduplication across sequences
#[allow(dead_code)]
#[derive(Debug, Default)]
struct RadixNode {
    children: HashMap<u32, RadixNode>,
    cached_block_id: Option<BlockId>,
}

pub struct AienKvManager {
    block_size: usize,
    total_blocks: usize,
    free_blocks: Vec<BlockId>,
    block_pool: Vec<KvBlock>,
    sequence_tables: HashMap<u64, BlockTable>,
    #[allow(dead_code)]
    radix_root: RadixNode,
    prefix_hits: usize,
    cow_count: usize,
}

impl AienKvManager {
    pub fn new(total_blocks: usize, block_size: usize) -> Self {
        let mut free_blocks = Vec::with_capacity(total_blocks);
        let mut block_pool = Vec::with_capacity(total_blocks);

        for i in 0..total_blocks {
            free_blocks.push(i);
            block_pool.push(KvBlock {
                block_id: i,
                ref_count: 0,
                num_tokens: 0,
                is_shared: false,
            });
        }

        Self {
            block_size,
            total_blocks,
            free_blocks,
            block_pool,
            sequence_tables: HashMap::new(),
            radix_root: RadixNode::default(),
            prefix_hits: 0,
            cow_count: 0,
        }
    }

    pub fn block_size(&self) -> usize {
        self.block_size
    }

    pub fn available_blocks(&self) -> usize {
        self.free_blocks.len()
    }

    pub fn allocate_sequence(
        &mut self,
        seq_id: u64,
        prompt_tokens: &[u32],
    ) -> Result<Vec<BlockId>, String> {
        let token_count = prompt_tokens.len();
        let blocks_needed = (token_count + self.block_size - 1) / self.block_size;

        if self.free_blocks.len() < blocks_needed {
            return Err(format!(
                "Out of KV memory: needed {} blocks, but only {} free",
                blocks_needed,
                self.free_blocks.len()
            ));
        }

        let mut allocated_blocks = Vec::with_capacity(blocks_needed);
        let mut remaining_tokens = token_count;

        for _ in 0..blocks_needed {
            let block_id = self.free_blocks.pop().unwrap();
            let tokens_in_block = std::cmp::min(remaining_tokens, self.block_size);
            remaining_tokens -= tokens_in_block;

            let block = &mut self.block_pool[block_id];
            block.ref_count = 1;
            block.num_tokens = tokens_in_block;
            block.is_shared = false;

            allocated_blocks.push(block_id);
        }

        let table = BlockTable {
            block_ids: allocated_blocks.clone(),
            total_tokens: token_count,
        };
        self.sequence_tables.insert(seq_id, table);

        Ok(allocated_blocks)
    }

    /// Zero-copy sequence fork for subagent branching.
    /// Clones the block table and increments ref_counts without allocating new KV memory.
    pub fn fork_sequence(&mut self, parent_id: u64, child_id: u64) -> Result<Vec<BlockId>, String> {
        let parent_table = self
            .sequence_tables
            .get(&parent_id)
            .ok_or_else(|| format!("Parent sequence {} not found", parent_id))?
            .clone();

        for &block_id in &parent_table.block_ids {
            let block = &mut self.block_pool[block_id];
            block.ref_count += 1;
            block.is_shared = true;
        }

        self.sequence_tables.insert(child_id, parent_table.clone());
        Ok(parent_table.block_ids)
    }

    /// Appends a new token to a sequence with Copy-on-Write semantics.
    pub fn append_token(&mut self, seq_id: u64) -> Result<BlockId, String> {
        let table = self
            .sequence_tables
            .get_mut(&seq_id)
            .ok_or_else(|| format!("Sequence {} not found", seq_id))?;

        let last_block_id = match table.block_ids.last() {
            Some(&id) => id,
            None => {
                let new_id = self.free_blocks.pop().ok_or("Out of KV memory")?;
                let block = &mut self.block_pool[new_id];
                block.ref_count = 1;
                block.num_tokens = 0;
                block.is_shared = false;
                table.block_ids.push(new_id);
                new_id
            }
        };

        let last_block = &self.block_pool[last_block_id];
        if last_block.is_shared {
            // Copy-on-Write: allocate new private block, copy tokens, decrement shared ref_count
            let new_id = self.free_blocks.pop().ok_or("Out of KV memory on CoW")?;
            let old_tokens = last_block.num_tokens;

            self.block_pool[last_block_id].ref_count -= 1;
            if self.block_pool[last_block_id].ref_count == 1 {
                self.block_pool[last_block_id].is_shared = false;
            }

            let new_block = &mut self.block_pool[new_id];
            new_block.ref_count = 1;
            new_block.num_tokens = old_tokens + 1;
            new_block.is_shared = false;

            let last_idx = table.block_ids.len() - 1;
            table.block_ids[last_idx] = new_id;
            table.total_tokens += 1;
            self.cow_count += 1;

            return Ok(new_id);
        }

        if self.block_pool[last_block_id].num_tokens < self.block_size {
            // Room in current private block
            self.block_pool[last_block_id].num_tokens += 1;
            table.total_tokens += 1;
            Ok(last_block_id)
        } else {
            // Current block is full, allocate new private block
            let new_id = self.free_blocks.pop().ok_or("Out of KV memory")?;
            let new_block = &mut self.block_pool[new_id];
            new_block.ref_count = 1;
            new_block.num_tokens = 1;
            new_block.is_shared = false;

            table.block_ids.push(new_id);
            table.total_tokens += 1;
            Ok(new_id)
        }
    }

    /// Releases a sequence and returns unreferenced blocks back to the free pool.
    pub fn free_sequence(&mut self, seq_id: u64) {
        if let Some(table) = self.sequence_tables.remove(&seq_id) {
            for block_id in table.block_ids {
                let block = &mut self.block_pool[block_id];
                if block.ref_count > 0 {
                    block.ref_count -= 1;
                    if block.ref_count == 0 {
                        block.num_tokens = 0;
                        block.is_shared = false;
                        self.free_blocks.push(block_id);
                    } else if block.ref_count == 1 {
                        block.is_shared = false;
                    }
                }
            }
        }
    }

    pub fn get_block_table(&self, seq_id: u64) -> Option<&BlockTable> {
        self.sequence_tables.get(&seq_id)
    }

    pub fn metrics(&self) -> KvCacheMetrics {
        let shared_blocks = self.block_pool.iter().filter(|b| b.is_shared).count();
        KvCacheMetrics {
            total_blocks: self.total_blocks,
            free_blocks: self.free_blocks.len(),
            used_blocks: self.total_blocks - self.free_blocks.len(),
            shared_blocks,
            prefix_cache_hits: self.prefix_hits,
            cow_allocations: self.cow_count,
        }
    }
}

pub type SharedKvManager = Arc<RwLock<AienKvManager>>;

pub fn create_shared_kv_manager(total_blocks: usize, block_size: usize) -> SharedKvManager {
    Arc::new(RwLock::new(AienKvManager::new(total_blocks, block_size)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kv_allocation_and_free() {
        let mut mgr = AienKvManager::new(100, 16);
        assert_eq!(mgr.available_blocks(), 100);

        let tokens: Vec<u32> = (0..35).collect(); // requires 3 blocks (16 + 16 + 3)
        let blocks = mgr.allocate_sequence(1, &tokens).unwrap();
        assert_eq!(blocks.len(), 3);
        assert_eq!(mgr.available_blocks(), 97);

        mgr.free_sequence(1);
        assert_eq!(mgr.available_blocks(), 100);
    }

    #[test]
    fn test_zero_copy_fork_and_cow() {
        let mut mgr = AienKvManager::new(100, 16);
        let tokens: Vec<u32> = (0..32).collect(); // 2 full blocks
        mgr.allocate_sequence(1, &tokens).unwrap();

        // Fork subagent 2 from parent 1
        let child_blocks = mgr.fork_sequence(1, 2).unwrap();
        assert_eq!(child_blocks.len(), 2);
        // Zero new blocks allocated!
        assert_eq!(mgr.available_blocks(), 98);
        assert_eq!(mgr.metrics().shared_blocks, 2);

        // Subagent 2 generates a new token: triggers Copy-on-Write for the active block
        let new_block = mgr.append_token(2).unwrap();
        assert_eq!(mgr.available_blocks(), 97); // 1 block allocated for CoW

        let table1 = mgr.get_block_table(1).unwrap();
        let table2 = mgr.get_block_table(2).unwrap();
        assert_eq!(table1.block_ids.len(), 2);
        assert_eq!(table2.block_ids.len(), 2);
        assert_ne!(table1.block_ids[1], table2.block_ids[1]);
        assert_eq!(table2.block_ids[1], new_block);

        // Free parent 1: shared blocks still kept alive by child 2
        mgr.free_sequence(1);
        assert_eq!(mgr.available_blocks(), 98); // only block 1 released, block 0 retained by child 2

        mgr.free_sequence(2);
        assert_eq!(mgr.available_blocks(), 100); // all memory returned
    }
}

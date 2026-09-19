use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

pub type BlockId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KvDType {
    Fp4,
    Fp8,
    Bf16,
    Fp16,
    Fp32,
}

impl KvDType {
    pub fn bytes_per_element(&self) -> f32 {
        match self {
            KvDType::Fp4 => 0.5,
            KvDType::Fp8 => 1.0,
            KvDType::Bf16 | KvDType::Fp16 => 2.0,
            KvDType::Fp32 => 4.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct KvPoolConfig {
    pub num_blocks: usize,
    pub block_size: usize,
    pub num_layers: usize,
    pub num_kv_heads: usize,
    pub head_dim: usize,
    pub dtype: KvDType,
}

impl KvPoolConfig {
    pub fn for_qwen2_5_7b(num_blocks: usize, block_size: usize, dtype: KvDType) -> Self {
        Self {
            num_blocks,
            block_size,
            num_layers: 28,
            num_kv_heads: 4,
            head_dim: 128,
            dtype,
        }
    }

    pub fn for_nemotron_30b(num_blocks: usize, block_size: usize, dtype: KvDType) -> Self {
        Self {
            num_blocks,
            block_size,
            num_layers: 48,
            num_kv_heads: 8,
            head_dim: 128,
            dtype,
        }
    }

    pub fn bytes_per_block(&self) -> usize {
        // 2 for Key and Value tensors
        let elements = 2 * self.num_layers * self.block_size * self.num_kv_heads * self.head_dim;
        (elements as f32 * self.dtype.bytes_per_element()).ceil() as usize
    }

    pub fn total_bytes(&self) -> usize {
        self.num_blocks * self.bytes_per_block()
    }
}

/// Physical unified-memory KV tensor pool backing virtual block IDs on hardware silicon.
pub struct UnifiedKvTensorPool {
    config: KvPoolConfig,
    base_ptr: *mut u8,
    total_bytes: usize,
    block_bytes: usize,
    is_mmap: bool,
}

unsafe impl Send for UnifiedKvTensorPool {}
unsafe impl Sync for UnifiedKvTensorPool {}

impl UnifiedKvTensorPool {
    pub fn allocate(config: KvPoolConfig) -> Result<Self, String> {
        let total_bytes = config.total_bytes();
        let block_bytes = config.bytes_per_block();

        if total_bytes == 0 || block_bytes == 0 {
            return Err("Cannot allocate zero-sized KV tensor pool".to_string());
        }

        // Allocate page-aligned coherent unified memory via mmap
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                total_bytes,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            return Err(format!(
                "Failed to mmap {} bytes for KV tensor pool: errno {}",
                total_bytes,
                std::io::Error::last_os_error()
            ));
        }

        let base_ptr = ptr as *mut u8;

        // Attempt memory pinning (mlock) for deterministic latency without page faults
        unsafe {
            let _ = libc::mlock(ptr, total_bytes);
        }

        Ok(Self {
            config,
            base_ptr,
            total_bytes,
            block_bytes,
            is_mmap: true,
        })
    }

    pub fn config(&self) -> &KvPoolConfig {
        &self.config
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn block_bytes(&self) -> usize {
        self.block_bytes
    }

    pub fn base_ptr(&self) -> *mut u8 {
        self.base_ptr
    }

    #[inline]
    pub fn offset_for_block(&self, block_id: BlockId) -> usize {
        assert!(block_id < self.config.num_blocks, "BlockId {} exceeds pool size {}", block_id, self.config.num_blocks);
        block_id * self.block_bytes
    }

    #[inline]
    pub fn block_ptr(&self, block_id: BlockId) -> *mut u8 {
        unsafe { self.base_ptr.add(self.offset_for_block(block_id)) }
    }

    pub fn copy_block(&mut self, src_block: BlockId, dst_block: BlockId) {
        let src = self.block_ptr(src_block);
        let dst = self.block_ptr(dst_block);
        unsafe {
            std::ptr::copy_nonoverlapping(src, dst, self.block_bytes);
        }
    }

    pub fn zero_block(&mut self, block_id: BlockId) {
        let ptr = self.block_ptr(block_id);
        unsafe {
            std::ptr::write_bytes(ptr, 0, self.block_bytes);
        }
    }
}

impl Drop for UnifiedKvTensorPool {
    fn drop(&mut self) {
        if self.is_mmap && !self.base_ptr.is_null() {
            unsafe {
                let _ = libc::munlock(self.base_ptr as *const libc::c_void, self.total_bytes);
                libc::munmap(self.base_ptr as *mut libc::c_void, self.total_bytes);
            }
        }
    }
}

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
    pub physical_bytes_allocated: usize,
}

/// Functional radix node for automatic prefix deduplication across sequences
#[derive(Debug, Default)]
pub struct RadixNode {
    children: HashMap<u32, RadixNode>,
    cached_block_id: Option<BlockId>,
}

impl RadixNode {
    pub fn insert_prefix(&mut self, tokens: &[u32], block_ids: &[BlockId], block_size: usize) {
        if tokens.is_empty() || block_ids.is_empty() {
            return;
        }

        let mut current = self;
        for (i, &token) in tokens.iter().enumerate() {
            current = current.children.entry(token).or_default();
            // At block boundary, store the corresponding block id
            if (i + 1) % block_size == 0 {
                let block_idx = (i + 1) / block_size - 1;
                if block_idx < block_ids.len() {
                    current.cached_block_id = Some(block_ids[block_idx]);
                }
            }
        }
    }

    pub fn find_matching_prefix(&self, tokens: &[u32], block_size: usize) -> Vec<BlockId> {
        let mut matched_blocks = Vec::new();
        let mut current = self;

        for (i, &token) in tokens.iter().enumerate() {
            if let Some(next) = current.children.get(&token) {
                current = next;
                if (i + 1) % block_size == 0 {
                    if let Some(block_id) = current.cached_block_id {
                        matched_blocks.push(block_id);
                    } else {
                        break;
                    }
                }
            } else {
                break;
            }
        }

        matched_blocks
    }
}

pub struct AienKvManager {
    block_size: usize,
    total_blocks: usize,
    free_blocks: Vec<BlockId>,
    block_pool: Vec<KvBlock>,
    sequence_tables: HashMap<u64, BlockTable>,
    radix_root: RadixNode,
    prefix_hits: usize,
    cow_count: usize,
    tensor_pool: Option<UnifiedKvTensorPool>,
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
            tensor_pool: None,
        }
    }

    pub fn with_tensor_pool(total_blocks: usize, block_size: usize, config: KvPoolConfig) -> Result<Self, String> {
        let mut mgr = Self::new(total_blocks, block_size);
        let pool = UnifiedKvTensorPool::allocate(config)?;
        mgr.tensor_pool = Some(pool);
        Ok(mgr)
    }

    pub fn attach_tensor_pool(&mut self, config: KvPoolConfig) -> Result<(), String> {
        let pool = UnifiedKvTensorPool::allocate(config)?;
        self.tensor_pool = Some(pool);
        Ok(())
    }

    pub fn tensor_pool(&self) -> Option<&UnifiedKvTensorPool> {
        self.tensor_pool.as_ref()
    }

    pub fn tensor_pool_mut(&mut self) -> Option<&mut UnifiedKvTensorPool> {
        self.tensor_pool.as_mut()
    }

    pub fn block_size(&self) -> usize {
        self.block_size
    }

    pub fn total_blocks(&self) -> usize {
        self.total_blocks
    }

    pub fn available_blocks(&self) -> usize {
        self.free_blocks.len()
    }

    /// Allocates KV blocks for a sequence with automatic prefix caching lookup.
    pub fn allocate_sequence(
        &mut self,
        seq_id: u64,
        prompt_tokens: &[u32],
    ) -> Result<Vec<BlockId>, String> {
        let token_count = prompt_tokens.len();

        // 1. Check radix prefix cache for reusable prefix blocks
        let cached_prefix_blocks = self.radix_root.find_matching_prefix(prompt_tokens, self.block_size);
        let reused_block_count = cached_prefix_blocks.len();
        let reused_tokens = reused_block_count * self.block_size;

        if reused_block_count > 0 {
            self.prefix_hits += reused_tokens;
        }

        let remaining_tokens = token_count.saturating_sub(reused_tokens);
        let blocks_needed = if remaining_tokens == 0 {
            0
        } else {
            (remaining_tokens + self.block_size - 1) / self.block_size
        };

        if self.free_blocks.len() < blocks_needed {
            return Err(format!(
                "Out of KV memory: needed {} blocks, but only {} free",
                blocks_needed,
                self.free_blocks.len()
            ));
        }

        let mut allocated_blocks = Vec::with_capacity(reused_block_count + blocks_needed);

        // Mark reused prefix blocks
        for &block_id in &cached_prefix_blocks {
            let block = &mut self.block_pool[block_id];
            block.ref_count += 1;
            block.is_shared = true;
            allocated_blocks.push(block_id);
        }

        // Allocate new blocks for remaining tokens
        let mut tokens_left = remaining_tokens;
        let mut newly_allocated = Vec::with_capacity(blocks_needed);

        for _ in 0..blocks_needed {
            let block_id = self.free_blocks.pop().unwrap();
            let tokens_in_block = std::cmp::min(tokens_left, self.block_size);
            tokens_left -= tokens_in_block;

            let block = &mut self.block_pool[block_id];
            block.ref_count = 1;
            block.num_tokens = tokens_in_block;
            block.is_shared = false;

            allocated_blocks.push(block_id);
            newly_allocated.push(block_id);
        }

        // Index newly allocated sequence into radix tree for future prefix hits
        self.radix_root.insert_prefix(prompt_tokens, &allocated_blocks, self.block_size);

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

            // Physical block copy if tensor pool attached
            if let Some(pool) = &mut self.tensor_pool {
                pool.copy_block(last_block_id, new_id);
            }

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
        let physical_bytes_allocated = self.tensor_pool.as_ref().map(|p| p.total_bytes()).unwrap_or(0);
        KvCacheMetrics {
            total_blocks: self.total_blocks,
            free_blocks: self.free_blocks.len(),
            used_blocks: self.total_blocks - self.free_blocks.len(),
            shared_blocks,
            prefix_cache_hits: self.prefix_hits,
            cow_allocations: self.cow_count,
            physical_bytes_allocated,
        }
    }
}

pub type SharedKvManager = Arc<RwLock<AienKvManager>>;

pub fn create_shared_kv_manager(total_blocks: usize, block_size: usize) -> SharedKvManager {
    Arc::new(RwLock::new(AienKvManager::new(total_blocks, block_size)))
}

pub fn create_shared_kv_manager_with_pool(
    total_blocks: usize,
    block_size: usize,
    config: KvPoolConfig,
) -> Result<SharedKvManager, String> {
    let mgr = AienKvManager::with_tensor_pool(total_blocks, block_size, config)?;
    Ok(Arc::new(RwLock::new(mgr)))
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
        // Zero new blocks allocated
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
        assert_eq!(mgr.available_blocks(), 98);

        mgr.free_sequence(2);
        assert_eq!(mgr.available_blocks(), 100);
    }

    #[test]
    fn test_functional_prefix_caching() {
        let mut mgr = AienKvManager::new(100, 16);
        let shared_prompt: Vec<u32> = (0..32).collect(); // 2 blocks

        // Sequence 1 registers the shared prompt
        let blocks1 = mgr.allocate_sequence(10, &shared_prompt).unwrap();
        assert_eq!(blocks1.len(), 2);
        assert_eq!(mgr.available_blocks(), 98);
        assert_eq!(mgr.metrics().prefix_cache_hits, 0);

        // Sequence 2 arrives with identical shared prompt + 5 new tokens
        let mut sequence2_prompt = shared_prompt.clone();
        sequence2_prompt.extend_from_slice(&[100, 101, 102, 103, 104]);

        let blocks2 = mgr.allocate_sequence(20, &sequence2_prompt).unwrap();
        // Should reuse 2 blocks from prefix cache and allocate 1 new block for 5 new tokens
        assert_eq!(blocks2.len(), 3);
        assert_eq!(blocks2[0], blocks1[0]);
        assert_eq!(blocks2[1], blocks1[1]);
        assert_eq!(mgr.available_blocks(), 97); // only 1 block deducted
        assert_eq!(mgr.metrics().prefix_cache_hits, 32); // 32 tokens hit
    }

    #[test]
    fn test_physical_unified_tensor_pool() {
        let pool_cfg = KvPoolConfig::for_qwen2_5_7b(10, 16, KvDType::Fp4);
        let block_bytes = pool_cfg.bytes_per_block();
        let total_bytes = pool_cfg.total_bytes();
        assert!(block_bytes > 0);
        assert_eq!(total_bytes, block_bytes * 10);

        let mut pool = UnifiedKvTensorPool::allocate(pool_cfg).unwrap();
        assert!(!pool.base_ptr().is_null());

        // Write pattern into block 0
        let ptr0 = pool.block_ptr(0);
        unsafe {
            std::ptr::write_bytes(ptr0, 0xAA, 1024);
        }

        // Copy block 0 to block 1
        pool.copy_block(0, 1);
        let ptr1 = pool.block_ptr(1);
        unsafe {
            let slice = std::slice::from_raw_parts(ptr1, 1024);
            assert_eq!(slice[0], 0xAA);
            assert_eq!(slice[1023], 0xAA);
        }

        // Zero block 1
        pool.zero_block(1);
        unsafe {
            let slice = std::slice::from_raw_parts(ptr1, 1024);
            assert_eq!(slice[0], 0x00);
            assert_eq!(slice[1023], 0x00);
        }
    }
}

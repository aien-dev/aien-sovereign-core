//! Native Paged KV-Cache Manager with physical unified-memory tensors,
//! reference-counted copy-on-write branching, and prefix caching.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub type BlockId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvPoolConfig {
    pub num_blocks: usize,
    pub block_size: usize,
    pub num_layers: usize,
    pub num_kv_heads: usize,
    pub head_dim: usize,
    pub dtype: KvDType,
}

impl KvPoolConfig {
    pub fn for_tinyllama(num_blocks: usize, block_size: usize, dtype: KvDType) -> Self {
        Self {
            num_blocks,
            block_size,
            num_layers: 22,
            num_kv_heads: 4,
            head_dim: 64,
            dtype,
        }
    }

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

    pub fn for_model(
        num_blocks: usize,
        block_size: usize,
        num_layers: usize,
        num_kv_heads: usize,
        head_dim: usize,
        dtype: KvDType,
    ) -> Self {
        Self {
            num_blocks,
            block_size,
            num_layers,
            num_kv_heads,
            head_dim,
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

pub type CustomAllocFn = unsafe fn(usize) -> *mut u8;
pub type CustomFreeFn = unsafe fn(*mut u8, usize);

static CUSTOM_ALLOCATOR: parking_lot::RwLock<Option<(CustomAllocFn, CustomFreeFn)>> =
    parking_lot::RwLock::new(None);

/// Registers a custom physical memory allocator (e.g. cudaMallocManaged on Blackwell).
pub fn register_unified_allocator(alloc_fn: CustomAllocFn, free_fn: CustomFreeFn) {
    let mut g = CUSTOM_ALLOCATOR.write();
    *g = Some((alloc_fn, free_fn));
}

/// Unregisters any custom physical memory allocator.
pub fn unregister_unified_allocator() {
    let mut g = CUSTOM_ALLOCATOR.write();
    *g = None;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolMemoryKind {
    Mmap,
    Heap,
    CustomManaged,
}

/// Physical unified-memory KV tensor pool backing virtual block IDs on hardware silicon.
pub struct UnifiedKvTensorPool {
    config: KvPoolConfig,
    base_ptr: *mut u8,
    total_bytes: usize,
    block_bytes: usize,
    memory_kind: PoolMemoryKind,
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

        if let Some((alloc_fn, _)) = *CUSTOM_ALLOCATOR.read() {
            let ptr = unsafe { alloc_fn(total_bytes) };
            if !ptr.is_null() {
                return Ok(Self {
                    config,
                    base_ptr: ptr,
                    total_bytes,
                    block_bytes,
                    memory_kind: PoolMemoryKind::CustomManaged,
                });
            }
        }

        #[cfg(unix)]
        let (base_ptr, memory_kind) = {
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

            unsafe {
                let _ = libc::mlock(ptr, total_bytes);
            }

            (ptr as *mut u8, PoolMemoryKind::Mmap)
        };

        #[cfg(not(unix))]
        let (base_ptr, memory_kind) = {
            let layout = std::alloc::Layout::from_size_align(total_bytes, 4096)
                .map_err(|e| format!("Invalid memory layout: {}", e))?;
            let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
            if ptr.is_null() {
                return Err(format!(
                    "Failed to allocate {} bytes for KV tensor pool",
                    total_bytes
                ));
            }
            (ptr, PoolMemoryKind::Heap)
        };

        Ok(Self {
            config,
            base_ptr,
            total_bytes,
            block_bytes,
            memory_kind,
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
        assert!(
            block_id < self.config.num_blocks,
            "BlockId {} exceeds pool size {}",
            block_id,
            self.config.num_blocks
        );
        block_id * self.block_bytes
    }

    #[inline]
    pub fn block_ptr(&self, block_id: BlockId) -> *mut u8 {
        unsafe { self.base_ptr.add(self.offset_for_block(block_id)) }
    }

    pub fn copy_block(&mut self, src_block: BlockId, dst_block: BlockId) {
        if src_block == dst_block || self.base_ptr.is_null() || self.block_bytes == 0 {
            return;
        }
        let src = self.block_ptr(src_block);
        let dst = self.block_ptr(dst_block);
        unsafe {
            std::ptr::copy(src, dst, self.block_bytes);
        }
    }

    pub fn zero_block(&mut self, block_id: BlockId) {
        let ptr = self.block_ptr(block_id);
        unsafe {
            std::ptr::write_bytes(ptr, 0, self.block_bytes);
        }
    }

    #[inline]
    pub fn element_offset(
        &self,
        block_id: BlockId,
        layer_idx: usize,
        is_value: bool,
        token_in_block: usize,
    ) -> usize {
        assert!(
            block_id < self.config.num_blocks,
            "BlockId {} out of range",
            block_id
        );
        assert!(
            layer_idx < self.config.num_layers,
            "LayerIdx {} out of range",
            layer_idx
        );
        assert!(
            token_in_block < self.config.block_size,
            "TokenInBlock {} out of range (block_size {})",
            token_in_block,
            self.config.block_size
        );

        let kv_dim = self.config.num_kv_heads * self.config.head_dim;
        let bytes_per_elem = self.config.dtype.bytes_per_element();
        let kv_stride =
            (self.config.block_size as f32 * kv_dim as f32 * bytes_per_elem).ceil() as usize;
        let layer_stride = 2 * kv_stride;
        let token_stride = (kv_dim as f32 * bytes_per_elem).ceil() as usize;

        let block_base = self.offset_for_block(block_id);
        block_base
            + layer_idx * layer_stride
            + if is_value { kv_stride } else { 0 }
            + token_in_block * token_stride
    }

    pub fn write_token_kv(
        &mut self,
        block_id: BlockId,
        layer_idx: usize,
        token_in_block: usize,
        k: &[f32],
        v: &[f32],
    ) {
        let kv_dim = self.config.num_kv_heads * self.config.head_dim;
        assert_eq!(k.len(), kv_dim, "K vector length must equal kv_dim");
        assert_eq!(v.len(), kv_dim, "V vector length must equal kv_dim");

        let k_offset = self.element_offset(block_id, layer_idx, false, token_in_block);
        let v_offset = self.element_offset(block_id, layer_idx, true, token_in_block);

        unsafe {
            let k_ptr = self.base_ptr.add(k_offset) as *mut f32;
            let v_ptr = self.base_ptr.add(v_offset) as *mut f32;
            std::ptr::copy_nonoverlapping(k.as_ptr(), k_ptr, kv_dim);
            std::ptr::copy_nonoverlapping(v.as_ptr(), v_ptr, kv_dim);
        }
    }

    pub fn read_token_kv(
        &self,
        block_id: BlockId,
        layer_idx: usize,
        token_in_block: usize,
        k_out: &mut [f32],
        v_out: &mut [f32],
    ) {
        let kv_dim = self.config.num_kv_heads * self.config.head_dim;
        assert_eq!(k_out.len(), kv_dim, "K_out vector length must equal kv_dim");
        assert_eq!(v_out.len(), kv_dim, "V_out vector length must equal kv_dim");

        let k_offset = self.element_offset(block_id, layer_idx, false, token_in_block);
        let v_offset = self.element_offset(block_id, layer_idx, true, token_in_block);

        unsafe {
            let k_ptr = self.base_ptr.add(k_offset) as *const f32;
            let v_ptr = self.base_ptr.add(v_offset) as *const f32;
            std::ptr::copy_nonoverlapping(k_ptr, k_out.as_mut_ptr(), kv_dim);
            std::ptr::copy_nonoverlapping(v_ptr, v_out.as_mut_ptr(), kv_dim);
        }
    }

    pub fn gather_layer_kv(
        &self,
        block_ids: &[BlockId],
        total_tokens: usize,
        layer_idx: usize,
        flat_k: &mut Vec<f32>,
        flat_v: &mut Vec<f32>,
    ) {
        let kv_dim = self.config.num_kv_heads * self.config.head_dim;
        let needed_elements = total_tokens * kv_dim;
        flat_k.resize(needed_elements, 0.0);
        flat_v.resize(needed_elements, 0.0);

        let mut tokens_gathered = 0;
        for &block_id in block_ids {
            if tokens_gathered >= total_tokens {
                break;
            }
            let tokens_in_this_block = (total_tokens - tokens_gathered).min(self.config.block_size);
            if tokens_in_this_block == 0 {
                continue;
            }
            let k_block_offset = self.element_offset(block_id, layer_idx, false, 0);
            let v_block_offset = self.element_offset(block_id, layer_idx, true, 0);
            let chunk_elements = tokens_in_this_block * kv_dim;
            let dst_offset = tokens_gathered * kv_dim;

            unsafe {
                let k_src = self.base_ptr.add(k_block_offset) as *const f32;
                let v_src = self.base_ptr.add(v_block_offset) as *const f32;
                let k_dst = flat_k.as_mut_ptr().add(dst_offset);
                let v_dst = flat_v.as_mut_ptr().add(dst_offset);
                std::ptr::copy_nonoverlapping(k_src, k_dst, chunk_elements);
                std::ptr::copy_nonoverlapping(v_src, v_dst, chunk_elements);
            }
            tokens_gathered += tokens_in_this_block;
        }
    }

    #[inline]
    pub fn memory_kind(&self) -> PoolMemoryKind {
        self.memory_kind
    }

    #[inline]
    pub fn token_kv_offset(
        &self,
        block_id: BlockId,
        layer_idx: usize,
        is_value: bool,
        token_in_block: usize,
    ) -> usize {
        self.element_offset(block_id, layer_idx, is_value, token_in_block)
    }

    #[inline]
    pub unsafe fn token_kv_ptr(
        &self,
        block_id: BlockId,
        layer_idx: usize,
        is_value: bool,
        token_in_block: usize,
    ) -> *const u8 {
        let offset = self.element_offset(block_id, layer_idx, is_value, token_in_block);
        self.base_ptr.add(offset)
    }
}

impl Drop for UnifiedKvTensorPool {
    fn drop(&mut self) {
        if !self.base_ptr.is_null() {
            match self.memory_kind {
                PoolMemoryKind::CustomManaged => {
                    if let Some((_, free_fn)) = *CUSTOM_ALLOCATOR.read() {
                        unsafe {
                            free_fn(self.base_ptr, self.total_bytes);
                        }
                    }
                }
                PoolMemoryKind::Mmap => {
                    #[cfg(unix)]
                    unsafe {
                        let _ =
                            libc::munlock(self.base_ptr as *const libc::c_void, self.total_bytes);
                        libc::munmap(self.base_ptr as *mut libc::c_void, self.total_bytes);
                    }
                }
                PoolMemoryKind::Heap => {
                    #[cfg(not(unix))]
                    unsafe {
                        if let Ok(layout) =
                            std::alloc::Layout::from_size_align(self.total_bytes, 4096)
                        {
                            std::alloc::dealloc(self.base_ptr, layout);
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvBlock {
    pub block_id: BlockId,
    pub ref_count: usize,
    pub num_tokens: usize,
    pub is_shared: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlockTable {
    pub block_ids: Vec<BlockId>,
    pub total_tokens: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct KvCacheMetrics {
    pub total_blocks: usize,
    pub free_blocks: usize,
    pub used_blocks: usize,
    pub logical_pages: usize,
    pub physical_pages: usize,
    pub shared_pages: usize,
    pub private_pages: usize,
    pub cow_faults: usize,
    pub physical_kv_bytes: usize,
    pub bytes_saved_vs_full_copy: usize,
    pub prefix_cache_hits: usize,
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

    pub fn with_tensor_pool(
        total_blocks: usize,
        block_size: usize,
        config: KvPoolConfig,
    ) -> Result<Self, String> {
        let mut mgr = Self::new(total_blocks, block_size);
        mgr.attach_tensor_pool(config)?;
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

    pub fn has_sequence(&self, seq_id: u64) -> bool {
        self.sequence_tables.contains_key(&seq_id)
    }

    pub fn active_sequence_count(&self) -> usize {
        self.sequence_tables.len()
    }

    pub fn allocate_sequence(
        &mut self,
        seq_id: u64,
        prompt_tokens: &[u32],
    ) -> Result<Vec<BlockId>, String> {
        let token_count = prompt_tokens.len();

        let cached_prefix_blocks = self
            .radix_root
            .find_matching_prefix(prompt_tokens, self.block_size);
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

        for &block_id in &cached_prefix_blocks {
            let block = &mut self.block_pool[block_id];
            if block.ref_count == 0 {
                if let Some(pos) = self.free_blocks.iter().position(|&x| x == block_id) {
                    self.free_blocks.swap_remove(pos);
                }
            }
            block.ref_count += 1;
            block.is_shared = block.ref_count > 1;
            allocated_blocks.push(block_id);
        }

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

        self.radix_root
            .insert_prefix(prompt_tokens, &allocated_blocks, self.block_size);

        let table = BlockTable {
            block_ids: allocated_blocks.clone(),
            total_tokens: token_count,
        };
        self.sequence_tables.insert(seq_id, table);

        Ok(allocated_blocks)
    }

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

    pub fn fork_context(&mut self, parent_id: u64, child_id: u64) -> Result<Vec<BlockId>, String> {
        self.fork_sequence(parent_id, child_id)
    }

    pub fn append_token(&mut self, seq_id: u64) -> Result<BlockId, String> {
        let (block_id, _) = self.append_token_with_slot(seq_id)?;
        Ok(block_id)
    }

    pub fn append_token_with_slot(&mut self, seq_id: u64) -> Result<(BlockId, usize), String> {
        let block_size = self.block_size;
        let table = self
            .sequence_tables
            .get_mut(&seq_id)
            .ok_or_else(|| format!("Sequence {} not found", seq_id))?;

        if table.block_ids.is_empty() {
            let new_id = self.free_blocks.pop().ok_or("Out of KV memory")?;
            let block = &mut self.block_pool[new_id];
            block.ref_count = 1;
            block.num_tokens = 1;
            block.is_shared = false;
            table.block_ids.push(new_id);
            table.total_tokens = 1;
            return Ok((new_id, 0));
        }

        let last_block_id = *table.block_ids.last().unwrap();
        let last_block = &self.block_pool[last_block_id];

        if last_block.num_tokens < block_size {
            let slot = last_block.num_tokens;
            if last_block.is_shared {
                let new_id = self.free_blocks.pop().ok_or("Out of KV memory on CoW")?;
                let old_tokens = last_block.num_tokens;

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
                Ok((new_id, slot))
            } else {
                self.block_pool[last_block_id].num_tokens += 1;
                table.total_tokens += 1;
                Ok((last_block_id, slot))
            }
        } else {
            let new_id = self.free_blocks.pop().ok_or("Out of KV memory")?;
            let new_block = &mut self.block_pool[new_id];
            new_block.ref_count = 1;
            new_block.num_tokens = 1;
            new_block.is_shared = false;

            table.block_ids.push(new_id);
            table.total_tokens += 1;
            Ok((new_id, 0))
        }
    }

    pub fn write_sequence_token_kv(
        &mut self,
        seq_id: u64,
        layer_idx: usize,
        k: &[f32],
        v: &[f32],
    ) -> Result<(), String> {
        let table = self
            .sequence_tables
            .get(&seq_id)
            .ok_or_else(|| format!("Sequence {} not found", seq_id))?;
        let last_block_id = *table
            .block_ids
            .last()
            .ok_or_else(|| format!("Sequence {} has no allocated blocks", seq_id))?;
        let block = &self.block_pool[last_block_id];
        let token_in_block = block.num_tokens.saturating_sub(1);

        if let Some(pool) = &mut self.tensor_pool {
            pool.write_token_kv(last_block_id, layer_idx, token_in_block, k, v);
            Ok(())
        } else {
            Err("No physical tensor pool attached to KV manager".to_string())
        }
    }

    pub fn write_explicit_token_kv(
        &mut self,
        block_id: BlockId,
        layer_idx: usize,
        token_in_block: usize,
        k: &[f32],
        v: &[f32],
    ) -> Result<(), String> {
        if let Some(pool) = &mut self.tensor_pool {
            pool.write_token_kv(block_id, layer_idx, token_in_block, k, v);
            Ok(())
        } else {
            Err("No physical tensor pool attached to KV manager".to_string())
        }
    }

    pub fn gather_sequence_layer_kv(
        &self,
        seq_id: u64,
        layer_idx: usize,
        flat_k: &mut Vec<f32>,
        flat_v: &mut Vec<f32>,
    ) -> Result<(), String> {
        let table = self
            .sequence_tables
            .get(&seq_id)
            .ok_or_else(|| format!("Sequence {} not found", seq_id))?;
        if let Some(pool) = &self.tensor_pool {
            pool.gather_layer_kv(
                &table.block_ids,
                table.total_tokens,
                layer_idx,
                flat_k,
                flat_v,
            );
            Ok(())
        } else {
            Err("No physical tensor pool attached to KV manager".to_string())
        }
    }

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

    pub fn release_branch(&mut self, branch_id: u64) {
        self.free_sequence(branch_id);
    }

    pub fn get_block(&self, block_id: BlockId) -> Option<&KvBlock> {
        self.block_pool.get(block_id)
    }

    pub fn get_block_table(&self, seq_id: u64) -> Option<&BlockTable> {
        self.sequence_tables.get(&seq_id)
    }

    pub fn metrics(&self) -> KvCacheMetrics {
        let used_blocks = self.total_blocks - self.free_blocks.len();
        let logical_pages = self
            .sequence_tables
            .values()
            .map(|t| t.block_ids.len())
            .sum();
        let physical_pages = used_blocks;
        let shared_pages = self.block_pool.iter().filter(|b| b.ref_count > 1).count();
        let private_pages = self.block_pool.iter().filter(|b| b.ref_count == 1).count();
        let bytes_per_block = self
            .tensor_pool
            .as_ref()
            .map(|p| p.block_bytes())
            .unwrap_or(0);
        let physical_kv_bytes = physical_pages * bytes_per_block;
        let bytes_saved_vs_full_copy = if logical_pages > physical_pages {
            (logical_pages - physical_pages) * bytes_per_block
        } else {
            0
        };

        KvCacheMetrics {
            total_blocks: self.total_blocks,
            free_blocks: self.free_blocks.len(),
            used_blocks,
            logical_pages,
            physical_pages,
            shared_pages,
            private_pages,
            cow_faults: self.cow_count,
            physical_kv_bytes,
            bytes_saved_vs_full_copy,
            prefix_cache_hits: self.prefix_hits,
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

        let tokens: Vec<u32> = (0..35).collect();
        let blocks = mgr.allocate_sequence(1, &tokens).unwrap();
        assert_eq!(blocks.len(), 3);
        assert_eq!(mgr.available_blocks(), 97);

        mgr.free_sequence(1);
        assert_eq!(mgr.available_blocks(), 100);
    }

    #[test]
    fn test_zero_copy_fork_and_cow() {
        let mut mgr = AienKvManager::new(100, 16);
        let tokens: Vec<u32> = (0..30).collect();
        mgr.allocate_sequence(1, &tokens).unwrap();

        let child_blocks = mgr.fork_sequence(1, 2).unwrap();
        assert_eq!(child_blocks.len(), 2);
        assert_eq!(mgr.available_blocks(), 98);
        assert_eq!(mgr.metrics().shared_pages, 2);

        let new_block = mgr.append_token(2).unwrap();
        assert_eq!(mgr.available_blocks(), 97);

        let table1 = mgr.get_block_table(1).unwrap();
        let table2 = mgr.get_block_table(2).unwrap();
        assert_eq!(table1.block_ids.len(), 2);
        assert_eq!(table2.block_ids.len(), 2);
        assert_ne!(table1.block_ids[1], table2.block_ids[1]);
        assert_eq!(table2.block_ids[1], new_block);

        mgr.free_sequence(1);
        assert_eq!(mgr.available_blocks(), 98);

        mgr.free_sequence(2);
        assert_eq!(mgr.available_blocks(), 100);
    }

    #[test]
    fn test_functional_prefix_caching() {
        let mut mgr = AienKvManager::new(100, 16);
        let shared_prompt: Vec<u32> = (0..32).collect();

        let blocks1 = mgr.allocate_sequence(10, &shared_prompt).unwrap();
        assert_eq!(blocks1.len(), 2);
        assert_eq!(mgr.available_blocks(), 98);
        assert_eq!(mgr.metrics().prefix_cache_hits, 0);

        let mut sequence2_prompt = shared_prompt.clone();
        sequence2_prompt.extend_from_slice(&[100, 101, 102, 103, 104]);

        let blocks2 = mgr.allocate_sequence(20, &sequence2_prompt).unwrap();
        assert_eq!(blocks2.len(), 3);
        assert_eq!(blocks2[0], blocks1[0]);
        assert_eq!(blocks2[1], blocks1[1]);
        assert_eq!(mgr.available_blocks(), 97);
        assert_eq!(mgr.metrics().prefix_cache_hits, 32);
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

        let ptr0 = pool.block_ptr(0);
        unsafe {
            std::ptr::write_bytes(ptr0, 0xAA, 1024);
        }

        pool.copy_block(0, 1);
        let ptr1 = pool.block_ptr(1);
        unsafe {
            let slice = std::slice::from_raw_parts(ptr1, 1024);
            assert_eq!(slice[0], 0xAA);
            assert_eq!(slice[1023], 0xAA);
        }

        pool.zero_block(1);
        unsafe {
            let slice = std::slice::from_raw_parts(ptr1, 1024);
            assert_eq!(slice[0], 0x00);
            assert_eq!(slice[1023], 0x00);
        }
    }
}

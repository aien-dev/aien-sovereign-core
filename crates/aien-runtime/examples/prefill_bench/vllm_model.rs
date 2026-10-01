//! MODELED BASELINE: vLLM automatic prefix caching (APC), as a bookkeeping
//! model. It does no tensor math; it decides which tokens a vLLM-style engine
//! would prefill and how many physical KV blocks it would hold.
//!
//! Every modeled behavior and its source (vLLM main, read 2026-10-01):
//! - Block hash = hash(parent block hash, tuple of the block's tokens, extra
//!   keys). Only FULL blocks are cached. Default hash algo sha256.
//!   docs/design/prefix_caching.md ("Block hash", "We only cache full blocks");
//!   vllm/config/cache.py (`prefix_caching_hash_algo = "sha256"`,
//!   `enable_prefix_caching = True`).
//! - A cache hit is at most `prompt_len - 1` tokens, because the last token is
//!   always recomputed to get logits.
//!   vllm/v1/core/kv_cache_manager.py get_computed_blocks:
//!   `max_cache_hit_length = request.num_tokens - 1`.
//! - Blocks are cached right after allocation in `allocate_slots`
//!   (`self.coordinator.cache_blocks(request, num_tokens_to_cache)`), so a
//!   request scheduled later in the SAME step can hit blocks of an earlier one.
//!   (The hit-after-allocate ordering is from that code; that same-step
//!   scheduling really produces cross-request hits for N sibling requests is
//!   inferred from it and not re-verified by a run: UNVERIFIED.)
//! - Hits are shared by reference count (no copy). A request's partially
//!   filled last block is never shared, so each request gets its own tail
//!   block. This is how a vLLM request "diverges": it never writes into a
//!   shared block. There is no copy-on-write copy of KV bytes in the prefix
//!   caching path (same doc: partial blocks stay uncached).
//! - No eviction is modeled: the bench window never fills the pool
//!   (vLLM uses LRU free queue, same doc).
//! - Block size is a parameter. vLLM's `block_size` default is platform
//!   chosen (`Field(default=None)` in vllm/config/cache.py); the bench uses
//!   AIEN's block size (16) so both sides page identically.
//!
//! Not modeled: sliding window or multimodal extra hash keys, speculative
//! decoding, preemption, chunked prefill token budgets.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

pub type Hash = [u8; 32];

fn block_hash(parent: &Hash, tokens: &[u32]) -> Hash {
    let mut h = Sha256::new();
    h.update(parent);
    for t in tokens {
        h.update(t.to_le_bytes());
    }
    h.finalize().into()
}

pub struct ModelRequest {
    pub blocks: Vec<usize>,
    pub tokens: Vec<u32>,
    /// Number of leading blocks already hashed (cached or deliberately skipped).
    hashed_blocks: usize,
    last_hash: Hash,
    /// Tokens served from cache at admission (not recomputed).
    pub cached_tokens: usize,
}

pub struct PrefixCacheModel {
    block_size: usize,
    next_block: usize,
    cache: HashMap<Hash, usize>,
    refs: HashMap<usize, usize>,
}

impl PrefixCacheModel {
    pub fn new(block_size: usize) -> Self {
        assert!(block_size > 0);
        Self {
            block_size,
            next_block: 0,
            cache: HashMap::new(),
            refs: HashMap::new(),
        }
    }

    fn alloc(&mut self) -> usize {
        let id = self.next_block;
        self.next_block += 1;
        self.refs.insert(id, 1);
        id
    }

    fn cache_full_blocks(&mut self, req: &mut ModelRequest) {
        let bs = self.block_size;
        while (req.hashed_blocks + 1) * bs <= req.tokens.len() {
            let i = req.hashed_blocks;
            let h = block_hash(&req.last_hash, &req.tokens[i * bs..(i + 1) * bs]);
            self.cache.entry(h).or_insert(req.blocks[i]);
            req.last_hash = h;
            req.hashed_blocks += 1;
        }
    }

    /// Admits a prompt: finds the longest cached full-block prefix (capped at
    /// len-1 tokens), shares those blocks, allocates the rest, caches the new
    /// full blocks.
    pub fn admit(&mut self, prompt: &[u32]) -> ModelRequest {
        let bs = self.block_size;
        let max_hit = prompt.len().saturating_sub(1);
        let mut req = ModelRequest {
            blocks: Vec::new(),
            tokens: prompt.to_vec(),
            hashed_blocks: 0,
            last_hash: [0u8; 32],
            cached_tokens: 0,
        };
        while (req.hashed_blocks + 1) * bs <= max_hit {
            let i = req.hashed_blocks;
            let h = block_hash(&req.last_hash, &prompt[i * bs..(i + 1) * bs]);
            match self.cache.get(&h) {
                Some(&b) => {
                    *self.refs.get_mut(&b).expect("cached block is live") += 1;
                    req.blocks.push(b);
                    req.last_hash = h;
                    req.hashed_blocks += 1;
                }
                None => break,
            }
        }
        req.cached_tokens = req.hashed_blocks * bs;
        while req.blocks.len() * bs < prompt.len() {
            let b = self.alloc();
            req.blocks.push(b);
        }
        self.cache_full_blocks(&mut req);
        req
    }

    /// Appends one generated token (one new block when the last is full).
    pub fn append(&mut self, req: &mut ModelRequest, token: u32) {
        req.tokens.push(token);
        if req.tokens.len() > req.blocks.len() * self.block_size {
            let b = self.alloc();
            req.blocks.push(b);
        }
        self.cache_full_blocks(req);
    }

    pub fn physical_blocks(&self) -> usize {
        self.refs.len()
    }

    pub fn logical_blocks(&self, reqs: &[ModelRequest]) -> usize {
        reqs.iter().map(|r| r.blocks.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(n: usize) -> Vec<u32> {
        (0..n as u32).collect()
    }

    #[test]
    fn second_request_hits_all_full_blocks_below_len_minus_one() {
        let mut m = PrefixCacheModel::new(16);
        let p = prompt(217);
        let a = m.admit(&p);
        assert_eq!(a.cached_tokens, 0);
        assert_eq!(a.blocks.len(), 14);
        let b = m.admit(&p);
        assert_eq!(b.cached_tokens, 208); // 13 full blocks
        assert_eq!(m.physical_blocks(), 15); // 13 shared + 2 tails
    }

    #[test]
    fn exact_multiple_still_recomputes_the_last_block() {
        let mut m = PrefixCacheModel::new(16);
        let p = prompt(64);
        m.admit(&p);
        let b = m.admit(&p);
        assert_eq!(b.cached_tokens, 48); // cap is len-1 = 63 -> 3 blocks
    }

    #[test]
    fn short_prompt_below_one_block_never_shares() {
        let mut m = PrefixCacheModel::new(16);
        let p = prompt(10);
        m.admit(&p);
        let b = m.admit(&p);
        assert_eq!(b.cached_tokens, 0);
        assert_eq!(m.physical_blocks(), 2);
    }

    #[test]
    fn different_prefix_does_not_hit_after_a_mismatch() {
        let mut m = PrefixCacheModel::new(4);
        m.admit(&[1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let b = m.admit(&[1, 2, 3, 4, 5, 6, 0, 8, 9]);
        assert_eq!(b.cached_tokens, 4);
    }

    #[test]
    fn decode_blocks_grow_per_request_and_identical_full_blocks_dedupe_only_via_hit() {
        let mut m = PrefixCacheModel::new(4);
        let p = prompt(6);
        let mut a = m.admit(&p);
        let mut b = m.admit(&p);
        // a and b share block 0; tails separate.
        assert_eq!(m.physical_blocks(), 3);
        for t in [100, 101, 102] {
            m.append(&mut a, t);
            m.append(&mut b, t);
        }
        // 6+3=9 tokens each: 3 blocks each, block 0 shared => 5 physical.
        assert_eq!(m.physical_blocks(), 5);
    }
}

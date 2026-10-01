//! One KV for runtime and backend (PREFILL-E2E-0 cut C2).
//!
//! Before this module the daemon built the spine on a pool-less KV manager
//! and the backend with no KV manager at all, so block tables in the runtime
//! were bookkeeping only and the real K/V lived in the backend's private
//! dense per-request cache. `build_shared_kv_runtime` creates ONE manager with
//! a physical tensor pool sized for the loaded model and hands the same `Arc`
//! to the spine (scheduler, swarm forks, reclaim) and to the backend (which
//! writes prefill/decode K/V into the pool blocks named by the spine's block
//! tables).
//!
//! Lives in aien-runtime (not aien-cli) because this crate owns the spine and
//! already depends on aien-inference-abi and aien-kv-cache, so the assembly
//! can be exercised by integration tests next to the other spine tests.

use std::sync::Arc;

use aien_inference_abi::{NativeTransformerBackend, TensorBackend, TransformerWeights};
use aien_kv_cache::{create_shared_kv_manager_with_pool, KvDType, KvPoolConfig, SharedKvManager};
use aien_scheduler::SchedulerConfig;

use crate::spine::AienRuntimeSpine;

/// Sizing for the shared runtime/backend KV manager.
#[derive(Debug, Clone, Copy)]
pub struct SharedKvSizing {
    /// Sequence arena capacity of the spine.
    pub arena_capacity: usize,
    /// Number of physical KV blocks in the pool (and in the block allocator).
    pub total_blocks: usize,
}

/// Builds the pooled KV manager for `weights.config`. The block size is the
/// model config's `block_size`, because the backend's paged writes index
/// blocks with `weights.config.block_size`
/// (aien-inference-abi/src/transformer_backend.rs, prefill paged path).
pub fn build_model_kv_manager(
    weights: &TransformerWeights,
    total_blocks: usize,
) -> Result<SharedKvManager, String> {
    let cfg = &weights.config;
    if cfg.block_size == 0 {
        return Err("shared KV: model config block_size is 0".to_string());
    }
    let pool_cfg = KvPoolConfig::for_model(
        total_blocks,
        cfg.block_size,
        cfg.num_layers,
        cfg.num_kv_heads,
        cfg.head_dim,
        KvDType::Fp32,
    );
    create_shared_kv_manager_with_pool(total_blocks, cfg.block_size, pool_cfg)
}

/// Builds the runtime spine and the native backend over ONE pooled KV
/// manager. The returned backend's `kv_manager` is the same `Arc` as the
/// spine's `kv_manager` (`Arc::ptr_eq`).
pub fn build_shared_kv_runtime(
    weights: TransformerWeights,
    tensor_backend: Arc<dyn TensorBackend>,
    scheduler_config: SchedulerConfig,
    sizing: SharedKvSizing,
) -> Result<(AienRuntimeSpine, NativeTransformerBackend), String> {
    let kv_manager = build_model_kv_manager(&weights, sizing.total_blocks)?;
    let spine = AienRuntimeSpine::new(sizing.arena_capacity, scheduler_config, kv_manager.clone());
    let backend =
        NativeTransformerBackend::with_shared_kv_and_backend(weights, tensor_backend, kv_manager);
    Ok((spine, backend))
}

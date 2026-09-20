use aien_inference_abi::{
    BlackwellGb10Backend, ModelConfig, NativeTransformerBackend,
    ReferenceCpuBackend, TransformerWeights,
};
use aien_kv_cache::{KvDType, PoolMemoryKind};
use std::sync::Arc;

#[test]
fn test_blackwell_bf16_paged_end_to_end_32_branches() {
    let blackwell = Arc::new(BlackwellGb10Backend::new());
    if !blackwell.is_available() {
        eprintln!("Skipping: Blackwell GB10 GPU hardware not available");
        return;
    }

    // 1. Hardware verification
    assert!(blackwell.is_available(), "Blackwell GPU backend must be available");
    println!("Blackwell GPU detected: {}", blackwell.device_name());

    let config = ModelConfig {
        num_layers: 2,
        num_heads: 4,
        num_kv_heads: 2,
        head_dim: 32,
        hidden_dim: 128,
        intermediate_dim: 256,
        vocab_size: 512,
        block_size: 16,
        ..Default::default()
    };

    let weights = TransformerWeights::reference_test_weights(&config);

    // 2. Initialize NativeTransformerBackend with BF16 paged KV pool on Blackwell
    let mut backend = NativeTransformerBackend::with_paged_kv_backend_dtype(
        weights.clone(),
        blackwell.clone(),
        1000,
        config.block_size,
        KvDType::Bf16,
    ).expect("Failed to initialize backend with BF16 paged KV pool");

    // 3. Physical pool verification
    {
        let kv_mgr = backend.kv_manager.as_ref().expect("KV manager must exist").read();
        let pool = kv_mgr.tensor_pool().expect("Tensor pool must exist");
        assert_eq!(pool.config().dtype, KvDType::Bf16, "KV pool dtype must be BF16");
        assert_eq!(
            pool.memory_kind(),
            PoolMemoryKind::CustomManaged,
            "KV pool must be allocated via CustomManaged (cudaMallocManaged)"
        );
    }

    // 4. Create shared prefix parent context (32 tokens = exactly 2 blocks)
    let prompt: Vec<u32> = (1..=32).collect();
    let parent = backend.create_context(&prompt).expect("Failed to create parent context");

    // 5. Fork 32 independent agent branches
    let mut branches = Vec::with_capacity(32);
    for _ in 0..32 {
        let b = backend.fork_context(parent).expect("Failed to fork branch");
        branches.push(b);
    }

    // 6. Verify Copy-On-Write sharing metrics
    let r_init = backend.get_usage_receipt(branches[0]).expect("Usage receipt");
    assert_eq!(r_init.shared_pages, 2, "Both prefix pages must be shared");
    assert_eq!(r_init.private_pages, 0, "Zero private pages allocated before decode");
    assert_eq!(r_init.cow_faults, 0, "Zero CoW faults at fork time");
    assert_eq!(r_init.logical_pages, 2);

    let pre_kernel_calls = blackwell.paged_attention_kernel_count();
    let pre_fallbacks = blackwell.fallback_count();

    // 7. Execute 1 decode step per branch (32 decode steps total)
    let mut generated_tokens = Vec::with_capacity(32);
    let mut branch_logits = Vec::with_capacity(32);
    for &b in &branches {
        let (tok, logits) = backend.decode_branch_step(b).expect("decode_branch_step failed");
        assert!(tok > 0, "Decoded token must be positive");
        assert_eq!(logits.len(), config.vocab_size);
        generated_tokens.push(tok);
        branch_logits.push(logits);
    }

    let post_kernel_calls = blackwell.paged_attention_kernel_count();
    let post_fallbacks = blackwell.fallback_count();

    // 8. Prove Blackwell CUDA kernel was executed for every decode step without fallback
    let kernel_call_delta = post_kernel_calls - pre_kernel_calls;
    let fallback_delta = post_fallbacks - pre_fallbacks;

    println!("Blackwell PagedAttention kernel call delta: {}", kernel_call_delta);
    println!("Blackwell fallback delta: {}", fallback_delta);

    // Each decode step passes through config.num_layers layers.
    // With 32 branches and 2 layers, kernel is called 32 * 2 = 64 times!
    let expected_kernel_calls = (32 * config.num_layers) as u64;
    assert_eq!(
        kernel_call_delta, expected_kernel_calls,
        "PagedAttention kernel calls delta must equal 32 branches * num_layers"
    );
    assert_eq!(fallback_delta, 0, "Zero fallbacks allowed on Blackwell GPU");

    // 9. All branches started from the identical parent, so initial greedy decode token must match
    let first_token = generated_tokens[0];
    for (i, &tok) in generated_tokens.iter().enumerate() {
        assert_eq!(
            tok, first_token,
            "Branch {} generated token {} differing from branch 0 token {}",
            i, tok, first_token
        );
    }

    // 10. Verify post-decode CoW receipt: 2 shared pages remain, 1 private page added per branch
    let r_post = backend.get_usage_receipt(branches[0]).expect("Usage receipt");
    assert_eq!(r_post.shared_pages, 2, "Shared prefix pages must remain shared");
    assert_eq!(r_post.private_pages, 1, "1 private page allocated for decoded token");
    assert_eq!(r_post.logical_pages, 3);
    assert!(r_post.bytes_saved_vs_full_copy > 0);

    // 11. Run CPU reference on the exact same parent and branch to check numerical parity
    let mut cpu_backend = NativeTransformerBackend::with_paged_kv_backend_dtype(
        weights,
        Arc::new(ReferenceCpuBackend::new()),
        1000,
        config.block_size,
        KvDType::Bf16,
    ).expect("Failed to create CPU reference backend");

    let cpu_parent = cpu_backend.create_context(&prompt).expect("CPU parent");
    let cpu_branch = cpu_backend.fork_context(cpu_parent).expect("CPU branch");
    let (cpu_tok, cpu_logits) = cpu_backend.decode_branch_step(cpu_branch).expect("CPU decode");

    assert_eq!(first_token, cpu_tok, "Blackwell GPU token must match Reference CPU token");

    let mut max_diff = 0.0f32;
    for i in 0..config.vocab_size {
        let diff = (branch_logits[0][i] - cpu_logits[i]).abs();
        if diff > max_diff {
            max_diff = diff;
        }
    }
    println!("Max logit diff between Blackwell sm_121 and Reference CPU: {}", max_diff);
    assert!(
        max_diff < 0.05,
        "Logit numerical divergence between Blackwell and CPU: max_diff = {}",
        max_diff
    );

    // 12. Clean up branches
    for b in branches {
        backend.release_branch(b).expect("Failed to release branch");
    }

    let kv_mgr = backend.kv_manager.as_ref().expect("KV manager").read();
    assert_eq!(kv_mgr.active_sequence_count(), 1, "Only parent context remains");
    let m = kv_mgr.metrics();
    assert_eq!(m.physical_pages, 2, "Only 2 physical pages for the root context remain");
    assert_eq!(m.shared_pages, 0, "Parent context is now sole owner");
    assert_eq!(m.private_pages, 2);

    println!(
        "PASS: Blackwell sm_121 BF16 PagedAttention End-to-End Proof verified across 32 branches (64 kernel calls, 0 fallbacks, O(1) prefix memory, exact token parity)"
    );
}

//! PREFILL-E2E-0 cut C2: one KV for runtime and backend (bullets 4, 8, 10).
//!
//! Builds the spine and the native backend through the same factory the
//! daemon uses (`aien_runtime::shared_kv::build_shared_kv_runtime`), submits
//! one root prompt (<= 16 tokens, a single prefill chunk) through the real
//! swarm -> scheduler `prefill_detached` -> backend `execute_step` path up to
//! the completion fence, then reads the root's K/V back out of the SPINE's KV
//! pool and compares it with the dense K/V an unpaged backend (no KV manager)
//! computes for the same prompt with the same weights.
//!
//! Mutant (kept outside the repo, applied by the forge job): the factory gives
//! the backend its own separate pooled manager. Then the backend writes K/V
//! into a pool the spine never sees, `Arc::ptr_eq` is false and the spine's
//! pool holds only zeros for the root, so this test must fail.

use std::sync::Arc;

use aien_inference_abi::{
    ModelConfig, NativeTransformerBackend, ReferenceCpuBackend, TransformerWeights,
};
use aien_runtime::shared_kv::{build_shared_kv_runtime, SharedKvSizing};
use aien_runtime::swarm::SwarmConfig;
use aien_scheduler::SchedulerConfig;

const TOL: f32 = 1e-6;

fn small_config() -> ModelConfig {
    ModelConfig {
        model_id: "pe2e-c2-shared-kv-reference".to_string(),
        max_sequence_length: 256,
        // Small blocks so the 11-token prompt spans three blocks, the last one partial.
        block_size: 4,
        num_layers: 3,
        num_heads: 4,
        head_dim: 8,
        num_kv_heads: 2,
        hidden_dim: 32,
        intermediate_dim: 64,
        vocab_size: 97,
        rms_norm_eps: 1e-5,
        rope_theta: 10000.0,
    }
}

fn daemon_like_scheduler() -> SchedulerConfig {
    // Same chunking as the daemon (aien-cli run_daemon_server): chunk 128, so
    // the prompt below is one prefill chunk.
    SchedulerConfig {
        max_batch_size: 32,
        max_batch_tokens: 2048,
        max_prefill_tokens: 1024,
        prefill_chunk_size: 128,
        chunk_prefill: true,
        watermark_blocks: 2,
    }
}

#[tokio::test]
async fn shared_kv_root_prefill_lands_in_spine_pool_and_matches_unpaged_backend() {
    let config = small_config();
    let weights = TransformerWeights::reference_test_weights(&config);
    let prompt: Vec<u32> = vec![5, 17, 42, 3, 88, 61, 9, 23, 70, 11, 36];
    assert!(prompt.len() <= 16);

    let (mut spine, mut backend) = build_shared_kv_runtime(
        weights.clone(),
        Arc::new(ReferenceCpuBackend::new()),
        daemon_like_scheduler(),
        SharedKvSizing {
            arena_capacity: 64,
            total_blocks: 64,
        },
    )
    .expect("build shared KV runtime");

    // Check 1: one KV. The backend holds the very same Arc as the spine.
    let backend_kv = backend
        .kv_manager
        .clone()
        .expect("backend must be wired to a KV manager");
    assert!(
        Arc::ptr_eq(&backend_kv, &spine.kv_manager),
        "SHARED_KV_VIOLATION: backend KV manager is not the spine's KV manager"
    );
    assert!(
        spine.kv_manager.read().tensor_pool().is_some(),
        "SHARED_KV_VIOLATION: spine KV manager has no physical tensor pool"
    );

    // Submit one root prompt through the real swarm/scheduler path.
    let swarm_id = spine
        .launch_swarm(
            SwarmConfig {
                model_handle: 1,
                branch_count: 0,
                max_active_sequences: 1,
                max_tokens_per_branch: 1,
                root_world_id: 0,
                priority: 1,
            },
            &prompt,
        )
        .expect("launch swarm");
    let root = spine
        .swarm_manager
        .get_swarm(swarm_id)
        .expect("swarm record")
        .root_sequence_id
        .as_u64();

    let before = spine
        .kv_manager
        .read()
        .prefill_state(root)
        .expect("root has a KV table after launch");
    assert!(!before.is_ready(), "root must not be ready before prefill");

    let metrics = spine
        .step(&mut backend)
        .await
        .expect("spine step")
        .expect("step must report the root prefill");
    assert_eq!(metrics.prefill_tokens_processed, prompt.len());

    let after = spine
        .kv_manager
        .read()
        .prefill_state(root)
        .expect("root still has a KV table after prefill");
    assert!(after.is_ready(), "root must be ready after the completion fence");
    assert!(!spine.swarm_manager.is_root_prefill_pending(swarm_id));

    // Control: an unpaged backend (no KV manager) with the same weights.
    let mut control = NativeTransformerBackend::new_reference(weights);
    let control_id = 9_000_001u64;
    control
        .prefill_sequence(control_id, &prompt)
        .expect("control prefill");
    let control_seq = control
        .sequences
        .get(&control_id)
        .expect("control sequence state");

    let kv_dim = config.num_kv_heads * config.head_dim;
    let kv = spine.kv_manager.read();
    for layer in 0..config.num_layers {
        let mut k = Vec::new();
        let mut v = Vec::new();
        kv.gather_sequence_layer_kv(root, layer, &mut k, &mut v)
            .expect("gather root K/V from the spine's pool");

        let ck = &control_seq.layers[layer].flat_k;
        let cv = &control_seq.layers[layer].flat_v;
        assert_eq!(ck.len(), prompt.len() * kv_dim, "control K length, layer {layer}");
        assert_eq!(k.len(), ck.len(), "spine pool K length, layer {layer}");
        assert_eq!(v.len(), cv.len(), "spine pool V length, layer {layer}");

        assert!(
            k.iter().any(|x| *x != 0.0) && v.iter().any(|x| *x != 0.0),
            "SHARED_KV_VIOLATION: spine pool K/V for root is all zero at layer {layer} \
             (prefill K/V did not land in the runtime KV)"
        );
        for (i, (a, b)) in k.iter().zip(ck.iter()).enumerate() {
            assert!(
                (a - b).abs() <= TOL,
                "K mismatch layer {layer} idx {i}: spine pool {a} vs unpaged {b}"
            );
        }
        for (i, (a, b)) in v.iter().zip(cv.iter()).enumerate() {
            assert!(
                (a - b).abs() <= TOL,
                "V mismatch layer {layer} idx {i}: spine pool {a} vs unpaged {b}"
            );
        }
    }
}

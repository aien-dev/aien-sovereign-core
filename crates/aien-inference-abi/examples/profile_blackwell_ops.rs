use aien_inference_abi::backend::TensorBackend;
use aien_inference_abi::blackwell_backend::BlackwellGb10Backend;
use aien_inference_abi::weights::{LayerKvCache, SequenceState, TransformerWeights};
use aien_inference_abi::ModelConfig;
use std::time::Instant;

fn main() {
    println!("=== AIEN NATIVE TRANSFORMER PER-OP PROFILE ON DGX SPARK ===");
    let model_config = ModelConfig {
        model_id: "TinyLlama/TinyLlama-1.1B-Chat-v1.0".to_string(),
        max_sequence_length: 2048,
        block_size: 16,
        num_layers: 22,
        num_heads: 32,
        head_dim: 64,
        num_kv_heads: 4,
        hidden_dim: 2048,
        intermediate_dim: 5632,
        vocab_size: 32000,
        rms_norm_eps: 1e-5,
        rope_theta: 10000.0,
    };

    let safetensors_path = "/home/drakestapleton/.cache/huggingface/hub/models--TinyLlama--TinyLlama-1.1B-Chat-v1.0/snapshots/fe8a4ea1ffedaf415f4da2f062534de366a451e6/model.safetensors";
    let weights = TransformerWeights::load_from_safetensors(safetensors_path, &model_config)
        .expect("Failed to load safetensors");

    let backend = BlackwellGb10Backend::new();
    println!("Using Backend: {}", backend.name());

    let hidden_dim = model_config.hidden_dim;
    let intermediate_dim = model_config.intermediate_dim;
    let vocab_size = model_config.vocab_size;
    let num_heads = model_config.num_heads;
    let num_kv_heads = model_config.num_kv_heads;
    let head_dim = model_config.head_dim;
    let q_dim = num_heads * head_dim;
    let kv_dim = num_kv_heads * head_dim;
    let eps = model_config.rms_norm_eps;
    let theta = model_config.rope_theta;

    // Simulate seq_len = 128 (decode step 128)
    let seq_len = 128;
    let mut seq_state = SequenceState {
        tokens: vec![1u32; seq_len],
        layers: vec![LayerKvCache {
            cached_k: vec![vec![0.01f32; kv_dim]; seq_len],
            cached_v: vec![vec![0.01f32; kv_dim]; seq_len],
            flat_k: vec![0.01f32; seq_len * kv_dim],
            flat_v: vec![0.01f32; seq_len * kv_dim],
        }; 22],
    };

    let warmup_iters = 2;
    let bench_iters = 5;

    // Accumulators in microseconds
    let mut time_rmsnorm_input_us = 0u128;
    let mut time_qkv_gemm_us = 0u128;
    let mut time_rope_us = 0u128;
    let mut time_kv_flatten_us = 0u128;
    let mut time_gqa_attn_us = 0u128;
    let mut time_oproj_gemm_us = 0u128;
    let mut time_rmsnorm_post_us = 0u128;
    let mut time_mlp_gate_up_us = 0u128;
    let mut time_swiglu_us = 0u128;
    let mut time_mlp_down_us = 0u128;
    let mut time_rmsnorm_final_us = 0u128;
    let mut time_lm_head_us = 0u128;
    let mut time_alloc_overhead_us = 0u128;
    let mut total_token_us = 0u128;

    for iter in 0..(warmup_iters + bench_iters) {
        let is_bench = iter >= warmup_iters;
        let t_token_start = Instant::now();

        // 1. Embedding lookup
        let mut x = vec![0.01f32; hidden_dim];

        for (layer_idx, layer_w) in weights.layers.iter().enumerate() {
            let kv_cache = &mut seq_state.layers[layer_idx];

            // Allocation timing
            let t_alloc = Instant::now();
            let mut x_norm = vec![0.0f32; hidden_dim];
            let mut q = vec![0.0f32; q_dim];
            let mut k = vec![0.0f32; kv_dim];
            let mut v = vec![0.0f32; kv_dim];
            let mut attn_out = vec![0.0f32; q_dim];
            let mut attn_proj = vec![0.0f32; hidden_dim];
            let mut post_norm = vec![0.0f32; hidden_dim];
            let mut gate = vec![0.0f32; intermediate_dim];
            let mut up = vec![0.0f32; intermediate_dim];
            let mut activated = vec![0.0f32; intermediate_dim];
            let mut mlp_out = vec![0.0f32; hidden_dim];
            let alloc_elapsed = t_alloc.elapsed().as_micros();
            if is_bench {
                time_alloc_overhead_us += alloc_elapsed;
            }

            // Input RMSNorm
            let t0 = Instant::now();
            backend.rmsnorm(&mut x_norm, &x, &layer_w.input_layernorm, eps);
            if is_bench {
                time_rmsnorm_input_us += t0.elapsed().as_micros();
            }

            // QKV GEMVs
            let t0 = Instant::now();
            backend.matmul_vec(&mut q, &x_norm, &layer_w.q_proj, q_dim, hidden_dim);
            backend.matmul_vec(&mut k, &x_norm, &layer_w.k_proj, kv_dim, hidden_dim);
            backend.matmul_vec(&mut v, &x_norm, &layer_w.v_proj, kv_dim, hidden_dim);
            if is_bench {
                time_qkv_gemm_us += t0.elapsed().as_micros();
            }

            // RoPE
            let t0 = Instant::now();
            backend.apply_rope(&mut q, &mut k, seq_len, head_dim, num_heads, num_kv_heads, theta);
            if is_bench {
                time_rope_us += t0.elapsed().as_micros();
            }

            // KV flatten & append
            let t0 = Instant::now();
            let mut flat_k = Vec::with_capacity(seq_len * kv_dim);
            let mut flat_v = Vec::with_capacity(seq_len * kv_dim);
            for t in 0..seq_len {
                flat_k.extend_from_slice(&kv_cache.cached_k[t]);
                flat_v.extend_from_slice(&kv_cache.cached_v[t]);
            }
            if is_bench {
                time_kv_flatten_us += t0.elapsed().as_micros();
            }

            // GQA Attention
            let t0 = Instant::now();
            backend.gqa_attention(&mut attn_out, &q, &flat_k, &flat_v, seq_len, num_heads, num_kv_heads, head_dim);
            if is_bench {
                time_gqa_attn_us += t0.elapsed().as_micros();
            }

            // O-proj
            let t0 = Instant::now();
            backend.matmul_vec(&mut attn_proj, &attn_out, &layer_w.o_proj, hidden_dim, q_dim);
            for i in 0..hidden_dim {
                x[i] += attn_proj[i];
            }
            if is_bench {
                time_oproj_gemm_us += t0.elapsed().as_micros();
            }

            // Post RMSNorm
            let t0 = Instant::now();
            backend.rmsnorm(&mut post_norm, &x, &layer_w.post_attention_layernorm, eps);
            if is_bench {
                time_rmsnorm_post_us += t0.elapsed().as_micros();
            }

            // MLP Gate & Up
            let t0 = Instant::now();
            backend.matmul_vec(&mut gate, &post_norm, &layer_w.gate_proj, intermediate_dim, hidden_dim);
            backend.matmul_vec(&mut up, &post_norm, &layer_w.up_proj, intermediate_dim, hidden_dim);
            if is_bench {
                time_mlp_gate_up_us += t0.elapsed().as_micros();
            }

            // SwiGLU
            let t0 = Instant::now();
            backend.swiglu(&mut activated, &gate, &up);
            if is_bench {
                time_swiglu_us += t0.elapsed().as_micros();
            }

            // MLP Down
            let t0 = Instant::now();
            backend.matmul_vec(&mut mlp_out, &activated, &layer_w.down_proj, hidden_dim, intermediate_dim);
            for i in 0..hidden_dim {
                x[i] += mlp_out[i];
            }
            if is_bench {
                time_mlp_down_us += t0.elapsed().as_micros();
            }
        }

        // Final RMSNorm
        let mut x_final = vec![0.0f32; hidden_dim];
        let t0 = Instant::now();
        backend.rmsnorm(&mut x_final, &x, &weights.final_norm, eps);
        if is_bench {
            time_rmsnorm_final_us += t0.elapsed().as_micros();
        }

        // LM Head
        let mut logits = vec![0.0f32; vocab_size];
        let t0 = Instant::now();
        backend.compute_logits(&mut logits, &x_final, &weights.lm_head, vocab_size, hidden_dim);
        if is_bench {
            time_lm_head_us += t0.elapsed().as_micros();
        }

        let elapsed = t_token_start.elapsed().as_micros();
        if is_bench {
            total_token_us += elapsed;
        }
    }

    let n = bench_iters as f64;
    println!("\n--- PER-TOKEN DECODE WALL TIME BREAKDOWN (Avg over {} iters, seq_len={}) ---", bench_iters, seq_len);
    let ms = |us: u128| (us as f64) / (n * 1000.0);
    let total_ms = ms(total_token_us);

    let row = |name: &str, us: u128| {
        let val_ms = ms(us);
        let pct = (val_ms / total_ms) * 100.0;
        println!("{:<32} | {:>8.2} ms | {:>6.2} %", name, val_ms, pct);
    };

    println!("{:<32} | {:>11} | {:>8}", "Operation", "Wall Time", "Share");
    println!("{:-<32}-+-{:-<11}-+-{:-<8}", "", "", "");
    row("Input RMSNorm (22 layers)", time_rmsnorm_input_us);
    row("QKV GEMVs (22 layers)", time_qkv_gemm_us);
    row("RoPE (22 layers)", time_rope_us);
    row("KV Flattening (22 layers)", time_kv_flatten_us);
    row("GQA Attention (22 layers)", time_gqa_attn_us);
    row("O-proj GEMV (22 layers)", time_oproj_gemm_us);
    row("Post RMSNorm (22 layers)", time_rmsnorm_post_us);
    row("MLP Gate+Up GEMVs (22 layers)", time_mlp_gate_up_us);
    row("SwiGLU Activation (22 layers)", time_swiglu_us);
    row("MLP Down GEMV (22 layers)", time_mlp_down_us);
    row("Final RMSNorm (1 layer)", time_rmsnorm_final_us);
    row("LM Head GEMV (32k vocab)", time_lm_head_us);
    row("Heap Allocation / Buffers", time_alloc_overhead_us);
    println!("{:-<32}-+-{:-<11}-+-{:-<8}", "", "", "");
    println!("{:<32} | {:>8.2} ms | 100.00 %", "Total Single-Token Decode", total_ms);
    println!("Theoretical Decode Throughput: {:.2} tokens/sec", 1000.0 / total_ms);
    println!("Blackwell GPU Kernel Launches: {}", backend.kernel_exec_count());
    println!("CPU Fallback Executions: {}", backend.fallback_count());
}

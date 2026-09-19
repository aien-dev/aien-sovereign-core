//! Pure native Rust transformer inference backend executing real forward passes.
//! Dispatches tensor math through the decoupled TensorBackend trait for CPU and Mojo/GB10 execution.

use crate::backend::{ReferenceCpuBackend, TensorBackend};
use crate::mojo_backend::MojoGb10Backend;
use crate::tensor::{sample_argmax, sample_temperature};
use crate::weights::{LayerKvCache, SequenceState, TransformerWeights};
use crate::{
    AienInferenceBackend, DecodeOutput, FinishReason, ModelConfig, ScheduledBatch, StepMetrics,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

/// Fully native Rust transformer backend executing real tensor forward computation.
/// Decouples execution orchestration from compute hardware via the TensorBackend trait.
pub struct NativeTransformerBackend {
    pub weights: TransformerWeights,
    pub sequences: HashMap<u64, SequenceState>,
    pub tensor_backend: Arc<dyn TensorBackend>,
}

impl NativeTransformerBackend {
    /// Creates a new backend with default ReferenceCpuBackend oracle.
    pub fn new(weights: TransformerWeights) -> Self {
        Self {
            weights,
            sequences: HashMap::new(),
            tensor_backend: Arc::new(ReferenceCpuBackend::new()),
        }
    }

    /// Creates a backend with an explicit TensorBackend implementation.
    pub fn with_backend(weights: TransformerWeights, tensor_backend: Arc<dyn TensorBackend>) -> Self {
        Self {
            weights,
            sequences: HashMap::new(),
            tensor_backend,
        }
    }

    /// Explicit constructor for ReferenceCpuBackend golden oracle.
    pub fn new_reference(weights: TransformerWeights) -> Self {
        Self::new(weights)
    }

    /// Explicit constructor for Mojo GB10 hardware accelerated backend.
    pub fn new_mojo(weights: TransformerWeights) -> Self {
        Self::with_backend(weights, Arc::new(MojoGb10Backend::new()))
    }

    /// Convenience constructor with deterministic reference weights and ReferenceCpuBackend.
    pub fn with_reference_weights(config: &ModelConfig) -> Self {
        let weights = TransformerWeights::reference_test_weights(config);
        Self::new(weights)
    }

    /// Convenience constructor with deterministic reference weights and MojoGb10Backend.
    pub fn with_mojo_backend(config: &ModelConfig) -> Self {
        let weights = TransformerWeights::reference_test_weights(config);
        Self::new_mojo(weights)
    }

    /// Implementation helper executing forward pass on disjoint struct fields.
    pub fn forward_token_impl(
        weights: &TransformerWeights,
        backend: &dyn TensorBackend,
        token_id: u32,
        pos: usize,
        seq_state: &mut SequenceState,
    ) -> Vec<f32> {
        let hidden_dim = weights.config.hidden_dim();
        let num_heads = weights.config.num_heads;
        let num_kv_heads = weights.config.num_kv_heads;
        let head_dim = weights.config.head_dim;
        let q_dim = num_heads * head_dim;
        let kv_dim = num_kv_heads * head_dim;
        let intermediate_dim = weights.config.intermediate_dim();
        let eps = weights.config.rms_norm_eps;
        let theta = weights.config.rope_theta;

        // 1. Embedding lookup: x = embed_tokens[token_id]
        let token_idx = (token_id as usize) % weights.config.vocab_size();
        let embed_slice = &weights.embed_tokens[token_idx * hidden_dim..(token_idx + 1) * hidden_dim];
        let mut x = embed_slice.to_vec();

        // 2. Pre-allocated scratch buffers to eliminate per-layer heap allocations
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

        // 3. Transformer layers
        for (layer_idx, layer_w) in weights.layers.iter().enumerate() {
            let kv_cache = &mut seq_state.layers[layer_idx];

            // 2a. Input RMSNorm
            backend.rmsnorm(&mut x_norm, &x, &layer_w.input_layernorm, eps);

            // 2b. Q, K, V Projections
            backend.matmul_vec(&mut q, &x_norm, &layer_w.q_proj, q_dim, hidden_dim);
            backend.matmul_vec(&mut k, &x_norm, &layer_w.k_proj, kv_dim, hidden_dim);
            backend.matmul_vec(&mut v, &x_norm, &layer_w.v_proj, kv_dim, hidden_dim);

            // 2c. Rotary Positional Embeddings (RoPE)
            backend.apply_rope(
                &mut q,
                &mut k,
                pos,
                head_dim,
                num_heads,
                num_kv_heads,
                theta,
            );

            // 2d. Append to KV Cache (contiguous zero-copy buffer + backward compatibility)
            kv_cache.cached_k.push(k.clone());
            kv_cache.cached_v.push(v.clone());
            kv_cache.flat_k.extend_from_slice(&k);
            kv_cache.flat_v.extend_from_slice(&v);

            // 2e. Scaled Dot-Product GQA Attention
            let seq_len = kv_cache.cached_k.len();
            backend.gqa_attention(
                &mut attn_out,
                &q,
                &kv_cache.flat_k,
                &kv_cache.flat_v,
                seq_len,
                num_heads,
                num_kv_heads,
                head_dim,
            );

            // 2f. Output projection and residual connection
            backend.matmul_vec(
                &mut attn_proj,
                &attn_out,
                &layer_w.o_proj,
                hidden_dim,
                q_dim,
            );
            for i in 0..hidden_dim {
                x[i] += attn_proj[i];
            }

            // 2g. Post-Attention RMSNorm
            backend.rmsnorm(&mut post_norm, &x, &layer_w.post_attention_layernorm, eps);

            // 2h. SwiGLU MLP and residual connection
            backend.matmul_vec(&mut gate, &post_norm, &layer_w.gate_proj, intermediate_dim, hidden_dim);
            backend.matmul_vec(&mut up, &post_norm, &layer_w.up_proj, intermediate_dim, hidden_dim);
            backend.swiglu(&mut activated, &gate, &up);
            backend.matmul_vec(&mut mlp_out, &activated, &layer_w.down_proj, hidden_dim, intermediate_dim);

            for i in 0..hidden_dim {
                x[i] += mlp_out[i];
            }
        }

        // 3. Final RMSNorm
        let mut x_final = vec![0.0f32; hidden_dim];
        backend.rmsnorm(&mut x_final, &x, &weights.final_norm, eps);
        x_final
    }

    /// Implementation helper computing logits projection on disjoint struct fields.
    pub fn compute_logits_impl(
        weights: &TransformerWeights,
        backend: &dyn TensorBackend,
        hidden_state: &[f32],
    ) -> Vec<f32> {
        let hidden_dim = weights.config.hidden_dim();
        let vocab_size = weights.config.vocab_size();
        let mut logits = vec![0.0f32; vocab_size];
        backend.compute_logits(
            &mut logits,
            hidden_state,
            &weights.lm_head,
            vocab_size,
            hidden_dim,
        );
        logits
    }

    /// Prefill all prompt tokens layer-by-layer rather than token-by-token.
    /// Eliminates streaming the 22 layers of model weights 128 times across the memory bus,
    /// keeping weights resident in CPU cache across prompt token positions.
    pub fn prefill_prompt_layer_by_layer(
        weights: &TransformerWeights,
        backend: &dyn TensorBackend,
        prompt_tokens: &[u32],
        seq_state: &mut SequenceState,
    ) -> Vec<f32> {
        let n = prompt_tokens.len();
        if n == 0 {
            return Vec::new();
        }
        if n == 1 {
            seq_state.tokens.push(prompt_tokens[0]);
            return Self::forward_token_impl(weights, backend, prompt_tokens[0], 0, seq_state);
        }

        let hidden_dim = weights.config.hidden_dim();
        let num_heads = weights.config.num_heads;
        let num_kv_heads = weights.config.num_kv_heads;
        let head_dim = weights.config.head_dim;
        let q_dim = num_heads * head_dim;
        let kv_dim = num_kv_heads * head_dim;
        let intermediate_dim = weights.config.intermediate_dim();
        let eps = weights.config.rms_norm_eps;
        let theta = weights.config.rope_theta;

        seq_state.tokens.extend_from_slice(prompt_tokens);

        // 1. Look up embeddings for all prompt tokens
        let mut states: Vec<Vec<f32>> = prompt_tokens
            .iter()
            .map(|&tok| {
                let token_idx = (tok as usize) % weights.config.vocab_size();
                let slice = &weights.embed_tokens[token_idx * hidden_dim..(token_idx + 1) * hidden_dim];
                slice.to_vec()
            })
            .collect();

        // 2. Pre-allocated batched buffers for all prompt tokens across layers
        let mut x_norm_batch = vec![0.0f32; n * hidden_dim];
        let mut q_batch = vec![0.0f32; n * q_dim];
        let mut k_batch = vec![0.0f32; n * kv_dim];
        let mut v_batch = vec![0.0f32; n * kv_dim];
        let mut attn_out_batch = vec![0.0f32; n * q_dim];
        let mut attn_proj_batch = vec![0.0f32; n * hidden_dim];
        let mut post_norm_batch = vec![0.0f32; n * hidden_dim];
        let mut gate_batch = vec![0.0f32; n * intermediate_dim];
        let mut up_batch = vec![0.0f32; n * intermediate_dim];
        let mut act_batch = vec![0.0f32; n * intermediate_dim];
        let mut mlp_out_batch = vec![0.0f32; n * hidden_dim];

        // 3. Process each transformer layer using batched GEMM
        for (layer_idx, layer_w) in weights.layers.iter().enumerate() {
            let kv_cache = &mut seq_state.layers[layer_idx];

            // 3a. Batched Input RMSNorm
            for t in 0..n {
                let out_slice = &mut x_norm_batch[t * hidden_dim..(t + 1) * hidden_dim];
                backend.rmsnorm(out_slice, &states[t], &layer_w.input_layernorm, eps);
            }

            // 3b. Batched Q, K, V GEMMs across all prompt tokens
            backend.matmul_batch(&mut q_batch, &x_norm_batch, &layer_w.q_proj, n, hidden_dim, q_dim);
            backend.matmul_batch(&mut k_batch, &x_norm_batch, &layer_w.k_proj, n, hidden_dim, kv_dim);
            backend.matmul_batch(&mut v_batch, &x_norm_batch, &layer_w.v_proj, n, hidden_dim, kv_dim);

            // 3c. RoPE for each prompt position and append to KV cache
            for t in 0..n {
                let q_t = &mut q_batch[t * q_dim..(t + 1) * q_dim];
                let k_t = &mut k_batch[t * kv_dim..(t + 1) * kv_dim];
                let v_t = &v_batch[t * kv_dim..(t + 1) * kv_dim];

                backend.apply_rope(q_t, k_t, t, head_dim, num_heads, num_kv_heads, theta);

                kv_cache.cached_k.push(k_t.to_vec());
                kv_cache.cached_v.push(v_t.to_vec());
                kv_cache.flat_k.extend_from_slice(k_t);
                kv_cache.flat_v.extend_from_slice(v_t);
            }

            // 3d. Causal GQA Attention across all prompt tokens
            for t in 0..n {
                let q_t = &q_batch[t * q_dim..(t + 1) * q_dim];
                let attn_t = &mut attn_out_batch[t * q_dim..(t + 1) * q_dim];
                let seq_len = t + 1;

                backend.gqa_attention(
                    attn_t,
                    q_t,
                    &kv_cache.flat_k,
                    &kv_cache.flat_v,
                    seq_len,
                    num_heads,
                    num_kv_heads,
                    head_dim,
                );
            }

            // 3e. Batched Output Projection GEMM
            backend.matmul_batch(&mut attn_proj_batch, &attn_out_batch, &layer_w.o_proj, n, q_dim, hidden_dim);
            for t in 0..n {
                let proj_t = &attn_proj_batch[t * hidden_dim..(t + 1) * hidden_dim];
                for i in 0..hidden_dim {
                    states[t][i] += proj_t[i];
                }
            }

            // 3f. Batched Post-Attention RMSNorm
            for t in 0..n {
                let out_slice = &mut post_norm_batch[t * hidden_dim..(t + 1) * hidden_dim];
                backend.rmsnorm(out_slice, &states[t], &layer_w.post_attention_layernorm, eps);
            }

            // 3g. Batched MLP Gate and Up GEMMs
            backend.matmul_batch(&mut gate_batch, &post_norm_batch, &layer_w.gate_proj, n, hidden_dim, intermediate_dim);
            backend.matmul_batch(&mut up_batch, &post_norm_batch, &layer_w.up_proj, n, hidden_dim, intermediate_dim);

            // 3h. SwiGLU Activations
            for t in 0..n {
                let gate_t = &gate_batch[t * intermediate_dim..(t + 1) * intermediate_dim];
                let up_t = &up_batch[t * intermediate_dim..(t + 1) * intermediate_dim];
                let act_t = &mut act_batch[t * intermediate_dim..(t + 1) * intermediate_dim];
                backend.swiglu(act_t, gate_t, up_t);
            }

            // 3i. Batched MLP Down Projection GEMM
            backend.matmul_batch(&mut mlp_out_batch, &act_batch, &layer_w.down_proj, n, intermediate_dim, hidden_dim);
            for t in 0..n {
                let mlp_t = &mlp_out_batch[t * hidden_dim..(t + 1) * hidden_dim];
                for i in 0..hidden_dim {
                    states[t][i] += mlp_t[i];
                }
            }
        }

        // 4. Final RMSNorm on the last prompt token
        let last_x = &states[n - 1];
        let mut x_final = vec![0.0f32; hidden_dim];
        backend.rmsnorm(&mut x_final, last_x, &weights.final_norm, eps);
        x_final
    }

    /// Single-token forward pass executing through the configured TensorBackend trait.
    pub fn forward_token(
        &self,
        token_id: u32,
        pos: usize,
        seq_state: &mut SequenceState,
    ) -> Vec<f32> {
        Self::forward_token_impl(&self.weights, &*self.tensor_backend, token_id, pos, seq_state)
    }

    /// Computes logits projection executing through the configured TensorBackend trait.
    pub fn compute_logits(&self, hidden_state: &[f32]) -> Vec<f32> {
        Self::compute_logits_impl(&self.weights, &*self.tensor_backend, hidden_state)
    }
}

#[async_trait]
impl AienInferenceBackend for NativeTransformerBackend {
    async fn load_model(&mut self, config: &ModelConfig) -> Result<(), String> {
        self.weights = TransformerWeights::reference_test_weights(config);
        self.sequences.clear();
        Ok(())
    }

    async fn execute_step(
        &mut self,
        batch: &ScheduledBatch,
    ) -> Result<(Vec<DecodeOutput>, StepMetrics), String> {
        let t0 = std::time::Instant::now();
        let mut outputs = Vec::new();
        let mut prefill_tokens = 0;
        let num_layers = self.weights.config.num_layers;

        // 1. Prefill Requests
        for req in &batch.prefill_requests {
            prefill_tokens += req.prompt_tokens.len();
            let seq = self
                .sequences
                .entry(req.request_id)
                .or_insert_with(|| SequenceState {
                    tokens: Vec::new(),
                    layers: vec![LayerKvCache::default(); num_layers],
                });

            let last_hidden = Self::prefill_prompt_layer_by_layer(
                &self.weights,
                &*self.tensor_backend,
                &req.prompt_tokens,
                seq,
            );

            // Compute real logits on the prompt's last token via TensorBackend
            let logits = Self::compute_logits_impl(&self.weights, &*self.tensor_backend, &last_hidden);
            let (sampled_tok, logprob) = if req.sampling_params.temperature <= 0.001 {
                sample_argmax(&logits)
            } else {
                sample_temperature(&logits, req.sampling_params.temperature, req.request_id)
            };

            seq.tokens.push(sampled_tok);

            outputs.push(DecodeOutput::Token {
                request_id: req.request_id,
                token_id: sampled_tok,
                logprob: Some(logprob),
            });
        }

        // 2. Decode Requests
        for &req_id in &batch.decode_requests {
            if let Some(seq) = self.sequences.get_mut(&req_id) {
                // Prefill already pushed the first generated token to seq.tokens,
                // so the sequence length is currently tokens.len() and the token being
                // evaluated is at index tokens.len() - 1.
                let pos = seq.tokens.len().saturating_sub(1);
                let last_token = *seq.tokens.last().unwrap_or(&1);

                let hidden = Self::forward_token_impl(
                    &self.weights,
                    &*self.tensor_backend,
                    last_token,
                    pos,
                    seq,
                );
                let logits = Self::compute_logits_impl(&self.weights, &*self.tensor_backend, &hidden);

                let (sampled_tok, logprob) = sample_argmax(&logits);
                seq.tokens.push(sampled_tok);

                let stop_tokens = [
                    crate::tokenizer::TinyLlamaTokenizer::UNK_TOKEN_ID,
                    crate::tokenizer::TinyLlamaTokenizer::BOS_TOKEN_ID,
                    crate::tokenizer::TinyLlamaTokenizer::EOS_TOKEN_ID,
                ];
                if stop_tokens.contains(&sampled_tok) {
                    outputs.push(DecodeOutput::Finished {
                        request_id: req_id,
                        reason: FinishReason::StopToken,
                        total_tokens: seq.tokens.len(),
                    });
                } else {
                    outputs.push(DecodeOutput::Token {
                        request_id: req_id,
                        token_id: sampled_tok,
                        logprob: Some(logprob),
                    });
                }
            }
        }

        let elapsed_us = t0.elapsed().as_micros() as u64;
        let metrics = StepMetrics {
            prefill_tokens_processed: prefill_tokens,
            decode_tokens_emitted: batch.decode_requests.len() + batch.prefill_requests.len(),
            step_latency_us: elapsed_us,
            active_kv_blocks: batch.block_tables.values().map(|v| v.len()).sum(),
        };

        Ok((outputs, metrics))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_constructors() {
        let config = ModelConfig {
            num_layers: 2,
            num_heads: 4,
            num_kv_heads: 2,
            head_dim: 16,
            hidden_dim: 64,
            intermediate_dim: 128,
            vocab_size: 256,
            ..Default::default()
        };

        let cpu_backend = NativeTransformerBackend::with_reference_weights(&config);
        assert_eq!(cpu_backend.tensor_backend.name(), "ReferenceCpuBackend");

        let mojo_backend = NativeTransformerBackend::with_mojo_backend(&config);
        assert!(mojo_backend.tensor_backend.name().starts_with("MojoGb10Backend"));
    }

    #[test]
    fn test_forward_token_and_logits_execution() {
        let config = ModelConfig {
            num_layers: 2,
            num_heads: 4,
            num_kv_heads: 2,
            head_dim: 16,
            hidden_dim: 64,
            intermediate_dim: 128,
            vocab_size: 256,
            ..Default::default()
        };

        let backend = NativeTransformerBackend::with_reference_weights(&config);
        let mut seq_state = SequenceState {
            tokens: vec![1],
            layers: vec![LayerKvCache::default(); config.num_layers],
        };

        let hidden = backend.forward_token(1, 0, &mut seq_state);
        assert_eq!(hidden.len(), config.hidden_dim());

        let logits = backend.compute_logits(&hidden);
        assert_eq!(logits.len(), config.vocab_size());
    }
}

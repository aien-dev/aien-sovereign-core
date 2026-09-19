//! Native weight data structures and zero-dependency loaders for transformer models.

use crate::tensor::{apply_rope, matmul_vec, rmsnorm, scaled_dot_product_attention_single, swiglu};
use crate::ModelConfig;
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct LayerKvCache {
    pub cached_k: Vec<Vec<f32>>,
    pub cached_v: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, Default)]
pub struct SequenceState {
    pub tokens: Vec<u32>,
    pub layers: Vec<LayerKvCache>,
}

#[derive(Debug, Clone)]
pub struct TransformerLayerWeights {
    pub input_layernorm: Vec<f32>,
    pub q_proj: Vec<f32>,
    pub k_proj: Vec<f32>,
    pub v_proj: Vec<f32>,
    pub o_proj: Vec<f32>,
    pub post_attention_layernorm: Vec<f32>,
    pub gate_proj: Vec<f32>,
    pub up_proj: Vec<f32>,
    pub down_proj: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct TransformerWeights {
    pub config: ModelConfig,
    pub embed_tokens: Vec<f32>,
    pub layers: Vec<TransformerLayerWeights>,
    pub final_norm: Vec<f32>,
    pub lm_head: Vec<f32>,
}

impl TransformerWeights {
    /// Creates deterministically initialized weights for tests and headless verification.
    pub fn reference_test_weights(config: &ModelConfig) -> Self {
        let hidden_dim = config.hidden_dim();
        let q_dim = config.num_heads * config.head_dim;
        let kv_dim = config.num_kv_heads * config.head_dim;
        let intermediate_dim = config.intermediate_dim();
        let vocab_size = config.vocab_size();

        let mut lcg_state: u64 = 42;
        let mut next_float = || -> f32 {
            lcg_state = lcg_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let val = ((lcg_state >> 33) as f32) / ((1u64 << 31) as f32);
            (val - 0.5) * 0.05
        };

        let mut embed_tokens = vec![0.0f32; vocab_size * hidden_dim];
        for v in embed_tokens.iter_mut() {
            *v = next_float();
        }

        let mut layers = Vec::with_capacity(config.num_layers);
        for _ in 0..config.num_layers {
            let mut input_layernorm = vec![1.0f32; hidden_dim];
            for v in input_layernorm.iter_mut() {
                *v = 1.0 + next_float() * 0.1;
            }

            let mut q_proj = vec![0.0f32; hidden_dim * q_dim];
            for v in q_proj.iter_mut() {
                *v = next_float();
            }

            let mut k_proj = vec![0.0f32; hidden_dim * kv_dim];
            for v in k_proj.iter_mut() {
                *v = next_float();
            }

            let mut v_proj = vec![0.0f32; hidden_dim * kv_dim];
            for v in v_proj.iter_mut() {
                *v = next_float();
            }

            let mut o_proj = vec![0.0f32; q_dim * hidden_dim];
            for v in o_proj.iter_mut() {
                *v = next_float();
            }

            let mut post_attention_layernorm = vec![1.0f32; hidden_dim];
            for v in post_attention_layernorm.iter_mut() {
                *v = 1.0 + next_float() * 0.1;
            }

            let mut gate_proj = vec![0.0f32; hidden_dim * intermediate_dim];
            for v in gate_proj.iter_mut() {
                *v = next_float();
            }

            let mut up_proj = vec![0.0f32; hidden_dim * intermediate_dim];
            for v in up_proj.iter_mut() {
                *v = next_float();
            }

            let mut down_proj = vec![0.0f32; intermediate_dim * hidden_dim];
            for v in down_proj.iter_mut() {
                *v = next_float();
            }

            layers.push(TransformerLayerWeights {
                input_layernorm,
                q_proj,
                k_proj,
                v_proj,
                o_proj,
                post_attention_layernorm,
                gate_proj,
                up_proj,
                down_proj,
            });
        }

        let mut final_norm = vec![1.0f32; hidden_dim];
        for v in final_norm.iter_mut() {
            *v = 1.0 + next_float() * 0.1;
        }

        let mut lm_head = vec![0.0f32; hidden_dim * vocab_size];
        for v in lm_head.iter_mut() {
            *v = next_float();
        }

        Self {
            config: config.clone(),
            embed_tokens,
            layers,
            final_norm,
            lm_head,
        }
    }

    /// Single-token forward pass through all transformer layers.
    /// Returns the output vector after final RMSNorm of shape [hidden_dim].
    pub fn forward_token(
        &self,
        token_id: u32,
        pos: usize,
        seq_state: &mut SequenceState,
    ) -> Vec<f32> {
        let hidden_dim = self.config.hidden_dim();
        let num_heads = self.config.num_heads;
        let num_kv_heads = self.config.num_kv_heads;
        let head_dim = self.config.head_dim;
        let q_dim = num_heads * head_dim;
        let kv_dim = num_kv_heads * head_dim;
        let intermediate_dim = self.config.intermediate_dim();
        let eps = self.config.rms_norm_eps;
        let theta = self.config.rope_theta;

        // 1. Embedding lookup: x = embed_tokens[token_id]
        let token_idx = (token_id as usize) % self.config.vocab_size();
        let embed_slice = &self.embed_tokens[token_idx * hidden_dim..(token_idx + 1) * hidden_dim];
        let mut x = embed_slice.to_vec();

        // 2. Transformer layers
        for (layer_idx, layer_w) in self.layers.iter().enumerate() {
            let kv_cache = &mut seq_state.layers[layer_idx];

            // 2a. Input RMSNorm
            let mut x_norm = vec![0.0f32; hidden_dim];
            rmsnorm(&x, &layer_w.input_layernorm, eps, &mut x_norm);

            // 2b. Q, K, V Projections
            let mut q = vec![0.0f32; q_dim];
            let mut k = vec![0.0f32; kv_dim];
            let mut v = vec![0.0f32; kv_dim];
            matmul_vec(&x_norm, &layer_w.q_proj, &mut q, hidden_dim, q_dim);
            matmul_vec(&x_norm, &layer_w.k_proj, &mut k, hidden_dim, kv_dim);
            matmul_vec(&x_norm, &layer_w.v_proj, &mut v, hidden_dim, kv_dim);

            // 2c. Rotary Positional Embeddings (RoPE)
            apply_rope(
                &mut q,
                &mut k,
                pos,
                num_heads,
                num_kv_heads,
                head_dim,
                theta,
            );

            // 2d. Append to KV Cache
            kv_cache.cached_k.push(k);
            kv_cache.cached_v.push(v);

            // 2e. Scaled Dot-Product Attention
            let mut attn_out = vec![0.0f32; q_dim];
            scaled_dot_product_attention_single(
                &q,
                &kv_cache.cached_k,
                &kv_cache.cached_v,
                num_heads,
                num_kv_heads,
                head_dim,
                &mut attn_out,
            );

            // 2f. Output projection and residual connection
            let mut attn_proj = vec![0.0f32; hidden_dim];
            matmul_vec(
                &attn_out,
                &layer_w.o_proj,
                &mut attn_proj,
                q_dim,
                hidden_dim,
            );
            for i in 0..hidden_dim {
                x[i] += attn_proj[i];
            }

            // 2g. Post-Attention RMSNorm
            let mut post_norm = vec![0.0f32; hidden_dim];
            rmsnorm(&x, &layer_w.post_attention_layernorm, eps, &mut post_norm);

            // 2h. SwiGLU MLP and residual connection
            let mut mlp_out = vec![0.0f32; hidden_dim];
            swiglu(
                &post_norm,
                &layer_w.gate_proj,
                &layer_w.up_proj,
                &layer_w.down_proj,
                hidden_dim,
                intermediate_dim,
                &mut mlp_out,
            );
            for i in 0..hidden_dim {
                x[i] += mlp_out[i];
            }
        }

        // 3. Final RMSNorm
        let mut x_final = vec![0.0f32; hidden_dim];
        rmsnorm(&x, &self.final_norm, eps, &mut x_final);
        x_final
    }

    /// Projects hidden states through LM Head and computes real vocabulary logits.
    pub fn compute_logits(&self, hidden_state: &[f32]) -> Vec<f32> {
        let hidden_dim = self.config.hidden_dim();
        let vocab_size = self.config.vocab_size();
        let mut logits = vec![0.0f32; vocab_size];
        matmul_vec(
            hidden_state,
            &self.lm_head,
            &mut logits,
            hidden_dim,
            vocab_size,
        );
        logits
    }

    /// Loads model weights directly from a standard safetensors binary buffer.
    pub fn from_safetensors_bytes(bytes: &[u8], config: &ModelConfig) -> Result<Self, String> {
        if bytes.len() < 8 {
            return Err(
                "Invalid safetensors binary: buffer smaller than 8 bytes header".to_string(),
            );
        }

        let header_len = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) as usize;
        if bytes.len() < 8 + header_len {
            return Err("Invalid safetensors binary: header length exceeds buffer".to_string());
        }

        let header_str = std::str::from_utf8(&bytes[8..8 + header_len])
            .map_err(|e| format!("Safetensors header is not valid UTF-8: {}", e))?;

        let header: Value = serde_json::from_str(header_str)
            .map_err(|e| format!("Failed to parse safetensors JSON header: {}", e))?;

        let data_offset = 8 + header_len;
        let data_bytes = &bytes[data_offset..];

        let extract_tensor = |name: &str| -> Option<Vec<f32>> {
            let info = header.get(name)?;
            let offsets = info.get("data_offsets")?.as_array()?;
            let start = offsets.first()?.as_u64()? as usize;
            let end = offsets.get(1)?.as_u64()? as usize;
            let dtype = info.get("dtype")?.as_str()?;

            if end > data_bytes.len() || start > end {
                return None;
            }

            let raw_slice = &data_bytes[start..end];
            match dtype {
                "F32" => {
                    let count = (end - start) / 4;
                    let mut floats = Vec::with_capacity(count);
                    for chunk in raw_slice.chunks_exact(4) {
                        floats.push(f32::from_le_bytes(chunk.try_into().unwrap()));
                    }
                    Some(floats)
                }
                "BF16" => {
                    let count = (end - start) / 2;
                    let mut floats = Vec::with_capacity(count);
                    for chunk in raw_slice.chunks_exact(2) {
                        let bits = u16::from_le_bytes(chunk.try_into().unwrap());
                        floats.push(f32::from_bits((bits as u32) << 16));
                    }
                    Some(floats)
                }
                "F16" => {
                    let count = (end - start) / 2;
                    let mut floats = Vec::with_capacity(count);
                    for chunk in raw_slice.chunks_exact(2) {
                        let bits = u16::from_le_bytes(chunk.try_into().unwrap());
                        floats.push(half_to_float(bits));
                    }
                    Some(floats)
                }
                _ => None,
            }
        };

        let mut weights = Self::reference_test_weights(config);

        if let Some(t) = extract_tensor("model.embed_tokens.weight") {
            weights.embed_tokens = t;
        }

        for (idx, layer) in weights.layers.iter_mut().enumerate() {
            let prefix = format!("model.layers.{}", idx);
            if let Some(t) = extract_tensor(&format!("{}.input_layernorm.weight", prefix)) {
                layer.input_layernorm = t;
            }
            if let Some(t) = extract_tensor(&format!("{}.self_attn.q_proj.weight", prefix)) {
                layer.q_proj = t;
            }
            if let Some(t) = extract_tensor(&format!("{}.self_attn.k_proj.weight", prefix)) {
                layer.k_proj = t;
            }
            if let Some(t) = extract_tensor(&format!("{}.self_attn.v_proj.weight", prefix)) {
                layer.v_proj = t;
            }
            if let Some(t) = extract_tensor(&format!("{}.self_attn.o_proj.weight", prefix)) {
                layer.o_proj = t;
            }
            if let Some(t) = extract_tensor(&format!("{}.post_attention_layernorm.weight", prefix))
            {
                layer.post_attention_layernorm = t;
            }
            if let Some(t) = extract_tensor(&format!("{}.mlp.gate_proj.weight", prefix)) {
                layer.gate_proj = t;
            }
            if let Some(t) = extract_tensor(&format!("{}.mlp.up_proj.weight", prefix)) {
                layer.up_proj = t;
            }
            if let Some(t) = extract_tensor(&format!("{}.mlp.down_proj.weight", prefix)) {
                layer.down_proj = t;
            }
        }

        if let Some(t) = extract_tensor("model.norm.weight") {
            weights.final_norm = t;
        }

        if let Some(t) = extract_tensor("lm_head.weight") {
            weights.lm_head = t;
        }

        Ok(weights)
    }
}

/// Converts IEEE 754 half-precision float (f16) bits to f32.
fn half_to_float(bits: u16) -> f32 {
    let sign = ((bits >> 15) & 1) as u32;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let frac = (bits & 0x3ff) as u32;

    if exp == 0 {
        if frac == 0 {
            f32::from_bits(sign << 31)
        } else {
            let mut f = frac;
            let mut shift = 0;
            while (f & 0x400) == 0 {
                f <<= 1;
                shift += 1;
            }
            let exp32 = (127 - 15 - shift + 1) as u32;
            let frac32 = (f & 0x3ff) << 13;
            f32::from_bits((sign << 31) | (exp32 << 23) | frac32)
        }
    } else if exp == 31 {
        f32::from_bits((sign << 31) | (0xff << 23) | (frac << 13))
    } else {
        let exp32 = (exp + 127 - 15) as u32;
        let frac32 = frac << 13;
        f32::from_bits((sign << 31) | (exp32 << 23) | frac32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reference_weights_initialization() {
        let config = ModelConfig {
            model_id: "test-llama".to_string(),
            max_sequence_length: 2048,
            block_size: 16,
            num_layers: 2,
            num_heads: 4,
            head_dim: 16,
            num_kv_heads: 4,
            hidden_dim: 64,
            intermediate_dim: 128,
            vocab_size: 256,
            rms_norm_eps: 1e-5,
            rope_theta: 10000.0,
        };

        let weights = TransformerWeights::reference_test_weights(&config);
        assert_eq!(weights.embed_tokens.len(), 256 * 64);
        assert_eq!(weights.layers.len(), 2);
        assert_eq!(weights.layers[0].q_proj.len(), 64 * 64);
        assert_eq!(weights.layers[0].k_proj.len(), 64 * 64);
        assert_eq!(weights.layers[0].v_proj.len(), 64 * 64);
        assert_eq!(weights.layers[0].gate_proj.len(), 64 * 128);
        assert_eq!(weights.layers[0].down_proj.len(), 128 * 64);
        assert_eq!(weights.final_norm.len(), 64);
        assert_eq!(weights.lm_head.len(), 64 * 256);
    }
}

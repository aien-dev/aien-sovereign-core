//! Decoupled TensorBackend trait and ReferenceCpuBackend correctness oracle.
//! Provides hardware-neutral abstraction for transformer mathematical operations.

use crate::tensor::{apply_rope as tensor_rope, matmul_vec as tensor_matmul, rmsnorm as tensor_rmsnorm};

/// Decoupled mathematical abstraction for transformer tensor operations.
pub trait TensorBackend: Send + Sync {
    /// Human-readable name of the backend implementation.
    fn name(&self) -> &'static str;

    /// In-place Root Mean Square Normalization:
    /// out[i] = x[i] * weight[i] / sqrt(mean(x^2) + eps)
    fn rmsnorm(&self, out: &mut [f32], x: &[f32], weight: &[f32], eps: f32);

    /// In-place Rotary Positional Embeddings (RoPE) following canonical rotate_half convention:
    /// rot(v) = [-v[half_dim..], v[..half_dim]]
    /// out = v * cos(pos * theta^(-2i/d)) + rot(v) * sin(pos * theta^(-2i/d))
    fn apply_rope(
        &self,
        q: &mut [f32],
        k: &mut [f32],
        pos: usize,
        head_dim: usize,
        num_q_heads: usize,
        num_kv_heads: usize,
        theta: f32,
    );

    /// Vector-matrix multiplication for row-major weights: out = x * W^T
    /// where x is [in_dim], weight is [out_dim, in_dim], and out is [out_dim].
    fn matmul_vec(&self, out: &mut [f32], x: &[f32], weight: &[f32], out_dim: usize, in_dim: usize);

    /// SwiGLU activation: out = (gate * silu) * up = silu(gate) * up
    fn swiglu(&self, out: &mut [f32], gate: &[f32], up: &[f32]);

    /// Grouped-Query Attention (GQA) with multi-head queries and shared key-value heads.
    /// Operates over flattened key and value cache slices with causal history.
    fn gqa_attention(
        &self,
        out: &mut [f32],
        q: &[f32],
        k_cache: &[f32],
        v_cache: &[f32],
        seq_len: usize,
        num_q_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
    );

    /// Projects hidden states to vocabulary logits: logits = hidden * W^T
    fn compute_logits(
        &self,
        logits: &mut [f32],
        hidden: &[f32],
        embed_weight: &[f32],
        vocab_size: usize,
        hidden_dim: usize,
    );
}

/// Pure Rust FP32 reference implementation satisfying TensorBackend.
/// Serves as the golden correctness oracle on both development workstations and CI runners.
#[derive(Debug, Clone, Default)]
pub struct ReferenceCpuBackend;

impl ReferenceCpuBackend {
    pub fn new() -> Self {
        Self
    }
}

impl TensorBackend for ReferenceCpuBackend {
    fn name(&self) -> &'static str {
        "ReferenceCpuBackend"
    }

    fn rmsnorm(&self, out: &mut [f32], x: &[f32], weight: &[f32], eps: f32) {
        tensor_rmsnorm(x, weight, eps, out);
    }

    fn apply_rope(
        &self,
        q: &mut [f32],
        k: &mut [f32],
        pos: usize,
        head_dim: usize,
        num_q_heads: usize,
        num_kv_heads: usize,
        theta: f32,
    ) {
        tensor_rope(q, k, pos, num_q_heads, num_kv_heads, head_dim, theta);
    }

    fn matmul_vec(&self, out: &mut [f32], x: &[f32], weight: &[f32], out_dim: usize, in_dim: usize) {
        tensor_matmul(x, weight, out, in_dim, out_dim);
    }

    fn swiglu(&self, out: &mut [f32], gate: &[f32], up: &[f32]) {
        let n = gate.len().min(up.len()).min(out.len());
        for i in 0..n {
            let g = gate[i] as f64;
            let silu = g / (1.0 + (-g).exp());
            out[i] = (silu * (up[i] as f64)) as f32;
        }
    }

    fn gqa_attention(
        &self,
        out: &mut [f32],
        q: &[f32],
        k_cache: &[f32],
        v_cache: &[f32],
        seq_len: usize,
        num_q_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
    ) {
        if seq_len == 0 {
            out.fill(0.0);
            return;
        }

        let gqa_ratio = num_q_heads / num_kv_heads;
        let inv_sqrt_d = 1.0 / (head_dim as f64).sqrt();
        let mut scores = vec![0.0f64; seq_len];

        for h in 0..num_q_heads {
            let kv_head = h / gqa_ratio;
            let q_head = &q[h * head_dim..(h + 1) * head_dim];

            for t in 0..seq_len {
                let k_offset = (t * num_kv_heads + kv_head) * head_dim;
                let k_t = &k_cache[k_offset..k_offset + head_dim];
                let mut dot = 0.0f64;
                for d in 0..head_dim {
                    dot += (q_head[d] as f64) * (k_t[d] as f64);
                }
                scores[t] = dot * inv_sqrt_d;
            }

            let max_score = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let mut sum_exp = 0.0f64;
            for s in scores.iter_mut() {
                *s = (*s - max_score).exp();
                sum_exp += *s;
            }
            let inv_sum = 1.0 / sum_exp.max(1e-12);
            for s in scores.iter_mut() {
                *s *= inv_sum;
            }

            let out_head = &mut out[h * head_dim..(h + 1) * head_dim];
            for d in 0..head_dim {
                let mut sum = 0.0f64;
                for t in 0..seq_len {
                    let v_offset = (t * num_kv_heads + kv_head) * head_dim;
                    sum += scores[t] * (v_cache[v_offset + d] as f64);
                }
                out_head[d] = sum as f32;
            }
        }
    }

    fn compute_logits(
        &self,
        logits: &mut [f32],
        hidden: &[f32],
        embed_weight: &[f32],
        vocab_size: usize,
        hidden_dim: usize,
    ) {
        self.matmul_vec(logits, hidden, embed_weight, vocab_size, hidden_dim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reference_backend_name() {
        let backend = ReferenceCpuBackend::new();
        assert_eq!(backend.name(), "ReferenceCpuBackend");
    }

    #[test]
    fn test_reference_rmsnorm_matches_tensor() {
        let backend = ReferenceCpuBackend::new();
        let x = vec![1.0f32, 2.0, 3.0, 4.0];
        let w = vec![1.0f32, 1.0, 1.0, 1.0];
        let mut out = vec![0.0f32; 4];
        backend.rmsnorm(&mut out, &x, &w, 1e-5);

        let mut expected = vec![0.0f32; 4];
        tensor_rmsnorm(&x, &w, 1e-5, &mut expected);

        assert_eq!(out, expected);
    }

    #[test]
    fn test_reference_rope_matches_tensor() {
        let backend = ReferenceCpuBackend::new();
        let mut q1 = vec![1.0f32; 128];
        let mut k1 = vec![1.0f32; 64];
        let mut q2 = q1.clone();
        let mut k2 = k1.clone();

        backend.apply_rope(&mut q1, &mut k1, 5, 64, 2, 1, 10000.0);
        tensor_rope(&mut q2, &mut k2, 5, 2, 1, 64, 10000.0);

        assert_eq!(q1, q2);
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_reference_matmul_matches_tensor() {
        let backend = ReferenceCpuBackend::new();
        let x = vec![1.0f32, 2.0, 3.0];
        let w = vec![
            1.0f32, 0.0, 0.0,
            0.0, 1.0, 0.0,
        ];
        let mut out = vec![0.0f32; 2];
        backend.matmul_vec(&mut out, &x, &w, 2, 3);
        assert_eq!(out, vec![1.0, 2.0]);
    }

    #[test]
    fn test_reference_swiglu() {
        let backend = ReferenceCpuBackend::new();
        let gate = vec![0.0f32, 2.0];
        let up = vec![1.0f32, 3.0];
        let mut out = vec![0.0f32; 2];
        backend.swiglu(&mut out, &gate, &up);

        // silu(0) = 0 * 0.5 = 0, so out[0] = 0
        assert_eq!(out[0], 0.0);
        // silu(2) = 2 / (1 + exp(-2)) ~= 1.761594, out[1] = 1.761594 * 3 ~= 5.28478
        assert!((out[1] - 5.28478).abs() < 1e-4);
    }

    #[test]
    fn test_reference_gqa_attention() {
        let backend = ReferenceCpuBackend::new();
        let head_dim = 4;
        let num_q_heads = 2;
        let num_kv_heads = 1;
        let seq_len = 2;

        let q = vec![1.0f32; num_q_heads * head_dim];
        let k_cache = vec![1.0f32; seq_len * num_kv_heads * head_dim];
        let v_cache = vec![2.0f32; seq_len * num_kv_heads * head_dim];
        let mut out = vec![0.0f32; num_q_heads * head_dim];

        backend.gqa_attention(&mut out, &q, &k_cache, &v_cache, seq_len, num_q_heads, num_kv_heads, head_dim);

        // All values are 2.0, so weighted sum should be 2.0 for all outputs
        for v in &out {
            assert!((v - 2.0).abs() < 1e-5);
        }
    }

    #[test]
    fn test_reference_compute_logits() {
        let backend = ReferenceCpuBackend::new();
        let hidden = vec![1.0f32, 0.5];
        let embed = vec![
            2.0f32, 0.0,
            0.0, 4.0,
            1.0, 1.0,
        ];
        let mut logits = vec![0.0f32; 3];
        backend.compute_logits(&mut logits, &hidden, &embed, 3, 2);

        assert_eq!(logits, vec![2.0, 2.0, 1.5]);
    }
}

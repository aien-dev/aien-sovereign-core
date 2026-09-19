//! Pure native Rust tensor operations for transformer forward execution.
//! Provides cache-friendly vector and matrix math, RMSNorm, RoPE, and SwiGLU activations.

/// Cache-friendly vector-matrix multiplication: out = x * W
/// where x is [in_dim], W is row-major [in_dim, out_dim], and out is [out_dim].
#[inline]
pub fn matmul_vec(x: &[f32], w: &[f32], out: &mut [f32], in_dim: usize, out_dim: usize) {
    debug_assert_eq!(x.len(), in_dim);
    debug_assert_eq!(w.len(), in_dim * out_dim);
    debug_assert_eq!(out.len(), out_dim);

    out.fill(0.0);

    for i in 0..in_dim {
        let xi = x[i];
        let w_row = &w[i * out_dim..(i + 1) * out_dim];
        for j in 0..out_dim {
            out[j] += xi * w_row[j];
        }
    }
}

/// In-place Root Mean Square Normalization:
/// out[i] = x[i] * weight[i] / sqrt(mean(x^2) + eps)
#[inline]
pub fn rmsnorm(x: &[f32], weight: &[f32], eps: f32, out: &mut [f32]) {
    debug_assert_eq!(x.len(), weight.len());
    debug_assert_eq!(x.len(), out.len());

    let dim = x.len();
    let sum_sq: f32 = x.iter().map(|&v| v * v).sum();
    let mean_sq = sum_sq / (dim as f32);
    let scale = 1.0 / (mean_sq + eps).sqrt();

    for i in 0..dim {
        out[i] = x[i] * scale * weight[i];
    }
}

/// Applies Rotary Positional Embeddings (RoPE) to query and key head slices.
/// Rotates consecutive 2D pairs (2*i, 2*i + 1) by angle = pos * theta^(-2*i / head_dim).
#[inline]
pub fn apply_rope(
    q: &mut [f32],
    k: &mut [f32],
    pos: usize,
    num_heads: usize,
    num_kv_heads: usize,
    head_dim: usize,
    theta: f32,
) {
    let half_dim = head_dim / 2;

    // Rotate query heads
    for h in 0..num_heads {
        let head_offset = h * head_dim;
        for i in 0..half_dim {
            let idx0 = head_offset + 2 * i;
            let idx1 = head_offset + 2 * i + 1;

            let exponent = (2 * i) as f32 / (head_dim as f32);
            let freq = 1.0 / theta.powf(exponent);
            let rot = (pos as f32) * freq;
            let (sin_val, cos_val) = rot.sin_cos();

            let q0 = q[idx0];
            let q1 = q[idx1];
            q[idx0] = q0 * cos_val - q1 * sin_val;
            q[idx1] = q0 * sin_val + q1 * cos_val;
        }
    }

    // Rotate key heads
    for h in 0..num_kv_heads {
        let head_offset = h * head_dim;
        for i in 0..half_dim {
            let idx0 = head_offset + 2 * i;
            let idx1 = head_offset + 2 * i + 1;

            let exponent = (2 * i) as f32 / (head_dim as f32);
            let freq = 1.0 / theta.powf(exponent);
            let rot = (pos as f32) * freq;
            let (sin_val, cos_val) = rot.sin_cos();

            let k0 = k[idx0];
            let k1 = k[idx1];
            k[idx0] = k0 * cos_val - k1 * sin_val;
            k[idx1] = k0 * sin_val + k1 * cos_val;
        }
    }
}

/// Scaled Dot-Product Attention for a single token against cached key and value vectors.
/// Supports Grouped-Query Attention (GQA) where num_heads is a multiple of num_kv_heads.
pub fn scaled_dot_product_attention_single(
    q: &[f32],
    cached_k: &[Vec<f32>],
    cached_v: &[Vec<f32>],
    num_heads: usize,
    num_kv_heads: usize,
    head_dim: usize,
    out: &mut [f32],
) {
    debug_assert_eq!(q.len(), num_heads * head_dim);
    debug_assert_eq!(out.len(), num_heads * head_dim);
    debug_assert_eq!(cached_k.len(), cached_v.len());

    let seq_len = cached_k.len();
    if seq_len == 0 {
        out.fill(0.0);
        return;
    }

    let gqa_ratio = num_heads / num_kv_heads;
    let inv_sqrt_d = 1.0 / (head_dim as f32).sqrt();

    let mut scores = vec![0.0f32; seq_len];

    for h in 0..num_heads {
        let kv_head = h / gqa_ratio;
        let q_head = &q[h * head_dim..(h + 1) * head_dim];

        // 1. Compute attention score for each cached token
        for t in 0..seq_len {
            let k_t = &cached_k[t][kv_head * head_dim..(kv_head + 1) * head_dim];
            let dot: f32 = q_head.iter().zip(k_t.iter()).map(|(&a, &b)| a * b).sum();
            scores[t] = dot * inv_sqrt_d;
        }

        // 2. Softmax over sequence length
        let max_score = scores.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mut sum_exp = 0.0f32;
        for s in scores.iter_mut() {
            *s = (*s - max_score).exp();
            sum_exp += *s;
        }
        let inv_sum = 1.0 / sum_exp.max(1e-12);
        for s in scores.iter_mut() {
            *s *= inv_sum;
        }

        // 3. Weighted sum of values
        let out_head = &mut out[h * head_dim..(h + 1) * head_dim];
        out_head.fill(0.0);
        for t in 0..seq_len {
            let weight = scores[t];
            let v_t = &cached_v[t][kv_head * head_dim..(kv_head + 1) * head_dim];
            for d in 0..head_dim {
                out_head[d] += weight * v_t[d];
            }
        }
    }
}

/// SwiGLU Feed-Forward Network:
/// out = (silu(x * W_gate) * (x * W_up)) * W_down
pub fn swiglu(
    x: &[f32],
    gate_w: &[f32],
    up_w: &[f32],
    down_w: &[f32],
    hidden_dim: usize,
    intermediate_dim: usize,
    out: &mut [f32],
) {
    let mut gate = vec![0.0f32; intermediate_dim];
    let mut up = vec![0.0f32; intermediate_dim];
    let mut activated = vec![0.0f32; intermediate_dim];

    matmul_vec(x, gate_w, &mut gate, hidden_dim, intermediate_dim);
    matmul_vec(x, up_w, &mut up, hidden_dim, intermediate_dim);

    // silu(x) = x / (1 + exp(-x))
    for i in 0..intermediate_dim {
        let g = gate[i];
        let silu = g / (1.0 + (-g).exp());
        activated[i] = silu * up[i];
    }

    matmul_vec(&activated, down_w, out, intermediate_dim, hidden_dim);
}

/// Softmax activation over logits vector in-place.
pub fn softmax_inplace(logits: &mut [f32]) {
    let max_val = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let mut sum_exp = 0.0f32;
    for v in logits.iter_mut() {
        *v = (*v - max_val).exp();
        sum_exp += *v;
    }
    let inv_sum = 1.0 / sum_exp.max(1e-12);
    for v in logits.iter_mut() {
        *v *= inv_sum;
    }
}

/// Greedy argmax token selection from logits vector.
/// Returns (token_id, logprob).
pub fn sample_argmax(logits: &[f32]) -> (u32, f32) {
    assert!(!logits.is_empty(), "Logits cannot be empty");

    let mut best_idx = 0;
    let mut best_val = logits[0];

    for (idx, &val) in logits.iter().enumerate().skip(1) {
        if val > best_val {
            best_val = val;
            best_idx = idx;
        }
    }

    // Compute logprob = log(softmax(best_val))
    let max_val = best_val;
    let sum_exp: f32 = logits.iter().map(|&v| (v - max_val).exp()).sum();
    let logprob = best_val - max_val - sum_exp.ln();

    (best_idx as u32, logprob)
}

/// Temperature-scaled sampling with pseudo-random choice.
pub fn sample_temperature(logits: &[f32], temperature: f32, seed: u64) -> (u32, f32) {
    if temperature <= 0.001 {
        return sample_argmax(logits);
    }

    let mut scaled: Vec<f32> = logits.iter().map(|&v| v / temperature).collect();
    softmax_inplace(&mut scaled);

    // Deterministic pseudo-random number generator for reproducible sampling
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
    let r = ((state >> 33) as f32) / ((1u64 << 31) as f32);

    let mut cumulative = 0.0f32;
    for (idx, &prob) in scaled.iter().enumerate() {
        cumulative += prob;
        if r <= cumulative {
            return (idx as u32, prob.max(1e-12).ln());
        }
    }

    let last_idx = logits.len().saturating_sub(1);
    (last_idx as u32, scaled[last_idx].max(1e-12).ln())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rmsnorm_invariance() {
        let x = vec![1.0, 2.0, 3.0, 4.0];
        let weight = vec![1.0, 1.0, 1.0, 1.0];
        let mut out = vec![0.0; 4];

        rmsnorm(&x, &weight, 1e-5, &mut out);

        // RMS of output with all unit weights must equal sqrt(dim) * sqrt(1/dim) = 1.0
        let sum_sq: f32 = out.iter().map(|v| v * v).sum();
        let rms = (sum_sq / 4.0).sqrt();
        assert!((rms - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_rope_norm_preservation() {
        let mut q = vec![1.0, 0.0, 0.0, 1.0];
        let mut k = vec![0.5, 0.5, 0.5, 0.5];

        let orig_q_norm: f32 = q.iter().map(|v| v * v).sum();
        apply_rope(&mut q, &mut k, 5, 1, 1, 4, 10000.0);
        let new_q_norm: f32 = q.iter().map(|v| v * v).sum();

        assert!((orig_q_norm - new_q_norm).abs() < 1e-5);
    }

    #[test]
    fn test_matmul_exact() {
        let x = vec![2.0, 3.0];
        // 2x3 row-major matrix:
        // [1.0, 2.0, 3.0]
        // [4.0, 5.0, 6.0]
        let w = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let mut out = vec![0.0; 3];

        matmul_vec(&x, &w, &mut out, 2, 3);
        // out[0] = 2*1 + 3*4 = 14
        // out[1] = 2*2 + 3*5 = 19
        // out[2] = 2*3 + 3*6 = 24
        assert_eq!(out, vec![14.0, 19.0, 24.0]);
    }

    #[test]
    fn test_swiglu_activation() {
        let x = vec![1.0, 1.0];
        let gate_w = vec![1.0, 1.0]; // 2x1
        let up_w = vec![2.0, 2.0]; // 2x1
        let down_w = vec![1.0, 1.0]; // 1x2

        let mut out = vec![0.0; 2];
        swiglu(&x, &gate_w, &up_w, &down_w, 2, 1, &mut out);

        // gate = 2.0, silu(2.0) = 2.0 / (1 + exp(-2.0)) = 2.0 / 1.135335 = 1.761594
        // up = 4.0
        // activated = 1.761594 * 4.0 = 7.046376
        // out = [7.046376, 7.046376]
        assert!((out[0] - 7.046376).abs() < 1e-4);
        assert!((out[1] - 7.046376).abs() < 1e-4);
    }

    #[test]
    fn test_sample_argmax() {
        let logits = vec![-2.0, 0.5, 4.2, 1.1];
        let (tok, logprob) = sample_argmax(&logits);
        assert_eq!(tok, 2);
        assert!(logprob < 0.0);
    }
}

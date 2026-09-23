//! Attention kernel in isolation: GPU kv_append + attn vs the f64 CPU
//! reference on synthetic Q/K/V, at many sequence lengths. No weights, no MoE,
//! so routing sensitivity cannot mask or mimic a kernel error.
//!
//! Usage: qwen3_attn_kernel_check
//! PASS bar per case: cosine >= 0.999999 and max_abs <= 1e-4.

use aien_inference_abi::qwen3_coder::{
    QWEN3_CODER_HEAD_DIM, QWEN3_CODER_KV_DIM, QWEN3_CODER_KV_HEADS, QWEN3_CODER_Q_DIM,
    QWEN3_CODER_Q_HEADS,
};
use aien_inference_abi::qwen3_moe::parity_stats;
use aien_inference_abi::qwen3_serve::{QwenServeLib, QwenServeModel};
use aien_inference_abi::tensor::scaled_dot_product_attention_single;

const MAX_POS: usize = 512;
const SEQ_LENS: [usize; 8] = [1, 2, 3, 7, 64, 128, 129, 512];

/// Deterministic xorshift in [-1, 1).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    }
    fn vec(&mut self, n: usize, scale: f32) -> Vec<f32> {
        (0..n).map(|_| self.next() * scale).collect()
    }
}

fn main() {
    let lib = QwenServeLib::load(&QwenServeLib::default_path()).expect("load serve .so");
    let serve = QwenServeModel::create(lib).expect("model_create");

    let mut all_pass = true;
    // (label, q/k magnitude): moderate scores, then peaked softmax.
    for (case, (label, mag)) in [("moderate", 1.0f32), ("peaked", 4.0f32)]
        .iter()
        .enumerate()
    {
        let layer = case; // separate layer per case: fresh resident KV
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ case as u64);
        let keys: Vec<Vec<f32>> = (0..MAX_POS)
            .map(|_| rng.vec(QWEN3_CODER_KV_DIM, *mag))
            .collect();
        let values: Vec<Vec<f32>> = (0..MAX_POS)
            .map(|_| rng.vec(QWEN3_CODER_KV_DIM, 1.0))
            .collect();
        for pos in 0..MAX_POS {
            serve
                .kv_append(layer, &keys[pos], &values[pos], pos)
                .expect("kv_append");
        }
        let q = rng.vec(QWEN3_CODER_Q_DIM, *mag);

        for &seq in SEQ_LENS.iter() {
            let mut gpu = vec![0.0f32; QWEN3_CODER_Q_DIM];
            serve.attn(layer, &q, &mut gpu, seq).expect("attn");
            let mut cpu = vec![0.0f32; QWEN3_CODER_Q_DIM];
            scaled_dot_product_attention_single(
                &q,
                &keys[..seq],
                &values[..seq],
                QWEN3_CODER_Q_HEADS,
                QWEN3_CODER_KV_HEADS,
                QWEN3_CODER_HEAD_DIM,
                &mut cpu,
            );
            let s = parity_stats(&gpu, &cpu);
            let pass = s.cosine >= 0.999999 && s.max_abs <= 1e-4;
            all_pass &= pass;
            println!(
                "{label:>8} seq={seq:>4} cosine={:.9} max_abs={:.3e} {}",
                s.cosine,
                s.max_abs,
                if pass { "ok" } else { "FAIL" }
            );
        }
    }
    println!("{}", if all_pass { "PASS" } else { "FAIL" });
    if !all_pass {
        std::process::exit(1);
    }
}

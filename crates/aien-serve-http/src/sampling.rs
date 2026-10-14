//! Shared next-token sampling for engines that produce logits on the host.
//! Top-k is applied here (the workspace `SamplingParams` has no top-k field),
//! then the workspace `sample_with_params` does temperature + nucleus sampling
//! with a seed derived per step so the same request replays the same tokens.

use aien_inference_abi::transformer_backend::{decode_sampling_seed, sample_with_params};
use aien_inference_abi::SamplingParams;

use crate::engine::GenerateParams;

/// Picks the next token from `logits` for decode step `step` (0-based count of
/// tokens already generated in this request).
pub fn sample_next(logits: &[f32], params: &GenerateParams, step: usize) -> u32 {
    let masked;
    let logits = if params.top_k > 0 && params.top_k < logits.len() && params.temperature > 0.001 {
        masked = top_k_mask(logits, params.top_k);
        masked.as_slice()
    } else {
        logits
    };
    let sp = SamplingParams {
        temperature: params.temperature,
        top_p: params.top_p,
        max_tokens: 1,
        stop_token_ids: Vec::new(),
    };
    let seed = decode_sampling_seed(params.seed, step);
    sample_with_params(logits, &sp, seed).0
}

/// Copies `logits` with everything outside the `k` largest values set to -inf.
/// Ties at the boundary keep the lower token id, matching the nucleus tie rule.
fn top_k_mask(logits: &[f32], k: usize) -> Vec<f32> {
    let mut order: Vec<usize> = (0..logits.len()).collect();
    order.sort_by(|&a, &b| {
        logits[b]
            .partial_cmp(&logits[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    let mut out = vec![f32::NEG_INFINITY; logits.len()];
    for &i in &order[..k] {
        out[i] = logits[i];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(temperature: f32, top_k: usize) -> GenerateParams {
        GenerateParams {
            max_new_tokens: 1,
            temperature,
            top_p: 1.0,
            top_k,
            seed: 42,
            stop_token_ids: vec![],
        }
    }

    #[test]
    fn greedy_is_argmax() {
        assert_eq!(sample_next(&[0.1, 5.0, 2.0], &p(0.0, 0), 0), 1);
    }

    #[test]
    fn top_k_never_picks_outside_the_k_best() {
        let logits = [1.0, 9.0, 8.0, 0.5, 7.9];
        for step in 0..200 {
            let t = sample_next(&logits, &p(2.0, 2), step);
            assert!(t == 1 || t == 2, "step {step} picked {t}");
        }
    }

    #[test]
    fn same_seed_same_token() {
        let logits = [1.0, 1.1, 0.9, 1.05];
        for step in 0..20 {
            assert_eq!(
                sample_next(&logits, &p(1.0, 0), step),
                sample_next(&logits, &p(1.0, 0), step)
            );
        }
    }
}

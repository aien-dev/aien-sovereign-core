//! Pure native Rust transformer inference backend executing real forward passes.
//! Performs embedding lookup, RMSNorm, RoPE, Grouped-Query Attention, SwiGLU, and logits projection.

use crate::tensor::{sample_argmax, sample_temperature};
use crate::weights::{LayerKvCache, SequenceState, TransformerWeights};
use crate::{
    AienInferenceBackend, DecodeOutput, FinishReason, ModelConfig, ScheduledBatch, StepMetrics,
};
use async_trait::async_trait;
use std::collections::HashMap;

/// Fully native Rust transformer backend executing real tensor forward computation.
pub struct NativeTransformerBackend {
    pub weights: TransformerWeights,
    pub sequences: HashMap<u64, SequenceState>,
}

impl NativeTransformerBackend {
    pub fn new(weights: TransformerWeights) -> Self {
        Self {
            weights,
            sequences: HashMap::new(),
        }
    }

    pub fn with_reference_weights(config: &ModelConfig) -> Self {
        let weights = TransformerWeights::reference_test_weights(config);
        Self::new(weights)
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

            let mut last_hidden = Vec::new();
            for (idx, &token_id) in req.prompt_tokens.iter().enumerate() {
                seq.tokens.push(token_id);
                last_hidden = self.weights.forward_token(token_id, idx, seq);
            }

            // Compute real logits on the prompt's last token
            let logits = self.weights.compute_logits(&last_hidden);
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
                let pos = seq.tokens.len();
                let last_token = *seq.tokens.last().unwrap_or(&1);

                let hidden = self.weights.forward_token(last_token, pos, seq);
                let logits = self.weights.compute_logits(&hidden);

                let (sampled_tok, logprob) = sample_argmax(&logits);
                seq.tokens.push(sampled_tok);

                let stop_tokens = [0u32, 1u32, 2u32];
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

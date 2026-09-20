//! Autonomous Systems Optimizer (RSI) Boundary Interface
//! Provides read-only runtime snapshots and validated online hyperparameter tuning.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub active_sequences: usize,
    pub queued_sequences: usize,
    pub mean_batch_rows: f32,
    pub logical_kv_bytes: usize,
    pub physical_kv_bytes: usize,
    pub shared_pages: usize,
    pub cow_faults: usize,
    pub fallback_count: usize,
    pub step_latency_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeTuning {
    pub prefill_chunk_tokens: usize,
    pub max_batch_rows: usize,
    pub watermark_blocks: usize,
}

pub struct RsiBoundary;

impl RsiBoundary {
    /// Validates proposed runtime hyperparameter changes against legal operational ranges.
    pub fn validate_tuning(tuning: &RuntimeTuning) -> Result<(), String> {
        if tuning.prefill_chunk_tokens == 0 || tuning.prefill_chunk_tokens > 8192 {
            return Err("prefill_chunk_tokens must be within [1, 8192]".to_string());
        }
        if tuning.max_batch_rows == 0 || tuning.max_batch_rows > 4096 {
            return Err("max_batch_rows must be within [1, 4096]".to_string());
        }
        if tuning.watermark_blocks < 2 || tuning.watermark_blocks > 256 {
            return Err("watermark_blocks must be within [2, 256]".to_string());
        }
        Ok(())
    }
}

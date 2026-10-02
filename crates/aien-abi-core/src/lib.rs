use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelHandle(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContextHandle(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BranchHandle(pub u64);

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AienUsageReceipt {
    pub prefix_tokens: usize,
    pub private_tokens: usize,
    pub logical_pages: usize,
    pub physical_pages: usize,
    pub shared_pages: usize,
    pub private_pages: usize,
    pub cow_faults: usize,
    pub physical_kv_bytes: usize,
    pub bytes_saved_vs_full_copy: usize,
}

fn default_num_kv_heads() -> usize {
    8
}
fn default_vocab_size() -> usize {
    151936
}
fn default_rms_norm_eps() -> f32 {
    1e-6
}
fn default_rope_theta() -> f32 {
    10000.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model_id: String,
    pub max_sequence_length: usize,
    pub block_size: usize,
    pub num_layers: usize,
    pub num_heads: usize,
    pub head_dim: usize,
    #[serde(default = "default_num_kv_heads")]
    pub num_kv_heads: usize,
    #[serde(default)]
    pub hidden_dim: usize,
    #[serde(default)]
    pub intermediate_dim: usize,
    #[serde(default = "default_vocab_size")]
    pub vocab_size: usize,
    #[serde(default = "default_rms_norm_eps")]
    pub rms_norm_eps: f32,
    #[serde(default = "default_rope_theta")]
    pub rope_theta: f32,
}

impl ModelConfig {
    pub fn tinyllama_1_1b() -> Self {
        Self {
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
        }
    }

    pub fn hidden_dim(&self) -> usize {
        if self.hidden_dim > 0 {
            self.hidden_dim
        } else {
            self.num_heads * self.head_dim
        }
    }

    pub fn intermediate_dim(&self) -> usize {
        if self.intermediate_dim > 0 {
            self.intermediate_dim
        } else {
            self.hidden_dim() * 4
        }
    }

    pub fn vocab_size(&self) -> usize {
        if self.vocab_size > 0 {
            self.vocab_size
        } else {
            151936
        }
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model_id: "nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16".to_string(),
            max_sequence_length: 32768,
            block_size: 16,
            num_layers: 48,
            num_heads: 32,
            head_dim: 128,
            num_kv_heads: 8,
            hidden_dim: 4096,
            intermediate_dim: 14336,
            vocab_size: 151936,
            rms_norm_eps: 1e-6,
            rope_theta: 10000.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingParams {
    pub temperature: f32,
    pub top_p: f32,
    pub max_tokens: usize,
    pub stop_token_ids: Vec<u32>,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            top_p: 0.95,
            max_tokens: 512,
            stop_token_ids: vec![0, 1, 2],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceRequest {
    pub request_id: u64,
    pub prompt_tokens: Vec<u32>,
    pub sampling_params: SamplingParams,
    pub arrival_time_ns: u64,
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledBatch {
    pub prefill_requests: Vec<SequenceRequest>,
    pub decode_requests: Vec<u64>,
    pub block_tables: HashMap<u64, Vec<usize>>,
    pub step_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    StopToken,
    LengthLimit,
    Aborted,
    Preempted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecodeOutput {
    Token {
        request_id: u64,
        token_id: u32,
        logprob: Option<f32>,
    },
    Finished {
        request_id: u64,
        reason: FinishReason,
        total_tokens: usize,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StepMetrics {
    pub prefill_tokens_processed: usize,
    pub decode_tokens_emitted: usize,
    pub step_latency_us: u64,
    pub active_kv_blocks: usize,
}

#[async_trait]
pub trait AienInferenceBackend: Send + Sync {
    async fn load_model(&mut self, config: &ModelConfig) -> Result<(), String>;
    async fn execute_step(
        &mut self,
        batch: &ScheduledBatch,
    ) -> Result<(Vec<DecodeOutput>, StepMetrics), String>;

    /// Indicates whether the backend directly manages and appends tokens into the KV cache.
    /// When true, the scheduler avoids redundant secondary append operations during decode steps.
    fn manages_kv_cache(&self) -> bool {
        false
    }

    /// PREFILL-E2E-0 bullets 6-8: forks the backend's per-sequence state
    /// (tokens, position, last token) from a prefilled parent into a child.
    /// Called by the runtime spine once per branch AFTER the root passed the
    /// prefill completion fence and the KV manager shared the root blocks
    /// (`fork_branches_from_ready_root`), and BEFORE any branch is decoded.
    /// Must never copy or re-fork K/V: the physical KV fork is already done.
    /// Default: no-op, for backends that keep no per-sequence state.
    fn fork_sequence(&mut self, _parent_id: u64, _child_id: u64) -> Result<(), String> {
        Ok(())
    }

    /// PREFILL-E2E C6 (bullet 11): `fork_sequence` that also hands the
    /// backend the child's own sampling params, so a branch decodes with its
    /// request's temperature/top_p instead of argmax. Decode batches carry
    /// only request ids (`ScheduledBatch::decode_requests`), so the fork hook
    /// is where a branch's params reach the backend.
    /// Default: plain `fork_sequence` (params ignored).
    fn fork_sequence_with_sampling(
        &mut self,
        parent_id: u64,
        child_id: u64,
        _sampling: &SamplingParams,
    ) -> Result<(), String> {
        self.fork_sequence(parent_id, child_id)
    }

    /// Drops the backend's per-sequence state for `seq_id` (PREFILL-E2E
    /// bullet 12). The runtime spine calls it for every finished branch, for
    /// the swarm root once its last branch finished, and for every sequence of
    /// a cancelled swarm, always after the runtime freed the KV blocks and
    /// never while a step is executing. Must not free KV: the runtime owns the
    /// block tables. Unknown ids are a no-op (a cancelled branch may already
    /// have finished). Default: no-op.
    fn release_sequence(&mut self, _seq_id: u64) -> Result<(), String> {
        Ok(())
    }
}

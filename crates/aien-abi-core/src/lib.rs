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

/// Why an [`AttentionGeometry`] was refused. Every variant names the offending numbers so a
/// refusal in a log says what to fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AttentionGeometryError {
    ZeroQueryHeads,
    ZeroKvHeads,
    ZeroHeadDim,
    /// `num_q_heads` is not a whole multiple of `num_kv_heads` (every kv head must serve the
    /// same number of query heads).
    QueryHeadsNotDivisible {
        num_q_heads: usize,
        num_kv_heads: usize,
    },
    /// `num_q_heads * head_dim` or `num_kv_heads * head_dim` does not fit in `usize`.
    Overflow {
        heads: usize,
        head_dim: usize,
    },
    /// A KV pool (or any KV buffer) is laid out for a different kv-head count or head width
    /// than this geometry.
    KvPoolMismatch {
        geometry_kv_heads: usize,
        geometry_head_dim: usize,
        pool_kv_heads: usize,
        pool_head_dim: usize,
    },
}

impl std::fmt::Display for AttentionGeometryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroQueryHeads => write!(f, "attention geometry: num_q_heads is 0"),
            Self::ZeroKvHeads => write!(f, "attention geometry: num_kv_heads is 0"),
            Self::ZeroHeadDim => write!(f, "attention geometry: head_dim is 0"),
            Self::QueryHeadsNotDivisible {
                num_q_heads,
                num_kv_heads,
            } => write!(
                f,
                "attention geometry: num_q_heads {num_q_heads} is not a multiple of num_kv_heads {num_kv_heads}"
            ),
            Self::Overflow { heads, head_dim } => write!(
                f,
                "attention geometry: {heads} heads x head_dim {head_dim} overflows usize"
            ),
            Self::KvPoolMismatch {
                geometry_kv_heads,
                geometry_head_dim,
                pool_kv_heads,
                pool_head_dim,
            } => write!(
                f,
                "attention geometry: model has {geometry_kv_heads} kv heads x head_dim {geometry_head_dim} but the KV pool is laid out for {pool_kv_heads} kv heads x head_dim {pool_head_dim}"
            ),
        }
    }
}

impl std::error::Error for AttentionGeometryError {}

/// The checked head geometry of one attention layer: `num_q_heads` query heads sharing
/// `num_kv_heads` key/value heads, every head `head_dim` wide.
///
/// Construction is the only way to get a value, so a geometry in hand always satisfies:
/// no zero count, `num_q_heads` divisible by `num_kv_heads`, and both `q_dim` and `kv_dim`
/// fit in `usize`. The contract deliberately does NOT require a power-of-two GQA ratio,
/// `q_dim == hidden_dim` (Qwen3-Coder: q_dim 4096, hidden 2048) or `num_q_heads ==
/// num_kv_heads`; a kernel with a narrower envelope refuses on its own and says so.
///
/// Flattened layouts the geometry describes:
/// - q and attention output: `[num_q_heads][head_dim]` (`q_dim` values), head h at
///   `h * head_dim`;
/// - one token of K or V: `[num_kv_heads][head_dim]` (`kv_dim` values);
/// - query head h reads kv head `h / gqa_ratio` ([`Self::kv_head_of`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "AttentionGeometryRaw", into = "AttentionGeometryRaw")]
pub struct AttentionGeometry {
    num_q_heads: usize,
    num_kv_heads: usize,
    head_dim: usize,
}

/// Serde shape of [`AttentionGeometry`]; deserialising re-runs the checks.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AttentionGeometryRaw {
    pub num_q_heads: usize,
    pub num_kv_heads: usize,
    pub head_dim: usize,
}

impl TryFrom<AttentionGeometryRaw> for AttentionGeometry {
    type Error = AttentionGeometryError;
    fn try_from(r: AttentionGeometryRaw) -> Result<Self, Self::Error> {
        Self::new(r.num_q_heads, r.num_kv_heads, r.head_dim)
    }
}

impl From<AttentionGeometry> for AttentionGeometryRaw {
    fn from(g: AttentionGeometry) -> Self {
        Self {
            num_q_heads: g.num_q_heads,
            num_kv_heads: g.num_kv_heads,
            head_dim: g.head_dim,
        }
    }
}

impl AttentionGeometry {
    /// Checks and builds the geometry. See the type docs for the exact rules.
    pub fn new(
        num_q_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
    ) -> Result<Self, AttentionGeometryError> {
        if num_q_heads == 0 {
            return Err(AttentionGeometryError::ZeroQueryHeads);
        }
        if num_kv_heads == 0 {
            return Err(AttentionGeometryError::ZeroKvHeads);
        }
        if head_dim == 0 {
            return Err(AttentionGeometryError::ZeroHeadDim);
        }
        if !num_q_heads.is_multiple_of(num_kv_heads) {
            return Err(AttentionGeometryError::QueryHeadsNotDivisible {
                num_q_heads,
                num_kv_heads,
            });
        }
        if num_q_heads.checked_mul(head_dim).is_none() {
            return Err(AttentionGeometryError::Overflow {
                heads: num_q_heads,
                head_dim,
            });
        }
        // num_kv_heads <= num_q_heads here, so kv_dim fits whenever q_dim does.
        Ok(Self {
            num_q_heads,
            num_kv_heads,
            head_dim,
        })
    }

    pub fn num_q_heads(&self) -> usize {
        self.num_q_heads
    }

    pub fn num_kv_heads(&self) -> usize {
        self.num_kv_heads
    }

    pub fn head_dim(&self) -> usize {
        self.head_dim
    }

    /// Width of one flattened query (and of one attention output): `num_q_heads * head_dim`.
    pub fn q_dim(&self) -> usize {
        self.num_q_heads * self.head_dim
    }

    /// Width of one token's K (or V) row: `num_kv_heads * head_dim`.
    pub fn kv_dim(&self) -> usize {
        self.num_kv_heads * self.head_dim
    }

    /// Query heads per kv head (1 for multi-head attention).
    pub fn gqa_ratio(&self) -> usize {
        self.num_q_heads / self.num_kv_heads
    }

    /// The kv head query head `q_head` reads. `None` when `q_head >= num_q_heads`.
    pub fn kv_head_of(&self, q_head: usize) -> Option<usize> {
        (q_head < self.num_q_heads).then(|| q_head / self.gqa_ratio())
    }

    /// Refuses a KV pool or buffer laid out for a different kv-head count or head width.
    /// Call it before any native (chip) code reads the pool with this geometry.
    pub fn check_kv_pool(
        &self,
        pool_kv_heads: usize,
        pool_head_dim: usize,
    ) -> Result<(), AttentionGeometryError> {
        if pool_kv_heads != self.num_kv_heads || pool_head_dim != self.head_dim {
            return Err(AttentionGeometryError::KvPoolMismatch {
                geometry_kv_heads: self.num_kv_heads,
                geometry_head_dim: self.head_dim,
                pool_kv_heads,
                pool_head_dim,
            });
        }
        Ok(())
    }
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

    /// The checked head geometry of this model's attention layers
    /// (`num_heads` query heads, `num_kv_heads` kv heads, `head_dim`).
    pub fn attention_geometry(&self) -> Result<AttentionGeometry, AttentionGeometryError> {
        AttentionGeometry::new(self.num_heads, self.num_kv_heads, self.head_dim)
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

//! One KV for runtime and backend (PREFILL-E2E-0 cut C2).
//!
//! Before this module the daemon built the spine on a pool-less KV manager
//! and the backend with no KV manager at all, so block tables in the runtime
//! were bookkeeping only and the real K/V lived in the backend's private
//! dense per-request cache. `build_shared_kv_runtime` creates ONE manager with
//! a physical tensor pool sized for the loaded model and hands the same `Arc`
//! to the spine (scheduler, swarm forks, reclaim) and to the backend (which
//! writes prefill/decode K/V into the pool blocks named by the spine's block
//! tables).
//!
//! Lives in aien-runtime (not aien-cli) because this crate owns the spine and
//! already depends on aien-inference-abi and aien-kv-cache, so the assembly
//! can be exercised by integration tests next to the other spine tests.

use std::sync::Arc;

use aien_inference_abi::{NativeTransformerBackend, TensorBackend, TransformerWeights};
use aien_kv_cache::{create_shared_kv_manager_with_pool, KvDType, KvPoolConfig, SharedKvManager};
use aien_scheduler::SchedulerConfig;

use crate::spine::AienRuntimeSpine;

/// Sizing for the shared runtime/backend KV manager.
#[derive(Debug, Clone, Copy)]
pub struct SharedKvSizing {
    /// Sequence arena capacity of the spine.
    pub arena_capacity: usize,
    /// Number of physical KV blocks in the pool (and in the block allocator).
    pub total_blocks: usize,
}

/// Builds the pooled KV manager for `weights.config`. The block size is the
/// model config's `block_size`, because the backend's paged writes index
/// blocks with `weights.config.block_size`
/// (aien-inference-abi/src/transformer_backend.rs, prefill paged path).
pub fn build_model_kv_manager(
    weights: &TransformerWeights,
    total_blocks: usize,
) -> Result<SharedKvManager, String> {
    let cfg = &weights.config;
    if cfg.block_size == 0 {
        return Err("shared KV: model config block_size is 0".to_string());
    }
    let pool_cfg = KvPoolConfig::for_model(
        total_blocks,
        cfg.block_size,
        cfg.num_layers,
        cfg.num_kv_heads,
        cfg.head_dim,
        KvDType::Fp32,
    );
    create_shared_kv_manager_with_pool(total_blocks, cfg.block_size, pool_cfg)
}

/// Builds the runtime spine and the native backend over ONE pooled KV
/// manager. The returned backend's `kv_manager` is the same `Arc` as the
/// spine's `kv_manager` (`Arc::ptr_eq`).
pub fn build_shared_kv_runtime(
    weights: TransformerWeights,
    tensor_backend: Arc<dyn TensorBackend>,
    scheduler_config: SchedulerConfig,
    sizing: SharedKvSizing,
) -> Result<(AienRuntimeSpine, NativeTransformerBackend), String> {
    let kv_manager = build_model_kv_manager(&weights, sizing.total_blocks)?;
    let spine = AienRuntimeSpine::new(sizing.arena_capacity, scheduler_config, kv_manager.clone());
    let backend =
        NativeTransformerBackend::with_shared_kv_and_backend(weights, tensor_backend, kv_manager);
    Ok((spine, backend))
}

/// Element type of the shared pool. `build_model_kv_manager` lays the pool out
/// in this dtype, and [`KvPoolPlan`] prices it with the same value.
pub const SHARED_KV_DTYPE: KvDType = KvDType::Fp32;

/// The daemon's KV pool, sized from the loaded model (issue #239).
///
/// Before #239 the daemon asked for a fixed 8192 blocks for every model. With
/// the 16-token blocks of `model_dir.rs` that is 131072 tokens, which happens to
/// be Llama-3.2-1B's `max_position_embeddings`, but it priced every other model
/// at 131072 tokens too (SmolLM2-1.7B: 48 GiB of Fp32 KV for a model whose
/// context is 8192 tokens). The plan now takes the declared context budget from
/// the model itself (`ModelConfig::max_sequence_length`, i.e. config.json
/// `max_position_embeddings`) and derives
/// `bytes = blocks * block_size * num_layers * num_kv_heads * head_dim * 2 (K and V) * dtype bytes`,
/// with `blocks = ceil(context / block_size)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvPoolPlan {
    /// Declared context budget in tokens (where it came from: `context_source`).
    pub context_tokens: usize,
    /// Where `context_tokens` was taken from, for the startup log.
    pub context_source: &'static str,
    pub block_size: usize,
    pub total_blocks: usize,
    pub num_layers: usize,
    pub num_kv_heads: usize,
    pub head_dim: usize,
    pub dtype_bytes: usize,
    /// K plus V bytes for one token across all layers.
    pub bytes_per_token: usize,
    /// Bytes of the whole pool (`total_blocks * block_size * bytes_per_token`).
    pub total_bytes: usize,
}

const GIB: f64 = (1u64 << 30) as f64;

/// Bytes as GiB with two decimals, for logs and refusals.
pub fn fmt_gib(bytes: u64) -> String {
    format!("{:.2} GiB", bytes as f64 / GIB)
}

impl KvPoolPlan {
    /// Plan for `cfg` at the model's own declared context (`max_sequence_length`).
    pub fn for_model(cfg: &aien_inference_abi::ModelConfig) -> Result<Self, String> {
        Self::for_context(
            cfg,
            cfg.max_sequence_length,
            "model max_sequence_length (config.json max_position_embeddings)",
        )
    }

    /// Plan for `cfg` at the model's own context, or at an operator cap that can only
    /// lower it ([`KV_CONTEXT_CAP_ENV`]). A cap above the model's declared context is
    /// refused, never used to enlarge the pool.
    pub fn for_model_capped(
        cfg: &aien_inference_abi::ModelConfig,
        cap: Option<usize>,
    ) -> Result<Self, String> {
        match cap {
            None => Self::for_model(cfg),
            Some(c) if c > cfg.max_sequence_length => Err(format!(
                "{KV_CONTEXT_CAP_ENV}={c} exceeds the model's declared context ({} tokens); the cap can only lower it",
                cfg.max_sequence_length
            )),
            Some(c) => Self::for_context(cfg, c, "operator cap (AIEN_KV_CONTEXT_TOKENS)"),
        }
    }

    /// Plan for `cfg` at an explicit context budget. Every product is checked, so a
    /// config that would overflow `usize` is refused instead of wrapping.
    pub fn for_context(
        cfg: &aien_inference_abi::ModelConfig,
        context_tokens: usize,
        context_source: &'static str,
    ) -> Result<Self, String> {
        let named = [
            ("context_tokens", context_tokens),
            ("block_size", cfg.block_size),
            ("num_layers", cfg.num_layers),
            ("num_kv_heads", cfg.num_kv_heads),
            ("head_dim", cfg.head_dim),
        ];
        if let Some((name, _)) = named.iter().find(|(_, v)| *v == 0) {
            return Err(format!("KV pool plan: {name} is 0"));
        }
        // Fp32 is 4 bytes; the f32 accessor is exact for every whole-byte dtype.
        let dtype_bytes = SHARED_KV_DTYPE.bytes_per_element() as usize;
        let overflow = || "KV pool plan: size overflows usize".to_string();
        let bytes_per_token = 2usize
            .checked_mul(cfg.num_layers)
            .and_then(|v| v.checked_mul(cfg.num_kv_heads))
            .and_then(|v| v.checked_mul(cfg.head_dim))
            .and_then(|v| v.checked_mul(dtype_bytes))
            .ok_or_else(overflow)?;
        let total_blocks = context_tokens.div_ceil(cfg.block_size);
        let total_bytes = total_blocks
            .checked_mul(cfg.block_size)
            .and_then(|v| v.checked_mul(bytes_per_token))
            .ok_or_else(overflow)?;
        Ok(Self {
            context_tokens,
            context_source,
            block_size: cfg.block_size,
            total_blocks,
            num_layers: cfg.num_layers,
            num_kv_heads: cfg.num_kv_heads,
            head_dim: cfg.head_dim,
            dtype_bytes,
            bytes_per_token,
            total_bytes,
        })
    }

    /// The pool config `build_model_kv_manager` builds for this plan.
    pub fn pool_config(&self) -> KvPoolConfig {
        KvPoolConfig::for_model(
            self.total_blocks,
            self.block_size,
            self.num_layers,
            self.num_kv_heads,
            self.head_dim,
            SHARED_KV_DTYPE,
        )
    }

    /// One startup log line: size in bytes and GiB plus every derived parameter.
    pub fn log_line(&self) -> String {
        format!(
            "KV pool: {} bytes ({}) = {} blocks x {} tokens x {} layers x {} kv heads x {} head_dim x 2 (K,V) x {} bytes; context {} tokens from {}",
            self.total_bytes,
            fmt_gib(self.total_bytes as u64),
            self.total_blocks,
            self.block_size,
            self.num_layers,
            self.num_kv_heads,
            self.head_dim,
            self.dtype_bytes,
            self.context_tokens,
            self.context_source,
        )
    }
}

/// `MemAvailable` from the text of `/proc/meminfo`, in bytes (the file reports kB).
pub fn parse_mem_available(meminfo: &str) -> Option<u64> {
    meminfo.lines().find_map(|line| {
        let rest = line.strip_prefix("MemAvailable:")?;
        let mut parts = rest.split_whitespace();
        let value: u64 = parts.next()?.parse().ok()?;
        match parts.next() {
            Some("kB") => value.checked_mul(1024),
            _ => None,
        }
    })
}

/// `MemAvailable` of this host in bytes, or `None` where `/proc/meminfo` is absent
/// or has no parsable `MemAvailable` line.
pub fn read_mem_available() -> Option<u64> {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .as_deref()
        .and_then(parse_mem_available)
}

/// Why `MemAvailable` could not be established, as a plain message.
pub fn parse_mem_available_checked(meminfo: &str) -> Result<u64, String> {
    parse_mem_available(meminfo)
        .ok_or_else(|| "/proc/meminfo has no parsable `MemAvailable: <n> kB` line".to_string())
}

/// `MemAvailable` of this host in bytes, or the reason it cannot be established
/// (`/proc/meminfo` unreadable, or no parsable `MemAvailable` line).
pub fn read_mem_available_checked() -> Result<u64, String> {
    let text = std::fs::read_to_string("/proc/meminfo")
        .map_err(|e| format!("/proc/meminfo cannot be read: {e}"))?;
    parse_mem_available_checked(&text)
}

/// Operator override (documented): set to `1` to let the daemon allocate the KV pool
/// although `MemAvailable` cannot be established. Without it the daemon refuses to
/// start (fail closed). Every use is logged loudly on stderr.
pub const ALLOW_UNCHECKED_MEMORY_ENV: &str = "AIEN_ALLOW_UNCHECKED_KV_MEMORY";

/// True only when [`ALLOW_UNCHECKED_MEMORY_ENV`] is exactly `1`.
pub fn allow_unchecked_memory_from_env() -> bool {
    std::env::var(ALLOW_UNCHECKED_MEMORY_ENV).is_ok_and(|v| v == "1")
}

/// Operator cap (documented): the daemon's KV context budget in tokens, a positive
/// decimal integer no larger than the model's declared context. Unset: the model's
/// own context (unchanged behaviour). A sequence that outgrows the capped pool is
/// refused with `KV_POOL_EXHAUSTED_PREFIX` before anything is mutated.
pub const KV_CONTEXT_CAP_ENV: &str = "AIEN_KV_CONTEXT_TOKENS";

/// Parses a [`KV_CONTEXT_CAP_ENV`] value: digits only, greater than zero.
pub fn parse_kv_context_cap(value: &str) -> Result<usize, String> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "{KV_CONTEXT_CAP_ENV}={value:?} is not a positive decimal token count"
        ));
    }
    match value.parse::<usize>() {
        Ok(0) => Err(format!(
            "{KV_CONTEXT_CAP_ENV}=0: the cap must be at least 1 token"
        )),
        Ok(n) => Ok(n),
        Err(e) => Err(format!("{KV_CONTEXT_CAP_ENV}={value:?}: {e}")),
    }
}

/// The operator cap from the environment: `Ok(None)` when unset, a refusal when set
/// but not a valid count (including non-UTF-8).
pub fn kv_context_cap_from_env() -> Result<Option<usize>, String> {
    match std::env::var(KV_CONTEXT_CAP_ENV) {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(format!("{KV_CONTEXT_CAP_ENV}: {e}")),
        Ok(v) => parse_kv_context_cap(&v).map(Some),
    }
}

/// Refuses the pool when the host reports less available memory than it needs.
/// `available` is `Err(reason)` when memory could not be established: that refuses
/// too (fail closed, #254) unless `allow_unchecked` is set, in which case the check
/// is bypassed with a loud stderr line.
pub fn check_kv_memory(
    requested: u64,
    available: Result<u64, String>,
    allow_unchecked: bool,
) -> Result<(), String> {
    match available {
        Ok(avail) if requested > avail => Err(format!(
            "KV pool needs {requested} bytes ({}) but MemAvailable is {avail} bytes ({}); refusing to allocate",
            fmt_gib(requested),
            fmt_gib(avail),
        )),
        Ok(_) => Ok(()),
        Err(reason) if allow_unchecked => {
            eprintln!(
                "WARNING: KV POOL MEMORY CHECK BYPASSED by {ALLOW_UNCHECKED_MEMORY_ENV}=1: {reason}; allocating {requested} bytes ({}) WITHOUT knowing MemAvailable",
                fmt_gib(requested)
            );
            Ok(())
        }
        Err(reason) => Err(format!(
            "KV pool memory cannot be checked: {reason}; refusing to allocate {requested} bytes ({}). Set {ALLOW_UNCHECKED_MEMORY_ENV}=1 to override (not recommended)",
            fmt_gib(requested)
        )),
    }
}

/// Daemon planning step (#239): plans the pool for `cfg`, logs the plan, and
/// checks `MemAvailable` (read through `mem_available`) before anything is
/// allocated. Allocates nothing itself. Fails closed when memory cannot be
/// established unless `allow_unchecked` (#254).
pub fn plan_checked_model_kv(
    cfg: &aien_inference_abi::ModelConfig,
    context_cap: Option<usize>,
    mem_available: &dyn Fn() -> Result<u64, String>,
    allow_unchecked: bool,
) -> Result<KvPoolPlan, String> {
    let plan = KvPoolPlan::for_model_capped(cfg, context_cap)?;
    println!("  {}", plan.log_line());
    let available = mem_available();
    if let Ok(avail) = &available {
        println!(
            "  KV pool memory check: needs {} bytes ({}), MemAvailable {} bytes ({})",
            plan.total_bytes,
            fmt_gib(plan.total_bytes as u64),
            avail,
            fmt_gib(*avail)
        );
    }
    check_kv_memory(plan.total_bytes as u64, available, allow_unchecked)?;
    Ok(plan)
}

/// Daemon path (#239): [`plan_checked_model_kv`], then builds the spine and
/// backend over the one pooled manager sized by the plan.
pub fn build_shared_kv_runtime_for_model(
    weights: TransformerWeights,
    tensor_backend: Arc<dyn TensorBackend>,
    scheduler_config: SchedulerConfig,
    arena_capacity: usize,
    context_cap: Option<usize>,
    mem_available: &dyn Fn() -> Result<u64, String>,
    allow_unchecked: bool,
) -> Result<(AienRuntimeSpine, NativeTransformerBackend, KvPoolPlan), String> {
    let plan = plan_checked_model_kv(&weights.config, context_cap, mem_available, allow_unchecked)?;
    let (spine, backend) = build_shared_kv_runtime(
        weights,
        tensor_backend,
        scheduler_config,
        SharedKvSizing {
            arena_capacity,
            total_blocks: plan.total_blocks,
        },
    )?;
    Ok((spine, backend, plan))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aien_inference_abi::model_config_from_hf_json;

    /// Blocks the daemon asked for before #239 (aien-cli/src/commands.rs,
    /// `SharedKvSizing { total_blocks: 8192 }`, sovereign-core 328a7e9).
    const OLD_FIXED_DAEMON_BLOCKS: usize = 8192;

    /// unsloth/Llama-3.2-1B-Instruct config.json, snapshot 5a8abab (the KV-relevant
    /// fields as on disk; same text as the model_dir.rs fixture).
    const LLAMA32_1B_CONFIG: &str = r#"{
  "architectures": ["LlamaForCausalLM"], "attention_bias": false, "attention_dropout": 0.0,
  "bos_token_id": 128000, "eos_token_id": 128009, "head_dim": 64, "hidden_act": "silu",
  "hidden_size": 2048, "initializer_range": 0.02, "intermediate_size": 8192,
  "max_position_embeddings": 131072, "mlp_bias": false, "model_type": "llama",
  "num_attention_heads": 32, "num_hidden_layers": 16, "num_key_value_heads": 8,
  "pad_token_id": 128004, "pretraining_tp": 1, "rms_norm_eps": 1e-05,
  "rope_scaling": {"factor": 32.0, "high_freq_factor": 4.0, "low_freq_factor": 1.0,
    "original_max_position_embeddings": 8192, "rope_type": "llama3"},
  "rope_theta": 500000.0, "tie_word_embeddings": true, "torch_dtype": "bfloat16",
  "vocab_size": 128256
}"#;

    /// HuggingFaceTB/SmolLM2-1.7B-Instruct config.json, revision 31b70e2e (the
    /// fields this engine reads, as on disk; transformers.js block dropped).
    const SMOLLM2_17B_CONFIG: &str = r#"{
  "architectures": ["LlamaForCausalLM"], "attention_bias": false, "attention_dropout": 0.0,
  "bos_token_id": 1, "eos_token_id": 2, "hidden_act": "silu", "hidden_size": 2048,
  "initializer_range": 0.02, "intermediate_size": 8192, "max_position_embeddings": 8192,
  "mlp_bias": false, "model_type": "llama", "num_attention_heads": 32,
  "num_hidden_layers": 24, "num_key_value_heads": 32, "pad_token_id": 2,
  "pretraining_tp": 1, "rms_norm_eps": 1e-05, "rope_scaling": null, "rope_theta": 130000,
  "tie_word_embeddings": true, "torch_dtype": "bfloat16", "use_cache": true,
  "vocab_size": 49152
}"#;

    fn cfg(id: &str, json: &str) -> aien_inference_abi::ModelConfig {
        model_config_from_hf_json(id, json, None).expect("config.json parses")
    }

    fn old_fixed_bytes(c: &aien_inference_abi::ModelConfig) -> usize {
        KvPoolConfig::for_model(
            OLD_FIXED_DAEMON_BLOCKS,
            c.block_size,
            c.num_layers,
            c.num_kv_heads,
            c.head_dim,
            KvDType::Fp32,
        )
        .total_bytes()
    }

    #[test]
    fn llama32_1b_plan_is_byte_identical_to_the_old_fixed_pool() {
        let c = cfg("unsloth/Llama-3.2-1B-Instruct", LLAMA32_1B_CONFIG);
        let plan = KvPoolPlan::for_model(&c).unwrap();
        assert_eq!(plan.context_tokens, 131_072);
        assert_eq!(plan.block_size, 16);
        assert_eq!(
            (plan.num_layers, plan.num_kv_heads, plan.head_dim),
            (16, 8, 64)
        );
        assert_eq!(plan.total_blocks, OLD_FIXED_DAEMON_BLOCKS);
        // 2 * 16 * 8 * 64 * 4
        assert_eq!(plan.bytes_per_token, 65_536);
        assert_eq!(plan.total_bytes, 8_589_934_592); // 8.00 GiB
        assert_eq!(plan.total_bytes, old_fixed_bytes(&c));
        assert_eq!(plan.total_bytes, plan.pool_config().total_bytes());
    }

    #[test]
    fn smollm2_plan_is_sized_from_its_own_context() {
        let c = cfg("HuggingFaceTB/SmolLM2-1.7B-Instruct", SMOLLM2_17B_CONFIG);
        let plan = KvPoolPlan::for_model(&c).unwrap();
        assert_eq!(plan.context_tokens, 8192);
        assert_eq!(
            (plan.num_layers, plan.num_kv_heads, plan.head_dim),
            (24, 32, 64)
        );
        assert_eq!(plan.total_blocks, 512);
        // 2 * 24 * 32 * 64 * 4
        assert_eq!(plan.bytes_per_token, 393_216);
        assert_eq!(plan.total_bytes, 3_221_225_472); // 3.00 GiB
        assert_eq!(plan.total_bytes, plan.pool_config().total_bytes());
        // The old fixed pool was 16x larger: 48.00 GiB (51.5 GB).
        assert_eq!(old_fixed_bytes(&c), 51_539_607_552);
        assert_eq!(old_fixed_bytes(&c), 16 * plan.total_bytes);
    }

    #[test]
    fn plan_rounds_a_partial_block_up_and_refuses_zero_or_overflow() {
        let mut c = cfg("unsloth/Llama-3.2-1B-Instruct", LLAMA32_1B_CONFIG);
        let plan = KvPoolPlan::for_context(&c, 17, "test").unwrap();
        assert_eq!(plan.total_blocks, 2);
        assert_eq!(plan.total_bytes, 2 * 16 * 65_536);
        assert!(KvPoolPlan::for_context(&c, 0, "test")
            .unwrap_err()
            .contains("context_tokens is 0"));
        c.num_layers = usize::MAX / 2;
        assert!(KvPoolPlan::for_model(&c).unwrap_err().contains("overflow"));
    }

    #[test]
    fn log_line_names_bytes_gib_and_every_parameter() {
        let c = cfg("unsloth/Llama-3.2-1B-Instruct", LLAMA32_1B_CONFIG);
        let line = KvPoolPlan::for_model(&c).unwrap().log_line();
        for want in [
            "8589934592 bytes",
            "8.00 GiB",
            "8192 blocks",
            "16 tokens",
            "16 layers",
            "8 kv heads",
            "64 head_dim",
            "4 bytes",
            "context 131072 tokens",
            "max_position_embeddings",
        ] {
            assert!(line.contains(want), "{want:?} missing from {line:?}");
        }
    }

    #[test]
    fn mem_available_parses_kb_and_rejects_garbage() {
        let meminfo =
            "MemTotal:       130000000 kB\nMemFree:         1000 kB\nMemAvailable:   62000000 kB\n";
        assert_eq!(parse_mem_available(meminfo), Some(62_000_000 * 1024));
        assert_eq!(parse_mem_available("MemTotal: 1 kB\n"), None);
        assert_eq!(parse_mem_available("MemAvailable: x kB\n"), None);
        assert_eq!(parse_mem_available("MemAvailable: 5\n"), None);
    }

    #[test]
    fn memory_check_refuses_with_requested_and_available_bytes() {
        let err = check_kv_memory(3_221_225_472, Ok(1_073_741_824), false).unwrap_err();
        assert!(err.contains("3221225472 bytes (3.00 GiB)"), "{err}");
        assert!(err.contains("1073741824 bytes (1.00 GiB)"), "{err}");
        assert!(err.contains("refusing"), "{err}");
        assert!(check_kv_memory(10, Ok(10), false).is_ok());
    }

    fn tiny_config() -> aien_inference_abi::ModelConfig {
        aien_inference_abi::ModelConfig {
            model_id: "kv239-tiny".to_string(),
            max_sequence_length: 40,
            block_size: 16,
            num_layers: 2,
            num_heads: 4,
            head_dim: 16,
            num_kv_heads: 2,
            hidden_dim: 64,
            intermediate_dim: 128,
            vocab_size: 64,
            rms_norm_eps: 1e-5,
            rope_theta: 10000.0,
            rope_scaling: None,
            tie_word_embeddings: false,
            eos_token_ids: Vec::new(),
            qk_norm: false,
        }
    }

    // ---- #254: fail closed when memory cannot be established ----

    #[test]
    fn unreadable_meminfo_refuses_the_pool() {
        let cfg = tiny_config();
        let err = plan_checked_model_kv(
            &cfg,
            None,
            &|| Err("/proc/meminfo cannot be read: No such file".to_string()),
            false,
        )
        .unwrap_err();
        assert!(err.contains("cannot be checked"), "{err}");
        assert!(err.contains("refusing to allocate"), "{err}");
        assert!(err.contains(ALLOW_UNCHECKED_MEMORY_ENV), "{err}");
    }

    #[test]
    fn unreadable_meminfo_with_override_proceeds() {
        // The override path also prints a WARNING line on stderr (see check_kv_memory).
        let cfg = tiny_config();
        let plan = plan_checked_model_kv(&cfg, None, &|| Err("unreadable".to_string()), true)
            .expect("override set: proceeds");
        assert_eq!(plan.total_blocks, 3);
        // The override never bypasses a known shortage.
        assert!(check_kv_memory(10, Ok(1), true).is_err());
    }

    #[test]
    fn unparsable_meminfo_refuses_the_pool() {
        for text in [
            "MemTotal: 1 kB\n",
            "MemAvailable: x kB\n",
            "MemAvailable: 5\n",
            "",
        ] {
            let cfg = tiny_config();
            let err =
                plan_checked_model_kv(&cfg, None, &|| parse_mem_available_checked(text), false)
                    .unwrap_err();
            assert!(err.contains("no parsable"), "{text:?}: {err}");
        }
    }
    #[test]
    fn daemon_plan_refuses_before_allocating_and_sizes_the_manager() {
        // Exercises the planning, memory check and pooled manager without building a
        // spine: a spine creates a RuntimeController, which reads the process-wide
        // AIEN_RUNTIME_STATE_DIR that other tests in this crate point at temp dirs.
        let cfg = tiny_config();
        let err = plan_checked_model_kv(&cfg, None, &|| Ok(1), false).unwrap_err();
        assert!(err.contains("refusing"), "{err}");

        let plan =
            plan_checked_model_kv(&cfg, None, &|| Ok(1 << 30), false).expect("enough memory");
        // ceil(40 / 16) = 3 blocks of 16 tokens * (2 * 2 * 2 * 16 * 4) bytes per token.
        assert_eq!(plan.total_blocks, 3);
        assert_eq!(plan.total_bytes, 3 * 16 * 512);

        let weights = TransformerWeights::reference_test_weights(&cfg);
        let mgr = build_model_kv_manager(&weights, plan.total_blocks).expect("pool built");
        assert_eq!(mgr.read().total_block_count(), 3);
        assert_eq!(plan.pool_config().total_bytes(), plan.total_bytes);
    }

    #[test]
    fn kv_context_cap_parse_is_strict() {
        assert_eq!(parse_kv_context_cap("8192"), Ok(8192));
        assert_eq!(parse_kv_context_cap("1"), Ok(1));
        for bad in [
            "",
            "0",
            "00",
            "-1",
            "+5",
            " 8",
            "8 ",
            "8k",
            "1e3",
            "0x10",
            "99999999999999999999999",
        ] {
            let err = parse_kv_context_cap(bad).unwrap_err();
            assert!(err.contains(KV_CONTEXT_CAP_ENV), "{bad:?}: {err}");
        }
    }

    #[test]
    fn kv_context_cap_only_lowers_the_model_context() {
        let cfg = tiny_config(); // max_sequence_length 40, block 16
        assert_eq!(
            KvPoolPlan::for_model_capped(&cfg, None).unwrap(),
            KvPoolPlan::for_model(&cfg).unwrap()
        );
        let capped = KvPoolPlan::for_model_capped(&cfg, Some(17)).unwrap();
        assert_eq!((capped.context_tokens, capped.total_blocks), (17, 2));
        assert!(capped.context_source.contains(KV_CONTEXT_CAP_ENV));
        assert_eq!(
            KvPoolPlan::for_model_capped(&cfg, Some(40))
                .unwrap()
                .total_blocks,
            3
        );
        let err = KvPoolPlan::for_model_capped(&cfg, Some(41)).unwrap_err();
        assert!(err.contains("can only lower"), "{err}");
    }

    #[test]
    fn kv_context_cap_lets_a_long_context_model_fit_and_sizes_the_real_pool() {
        let cfg = tiny_config();
        let uncapped = KvPoolPlan::for_model(&cfg).unwrap().total_bytes as u64;
        let capped = KvPoolPlan::for_model_capped(&cfg, Some(16))
            .unwrap()
            .total_bytes as u64;
        assert!(capped < uncapped);
        // Memory between the two: the model's own context is refused, the cap fits.
        let avail = capped;
        assert!(plan_checked_model_kv(&cfg, None, &|| Ok(avail), false).is_err());
        let plan = plan_checked_model_kv(&cfg, Some(16), &|| Ok(avail), false).unwrap();
        assert_eq!(plan.total_blocks, 1);
        // The daemon path builds exactly the capped pool.
        let weights = TransformerWeights::reference_test_weights(&cfg);
        let (spine, _backend, plan) = build_shared_kv_runtime_for_model(
            weights,
            Arc::new(aien_inference_abi::ReferenceCpuBackend::new()),
            SchedulerConfig::default(),
            8,
            Some(17),
            &|| Ok(1 << 30),
            false,
        )
        .expect("capped runtime");
        assert_eq!(plan.total_blocks, 2);
        assert_eq!(spine.kv_manager.read().total_block_count(), 2);
    }
}

//! Qwen3 dense (Qwen3-8B family, bf16) on AIEN's own GPU engine (Omega), OWN-8B Stage 1.
//!
//! Design: `~/handoffs/2026-10-15-own8b-design.md` (section 2.2, stage table row 1). This stage
//! is host only: no launch, no chip. It is the dense sibling of `qwen3_moe_omega.rs`, which it
//! reuses (shard reader, peak RSS probe, matmul limits) but does not edit.
//!
//! - [`Qwen3DenseConfig`]: the shape, read from `config.json` through the strict Qwen3 reader of
//!   `model_dir.rs`, then held to what the dense Omega engine can run: model_type `qwen3` (the
//!   MoE family is refused by name), untied embeddings, plain rope, head_dim 64 or 128, every
//!   projection inside the bf16 streaming GEMV rule (`k % 256 == 0`, `256 <= k <= 16384`) and
//!   inside the resident matmul limits used for prefill (`K <= 16384`, `N <= 262144`).
//! - The streaming loader: no f32 image of the model. Every matrix is read from the shards as raw
//!   bf16 words into one reused host buffer, in the checkpoint's own `[n][k]` row layout (what the
//!   bf16 GEMV takes, no transpose), and handed to a [`DenseUploader`]. q, k and v are read back to
//!   back, so the fused `[q_dim + 2*kv_dim][hidden]` tensor is the three checkpoint tensors stacked
//!   in that order with no copy; gate and up likewise give `[2*intermediate][hidden]`, gate rows
//!   first. The host keeps the bf16 embedding (widened per row at lookup; the design's section
//!   2.2 says f32, which would add 1.24 GB at the 8B for the same values, so bf16 as in the 30B
//!   engine and the design's section 0) and the f32 norms. Peak
//!   host memory is the embedding plus one buffer the size of the largest matrix (the lm_head):
//!   [`dense_loader_transient_bytes`].

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::model_dir::model_config_from_hf_json;
use crate::qwen3_moe::bf16_to_f32;
use crate::qwen3_moe_omega::{
    peak_rss_bytes, ShardSource, TensorSource, MAX_RESIDENT_K, MAX_RESIDENT_N,
};

// The loader reads little-endian bf16 words in place (see `read_bf16`).
const _: () = assert!(
    cfg!(target_endian = "little"),
    "the dense loader needs a little-endian host"
);

/// head_dim values omega's paged attention takes (`omega_gpu_attention_api.h`, `TOO_LARGE`).
pub const ATTN_HEAD_DIMS: [usize; 2] = [64, 128];
/// Most query heads one paged attention launch takes (`OMEGA_GPU_ATTN_MAX_Q_HEADS`).
pub const ATTN_MAX_Q_HEADS: usize = 64;

// ---------------------------------------------------------------------------
// Model shape, read from the checkpoint.
// ---------------------------------------------------------------------------

/// The five matrices the dense engine streams, each `[n][k]` bf16 as the checkpoint stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DenseTensor {
    /// `q_proj | k_proj | v_proj` rows: `[q_dim + 2*kv_dim][hidden]`.
    Qkv,
    /// `o_proj`: `[hidden][q_dim]`.
    O,
    /// `gate_proj | up_proj` rows: `[2*intermediate][hidden]`.
    GateUp,
    /// `down_proj`: `[hidden][intermediate]`.
    Down,
    /// `lm_head`: `[vocab][hidden]`.
    LmHead,
}

impl DenseTensor {
    pub fn name(self) -> &'static str {
        match self {
            Self::Qkv => "q|k|v",
            Self::O => "o",
            Self::GateUp => "gate|up",
            Self::Down => "down",
            Self::LmHead => "lm_head",
        }
    }
}

/// The dense Qwen3 shape, read from `config.json` and validated against what the dense Omega
/// engine can run.
#[derive(Debug, Clone, PartialEq)]
pub struct Qwen3DenseConfig {
    pub layers: usize,
    pub hidden: usize,
    pub q_heads: usize,
    pub kv_heads: usize,
    pub head_dim: usize,
    pub intermediate: usize,
    pub vocab: usize,
    pub rms_eps: f32,
    pub rope_theta: f32,
    pub max_position: usize,
    /// Ids generation stops on (`generation_config.json` eos list, else `config.json` eos).
    pub eos_token_ids: Vec<u32>,
}

impl Qwen3DenseConfig {
    /// `config.json` and, when present, `generation_config.json` of a checkpoint directory.
    pub fn from_checkpoint(dir: &Path) -> Result<Self, String> {
        let path = dir.join("config.json");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let gen_path = dir.join("generation_config.json");
        let gen = match std::fs::read_to_string(&gen_path) {
            Ok(t) => Some(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("{}: {e}", gen_path.display())),
        };
        Self::from_json(&text, gen.as_deref()).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_json(config_json: &str, generation_json: Option<&str>) -> Result<Self, String> {
        let raw: serde_json::Value =
            serde_json::from_str(config_json).map_err(|e| format!("config.json: {e}"))?;
        match raw.get("model_type").and_then(|m| m.as_str()) {
            Some("qwen3") => {}
            Some("qwen3_moe") => {
                return Err(
                    "config refused: model_type \"qwen3_moe\" is the mixture-of-experts \
                            family; this loader is for dense \"qwen3\" (the MoE engine is \
                            qwen3_moe_omega)"
                        .into(),
                )
            }
            other => {
                return Err(format!(
                    "config refused: model_type {other:?}, this loader is for dense \"qwen3\""
                ))
            }
        }
        // A checkpoint that is not bf16 is refused here, not at its first tensor read.
        for key in ["torch_dtype", "dtype"] {
            if let Some(d) = raw.get(key).filter(|d| !d.is_null()) {
                if d.as_str() != Some("bfloat16") {
                    return Err(format!(
                        "config refused: {key} {d}, this loader streams bfloat16 weights"
                    ));
                }
            }
        }
        // Named before the general reader so the refusal says which field.
        if raw
            .get("tie_word_embeddings")
            .and_then(|t| t.as_bool())
            .unwrap_or(false)
        {
            return Err(
                "config refused: tie_word_embeddings=true is not implemented (the dense \
                        engine streams a separate lm_head)"
                    .into(),
            );
        }
        // The strict Qwen3ForCausalLM reader: unknown keys, sliding window, biases, activation,
        // rope type, head_dim presence and the attention geometry are refused there by name.
        let mc = model_config_from_hf_json("qwen3-dense", config_json, generation_json)
            .map_err(|e| format!("config refused: {e}"))?;
        if !mc.qk_norm {
            return Err(
                "config refused: architectures must be [\"Qwen3ForCausalLM\"] (q/k norm)".into(),
            );
        }
        if mc.rope_scaling.is_some() {
            return Err("config refused: rope_scaling is not implemented (plain rope only)".into());
        }
        let cfg = Self {
            layers: mc.num_layers,
            hidden: mc.hidden_dim,
            q_heads: mc.num_heads,
            kv_heads: mc.num_kv_heads,
            head_dim: mc.head_dim,
            intermediate: mc.intermediate_dim,
            vocab: mc.vocab_size,
            rms_eps: mc.rms_norm_eps,
            rope_theta: mc.rope_theta,
            max_position: mc.max_sequence_length,
            eos_token_ids: mc.eos_token_ids,
        };
        cfg.validate()?;
        Ok(cfg)
    }

    /// What the dense engine can run: omega's paged attention, the bf16 streaming GEMV for every
    /// projection (decode) and the resident matmul limits (prefill, `[k][n]`).
    pub fn validate(&self) -> Result<(), String> {
        let bad = |what: String| Err(format!("config refused: {what}"));
        if self.layers == 0
            || self.vocab == 0
            || self.q_heads == 0
            || self.kv_heads == 0
            || self.intermediate == 0
        {
            return bad("zero layers, vocab, heads or intermediate".into());
        }
        if !ATTN_HEAD_DIMS.contains(&self.head_dim) {
            return bad(format!(
                "head_dim {} (omega paged attention takes {ATTN_HEAD_DIMS:?})",
                self.head_dim
            ));
        }
        if !self.q_heads.is_multiple_of(self.kv_heads) || self.q_heads > ATTN_MAX_Q_HEADS {
            return bad(format!(
                "{} query heads over {} kv heads (need a whole GQA group and <= {ATTN_MAX_Q_HEADS} query heads)",
                self.q_heads, self.kv_heads
            ));
        }
        for (what, n, k) in self.matrices() {
            if let Err(e) = aien_omega_bf16::check_shape(n, k) {
                return bad(format!("{} [{n}][{k}]: {e}", what.name()));
            }
            if k > MAX_RESIDENT_K || n > MAX_RESIDENT_N {
                return bad(format!(
                    "{} [{n}][{k}] exceeds the resident matmul limit K <= {MAX_RESIDENT_K}, N <= {MAX_RESIDENT_N}",
                    what.name()
                ));
            }
        }
        Ok(())
    }

    pub fn q_dim(&self) -> usize {
        self.q_heads * self.head_dim
    }
    pub fn kv_dim(&self) -> usize {
        self.kv_heads * self.head_dim
    }
    /// Rows of the fused q|k|v tensor.
    pub fn qkv_n(&self) -> usize {
        self.q_dim() + 2 * self.kv_dim()
    }
    /// Rows of the fused gate|up tensor.
    pub fn gate_up_n(&self) -> usize {
        2 * self.intermediate
    }

    /// Every streamed matrix as `(what, n, k)`, `[n][k]` row layout.
    pub fn matrices(&self) -> [(DenseTensor, usize, usize); 5] {
        let h = self.hidden;
        [
            (DenseTensor::Qkv, self.qkv_n(), h),
            (DenseTensor::O, h, self.q_dim()),
            (DenseTensor::GateUp, self.gate_up_n(), h),
            (DenseTensor::Down, h, self.intermediate),
            (DenseTensor::LmHead, self.vocab, h),
        ]
    }

    /// bf16 words of the largest streamed matrix (the loader's one host buffer).
    pub fn max_matrix_words(&self) -> usize {
        self.matrices()
            .iter()
            .map(|&(_, n, k)| n * k)
            .max()
            .unwrap_or(0)
    }

    /// Bytes of one layer's four streamed bf16 tensors.
    pub fn layer_resident_bytes(&self) -> usize {
        self.matrices()
            .iter()
            .filter(|m| m.0 != DenseTensor::LmHead)
            .map(|&(_, n, k)| 2 * n * k)
            .sum()
    }

    /// Bytes resident on the device for the whole model (layers plus lm_head).
    pub fn model_resident_bytes(&self) -> usize {
        self.layers * self.layer_resident_bytes() + 2 * self.vocab * self.hidden
    }

    /// Bytes of the bf16 embedding the host keeps.
    pub fn embed_host_bytes(&self) -> usize {
        2 * self.vocab * self.hidden
    }
}

/// Largest host buffer the loader holds beyond the model's own host bytes: one bf16 buffer the
/// size of the largest matrix, reused for every read (norms included).
pub fn dense_loader_transient_bytes(config: &Qwen3DenseConfig) -> usize {
    2 * config.max_matrix_words()
}

// ---------------------------------------------------------------------------
// The device side, behind a trait so the loader runs without a chip.
// ---------------------------------------------------------------------------

/// One resident `[n][k]` bf16 matrix: `y[r] = sum_c W[r][c] * x[c]` in f32. Shared across
/// threads: an implementation over omega's `Bf16Tensor` (not `Sync`) holds it behind a lock, as
/// `OmegaLmHead` does for the 30B.
pub trait DenseGemv: Send + Sync + std::fmt::Debug {
    fn n(&self) -> usize;
    fn k(&self) -> usize;
    /// Returns the device time in ns (0 when not measured).
    fn gemv_f32(&self, x: &[f32], y: &mut [f32]) -> Result<u64, String>;
}

/// Makes `[n][k]` bf16 matrices resident, byte for byte as given (the checkpoint row layout).
/// The real one (omega's `Bf16Tensor`, NaN/Inf refused at upload) comes with the engine stage.
pub trait DenseUploader {
    /// `layer` is `None` for the lm_head.
    fn upload_bf16(
        &self,
        layer: Option<usize>,
        what: DenseTensor,
        n: usize,
        k: usize,
        bits: &[u16],
    ) -> Result<Arc<dyn DenseGemv>, String>;
}

// ---------------------------------------------------------------------------
// The loaded model.
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct DenseLayer {
    pub input_norm: Vec<f32>,
    pub post_norm: Vec<f32>,
    pub q_norm: Vec<f32>,
    pub k_norm: Vec<f32>,
    pub qkv: Arc<dyn DenseGemv>,
    pub o: Arc<dyn DenseGemv>,
    pub gate_up: Arc<dyn DenseGemv>,
    pub down: Arc<dyn DenseGemv>,
}

/// The whole model: streamed matrices on the device, the bf16 embedding and f32 norms on the host.
#[derive(Debug)]
pub struct Qwen3DenseOmegaWeights {
    pub config: Qwen3DenseConfig,
    /// `[vocab][hidden]` bf16 bits; a row is widened to f32 at lookup.
    pub embed_bf16: Vec<u16>,
    pub layers: Vec<DenseLayer>,
    pub final_norm: Vec<f32>,
    pub lm_head: Arc<dyn DenseGemv>,
}

impl Qwen3DenseOmegaWeights {
    /// The embedding row of `token` in f32; a token outside the vocabulary is refused.
    pub fn embed_row(&self, token: u32) -> Result<Vec<f32>, String> {
        let h = self.config.hidden;
        let t = token as usize;
        if t >= self.config.vocab {
            return Err(format!(
                "token {token} outside the vocabulary of {}",
                self.config.vocab
            ));
        }
        Ok(self.embed_bf16[t * h..(t + 1) * h]
            .iter()
            .map(|&b| bf16_to_f32(b))
            .collect())
    }

    /// Bytes the host keeps after the load (embedding and norms).
    pub fn host_bytes(&self) -> usize {
        self.embed_bf16.len() * 2
            + self.final_norm.len() * 4
            + self
                .layers
                .iter()
                .map(|l| {
                    4 * (l.input_norm.len() + l.post_norm.len() + l.q_norm.len() + l.k_norm.len())
                })
                .sum::<usize>()
    }
}

/// What a load measured.
#[derive(Debug, Clone, PartialEq)]
pub struct DenseLoadReport {
    /// bf16 bytes handed to the uploader.
    pub resident_bytes: usize,
    pub host_bytes: usize,
    /// The loader's one reused buffer, bytes ([`dense_loader_transient_bytes`]).
    pub transient_bytes: usize,
    pub uploads: usize,
    /// Process peak resident set (VmHWM) before and after the load, if `/proc` is readable.
    pub peak_rss_before_bytes: Option<u64>,
    pub peak_rss_bytes: Option<u64>,
    pub seconds: f64,
}

/// Reads one bf16 tensor (dtype and shape checked by the source) into `dst` as words.
fn read_bf16(
    source: &mut dyn TensorSource,
    name: &str,
    shape: &[usize],
    dst: &mut [u16],
) -> Result<(), String> {
    if shape.iter().product::<usize>() != dst.len() {
        return Err(format!(
            "{name}: shape {shape:?} does not fill {} words",
            dst.len()
        ));
    }
    // SAFETY: a u16 slice viewed as bytes of twice the length (u8 has no alignment need); the
    // source writes little-endian bf16 words, this host's byte order (asserted at compile time).
    let bytes =
        unsafe { std::slice::from_raw_parts_mut(dst.as_mut_ptr().cast::<u8>(), dst.len() * 2) };
    source.read_into(name, "BF16", shape, bytes)
}

/// A norm vector, read through the front of the loader's buffer and widened to f32.
fn read_norm(
    source: &mut dyn TensorSource,
    name: &str,
    len: usize,
    buf: &mut [u16],
) -> Result<Vec<f32>, String> {
    read_bf16(source, name, &[len], &mut buf[..len])?;
    Ok(buf[..len].iter().map(|&b| bf16_to_f32(b)).collect())
}

/// Loads a checkpoint directory: `config.json` for the shape, the shards for the weights.
pub fn load_qwen3_dense_omega(
    dir: &Path,
    uploader: &dyn DenseUploader,
    progress: &mut dyn FnMut(&str),
) -> Result<(Qwen3DenseOmegaWeights, DenseLoadReport), String> {
    let config = Qwen3DenseConfig::from_checkpoint(dir)?;
    let mut source = ShardSource::open(dir)?;
    load_qwen3_dense_omega_from(&config, &mut source, uploader, progress)
}

/// The streaming load from any [`TensorSource`].
pub fn load_qwen3_dense_omega_from(
    config: &Qwen3DenseConfig,
    source: &mut dyn TensorSource,
    uploader: &dyn DenseUploader,
    progress: &mut dyn FnMut(&str),
) -> Result<(Qwen3DenseOmegaWeights, DenseLoadReport), String> {
    config.validate()?;
    let t0 = Instant::now();
    let peak_before = peak_rss_bytes();
    let (h, i, hd) = (config.hidden, config.intermediate, config.head_dim);
    let (qd, kvd) = (config.q_dim(), config.kv_dim());

    let mut resident = 0usize;
    let mut uploads = 0usize;
    let mut upload = |layer: Option<usize>,
                      what: DenseTensor,
                      n: usize,
                      k: usize,
                      bits: &[u16]|
     -> Result<Arc<dyn DenseGemv>, String> {
        debug_assert_eq!(bits.len(), n * k);
        let at = layer.map_or("lm_head".to_string(), |l| format!("layer {l}"));
        let m = uploader
            .upload_bf16(layer, what, n, k, bits)
            .map_err(|e| format!("{at} {} [{n}][{k}] upload: {e}", what.name()))?;
        if m.n() != n || m.k() != k {
            return Err(format!(
                "{at} {} upload returned [{}][{}], asked [{n}][{k}]",
                what.name(),
                m.n(),
                m.k()
            ));
        }
        resident += 2 * n * k;
        uploads += 1;
        Ok(m)
    };

    // The one transient: every matrix and norm is read through it, layer after layer.
    let mut buf: Vec<u16> = vec![0; config.max_matrix_words()];
    let transient_bytes = buf.len() * 2;

    let mut embed_bf16 = vec![0u16; config.vocab * h];
    read_bf16(
        source,
        "model.embed_tokens.weight",
        &[config.vocab, h],
        &mut embed_bf16,
    )?;

    let mut layers = Vec::with_capacity(config.layers);
    for layer in 0..config.layers {
        let p = format!("model.layers.{layer}");
        let input_norm = read_norm(source, &format!("{p}.input_layernorm.weight"), h, &mut buf)?;
        let post_norm = read_norm(
            source,
            &format!("{p}.post_attention_layernorm.weight"),
            h,
            &mut buf,
        )?;
        let q_norm = read_norm(
            source,
            &format!("{p}.self_attn.q_norm.weight"),
            hd,
            &mut buf,
        )?;
        let k_norm = read_norm(
            source,
            &format!("{p}.self_attn.k_norm.weight"),
            hd,
            &mut buf,
        )?;

        // q|k|v: the three `[rows][hidden]` tensors back to back = the fused rows, no copy.
        let mut off = 0usize;
        for (name, rows) in [("q_proj", qd), ("k_proj", kvd), ("v_proj", kvd)] {
            read_bf16(
                source,
                &format!("{p}.self_attn.{name}.weight"),
                &[rows, h],
                &mut buf[off..off + rows * h],
            )?;
            off += rows * h;
        }
        let qkv = upload(
            Some(layer),
            DenseTensor::Qkv,
            config.qkv_n(),
            h,
            &buf[..off],
        )?;

        read_bf16(
            source,
            &format!("{p}.self_attn.o_proj.weight"),
            &[h, qd],
            &mut buf[..h * qd],
        )?;
        let o = upload(Some(layer), DenseTensor::O, h, qd, &buf[..h * qd])?;

        // gate|up: gate rows first, then up rows.
        let mut off = 0usize;
        for name in ["gate_proj", "up_proj"] {
            read_bf16(
                source,
                &format!("{p}.mlp.{name}.weight"),
                &[i, h],
                &mut buf[off..off + i * h],
            )?;
            off += i * h;
        }
        let gate_up = upload(Some(layer), DenseTensor::GateUp, 2 * i, h, &buf[..off])?;

        read_bf16(
            source,
            &format!("{p}.mlp.down_proj.weight"),
            &[h, i],
            &mut buf[..h * i],
        )?;
        let down = upload(Some(layer), DenseTensor::Down, h, i, &buf[..h * i])?;

        layers.push(DenseLayer {
            input_norm,
            post_norm,
            q_norm,
            k_norm,
            qkv,
            o,
            gate_up,
            down,
        });
        progress(&format!("layer {}/{} uploaded", layer + 1, config.layers));
    }

    let final_norm = read_norm(source, "model.norm.weight", h, &mut buf)?;
    let lm_words = config.vocab * h;
    read_bf16(
        source,
        "lm_head.weight",
        &[config.vocab, h],
        &mut buf[..lm_words],
    )?;
    let lm_head = upload(None, DenseTensor::LmHead, config.vocab, h, &buf[..lm_words])?;
    progress("lm_head uploaded");
    drop(buf);

    let weights = Qwen3DenseOmegaWeights {
        config: config.clone(),
        embed_bf16,
        layers,
        final_norm,
        lm_head,
    };
    let report = DenseLoadReport {
        resident_bytes: resident,
        host_bytes: weights.host_bytes(),
        transient_bytes,
        uploads,
        peak_rss_before_bytes: peak_before,
        peak_rss_bytes: peak_rss_bytes(),
        seconds: t0.elapsed().as_secs_f64(),
    };
    Ok((weights, report))
}

// ---------------------------------------------------------------------------
// The tiny host-test dense model and the recording uploader.
// ---------------------------------------------------------------------------

#[cfg(any(test, feature = "host-test-engine"))]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod test_support {
    //! A tiny dense Qwen3 checkpoint (in memory or written as two shards) and a recording
    //! uploader with host GEMVs, so the loader runs end to end without a chip.
    use super::*;
    use crate::qwen3_moe::f32_to_bf16;
    use crate::qwen3_moe_omega::test_support::{prand, MemorySource};
    use std::sync::Mutex;

    /// 2 layers, hidden 256, 4 query heads over 2 kv heads of 128 (q_dim 512 differs from hidden,
    /// so a transposed o_proj fails), intermediate 512, vocab 64.
    pub fn tiny_config() -> Qwen3DenseConfig {
        Qwen3DenseConfig {
            layers: 2,
            hidden: 256,
            q_heads: 4,
            kv_heads: 2,
            head_dim: 128,
            intermediate: 512,
            vocab: 64,
            rms_eps: 1e-6,
            rope_theta: 1e6,
            max_position: 4096,
            eos_token_ids: vec![3],
        }
    }

    /// The Hugging Face `config.json` text of `c` (the keys Qwen3-8B's carries).
    pub fn config_json(c: &Qwen3DenseConfig) -> String {
        serde_json::json!({
            "architectures": ["Qwen3ForCausalLM"],
            "attention_bias": false,
            "attention_dropout": 0.0,
            "bos_token_id": 1,
            "eos_token_id": c.eos_token_ids,
            "head_dim": c.head_dim,
            "hidden_act": "silu",
            "hidden_size": c.hidden,
            "initializer_range": 0.02,
            "intermediate_size": c.intermediate,
            "max_position_embeddings": c.max_position,
            "max_window_layers": c.layers,
            "model_type": "qwen3",
            "num_attention_heads": c.q_heads,
            "num_hidden_layers": c.layers,
            "num_key_value_heads": c.kv_heads,
            "rms_norm_eps": c.rms_eps,
            "rope_scaling": null,
            "rope_theta": c.rope_theta,
            "sliding_window": null,
            "tie_word_embeddings": false,
            "torch_dtype": "bfloat16",
            "transformers_version": "4.51.0",
            "use_cache": true,
            "use_sliding_window": false,
            "vocab_size": c.vocab
        })
        .to_string()
    }

    /// Every tensor of the tiny checkpoint, BF16, each with its own seed so rows of different
    /// tensors differ: `(name, shape, little-endian bytes)` in checkpoint order.
    pub fn tensors(c: &Qwen3DenseConfig) -> Vec<(String, Vec<usize>, Vec<u8>)> {
        let mut out = Vec::new();
        let mut seed = 0u64;
        let mut put = |name: String, shape: Vec<usize>, scale: f32, bias: f32| {
            seed += 1;
            let n: usize = shape.iter().product();
            let bytes: Vec<u8> = (0..n)
                .flat_map(|i| f32_to_bf16(bias + prand(seed, i) * scale).to_le_bytes())
                .collect();
            out.push((name, shape, bytes));
        };
        let (h, i, hd) = (c.hidden, c.intermediate, c.head_dim);
        put(
            "model.embed_tokens.weight".into(),
            vec![c.vocab, h],
            1.0,
            0.0,
        );
        for l in 0..c.layers {
            let p = format!("model.layers.{l}");
            put(format!("{p}.input_layernorm.weight"), vec![h], 0.1, 1.0);
            put(
                format!("{p}.self_attn.q_proj.weight"),
                vec![c.q_dim(), h],
                0.05,
                0.0,
            );
            put(
                format!("{p}.self_attn.k_proj.weight"),
                vec![c.kv_dim(), h],
                0.05,
                0.0,
            );
            put(
                format!("{p}.self_attn.v_proj.weight"),
                vec![c.kv_dim(), h],
                0.05,
                0.0,
            );
            put(
                format!("{p}.self_attn.o_proj.weight"),
                vec![h, c.q_dim()],
                0.05,
                0.0,
            );
            put(format!("{p}.self_attn.q_norm.weight"), vec![hd], 0.1, 1.0);
            put(format!("{p}.self_attn.k_norm.weight"), vec![hd], 0.1, 1.0);
            put(
                format!("{p}.post_attention_layernorm.weight"),
                vec![h],
                0.1,
                1.0,
            );
            put(format!("{p}.mlp.gate_proj.weight"), vec![i, h], 0.05, 0.0);
            put(format!("{p}.mlp.up_proj.weight"), vec![i, h], 0.05, 0.0);
            put(format!("{p}.mlp.down_proj.weight"), vec![h, i], 0.05, 0.0);
        }
        put("model.norm.weight".into(), vec![h], 0.1, 1.0);
        put("lm_head.weight".into(), vec![c.vocab, h], 0.1, 0.0);
        out
    }

    pub fn memory_source(c: &Qwen3DenseConfig) -> MemorySource {
        let mut m = MemorySource::default();
        for (name, shape, bytes) in tensors(c) {
            m.tensors.insert(name, ("BF16".into(), shape, bytes));
        }
        m
    }

    /// Writes `config.json`, `generation_config.json`, two safetensors shards (layer 0 and the
    /// embedding in the first, the rest in the second) and `model.safetensors.index.json`.
    pub fn write_checkpoint(dir: &Path, c: &Qwen3DenseConfig) -> Result<(), String> {
        let io = |e: std::io::Error| format!("{}: {e}", dir.display());
        std::fs::create_dir_all(dir).map_err(io)?;
        std::fs::write(dir.join("config.json"), config_json(c)).map_err(io)?;
        std::fs::write(
            dir.join("generation_config.json"),
            serde_json::json!({ "eos_token_id": c.eos_token_ids }).to_string(),
        )
        .map_err(io)?;
        let all = tensors(c);
        let split = all
            .iter()
            .position(|(n, _, _)| n.starts_with("model.layers.1."))
            .unwrap_or(all.len());
        let mut weight_map = serde_json::Map::new();
        for (index, part) in [&all[..split], &all[split..]].into_iter().enumerate() {
            let filename = format!("model-0000{}-of-00002.safetensors", index + 1);
            let mut header = serde_json::Map::new();
            let mut payload = Vec::new();
            for (name, shape, bytes) in part {
                header.insert(
                    name.clone(),
                    serde_json::json!({"dtype": "BF16", "shape": shape,
                        "data_offsets": [payload.len(), payload.len() + bytes.len()]}),
                );
                payload.extend_from_slice(bytes);
                weight_map.insert(name.clone(), serde_json::json!(filename));
            }
            let header = serde_json::to_vec(&header).map_err(|e| e.to_string())?;
            let mut file = (header.len() as u64).to_le_bytes().to_vec();
            file.extend_from_slice(&header);
            file.extend_from_slice(&payload);
            std::fs::write(dir.join(&filename), file).map_err(io)?;
        }
        std::fs::write(
            dir.join("model.safetensors.index.json"),
            serde_json::json!({ "metadata": {}, "weight_map": weight_map }).to_string(),
        )
        .map_err(io)
    }

    /// An `[n][k]` bf16 matrix on the host; GEMV in f32, sequential order.
    #[derive(Debug)]
    pub struct HostGemv {
        pub n: usize,
        pub k: usize,
        pub bits: Vec<u16>,
    }

    impl DenseGemv for HostGemv {
        fn n(&self) -> usize {
            self.n
        }
        fn k(&self) -> usize {
            self.k
        }
        fn gemv_f32(&self, x: &[f32], y: &mut [f32]) -> Result<u64, String> {
            if x.len() != self.k || y.len() != self.n {
                return Err(format!(
                    "gemv [{}][{}] given x {} y {}",
                    self.n,
                    self.k,
                    x.len(),
                    y.len()
                ));
            }
            for (r, out) in y.iter_mut().enumerate() {
                let row = &self.bits[r * self.k..(r + 1) * self.k];
                *out = row.iter().zip(x).map(|(&w, &v)| bf16_to_f32(w) * v).sum();
            }
            Ok(0)
        }
    }

    /// What a record-only upload hands back: the shape, no weights.
    #[derive(Debug)]
    pub struct RecordedShape {
        pub n: usize,
        pub k: usize,
    }

    impl DenseGemv for RecordedShape {
        fn n(&self) -> usize {
            self.n
        }
        fn k(&self) -> usize {
            self.k
        }
        fn gemv_f32(&self, _: &[f32], _: &mut [f32]) -> Result<u64, String> {
            Err("record-only upload keeps no weights".into())
        }
    }

    /// A deliberately wrong uploader, for the negative control.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum UploadMutant {
        /// Stores q|k|v with the k rows and the v rows exchanged.
        SwapKv { q_rows: usize, kv_rows: usize },
        /// Stores gate|up with the gate rows and the up rows exchanged.
        SwapGateUp { rows: usize },
    }

    /// One upload as the recording fake saw it.
    #[derive(Debug)]
    pub struct UploadRecord {
        pub layer: Option<usize>,
        pub what: DenseTensor,
        pub n: usize,
        pub k: usize,
        pub bytes: usize,
        /// The stored matrix (`keep` uploaders only).
        pub matrix: Option<Arc<HostGemv>>,
    }

    /// Records every upload; with `keep` it also stores the matrix for host GEMVs and the byte
    /// test. Like omega's `Bf16Tensor::upload` it refuses a length that is not `n * k` and a NaN
    /// or infinite weight, so every word is looked at.
    #[derive(Debug, Default)]
    pub struct RecordingDenseUploader {
        pub keep: bool,
        pub mutant: Option<UploadMutant>,
        pub records: Mutex<Vec<UploadRecord>>,
    }

    impl RecordingDenseUploader {
        pub fn keeping() -> Self {
            Self {
                keep: true,
                ..Self::default()
            }
        }
        /// Shapes and byte counts only (the real-checkpoint run: no host copy of the model).
        pub fn record_only() -> Self {
            Self::default()
        }
        pub fn swapping_kv(c: &Qwen3DenseConfig) -> Self {
            Self {
                keep: true,
                mutant: Some(UploadMutant::SwapKv {
                    q_rows: c.q_dim(),
                    kv_rows: c.kv_dim(),
                }),
                ..Self::default()
            }
        }
        pub fn swapping_gate_up(c: &Qwen3DenseConfig) -> Self {
            Self {
                keep: true,
                mutant: Some(UploadMutant::SwapGateUp {
                    rows: c.intermediate,
                }),
                ..Self::default()
            }
        }
    }

    impl DenseUploader for RecordingDenseUploader {
        fn upload_bf16(
            &self,
            layer: Option<usize>,
            what: DenseTensor,
            n: usize,
            k: usize,
            bits: &[u16],
        ) -> Result<Arc<dyn DenseGemv>, String> {
            if bits.len() != n * k {
                return Err(format!("{} words for [{n}][{k}]", bits.len()));
            }
            // bf16 exponent all ones = Inf or NaN.
            if let Some(at) = bits.iter().position(|&b| b & 0x7f80 == 0x7f80) {
                return Err(format!("word {at} is NaN or infinite (refused at upload)"));
            }
            let matrix = self.keep.then(|| {
                let mut stored = bits.to_vec();
                match (self.mutant, what) {
                    (Some(UploadMutant::SwapKv { q_rows, kv_rows }), DenseTensor::Qkv) => {
                        let (kb, vb) = stored[q_rows * k..].split_at_mut(kv_rows * k);
                        kb.swap_with_slice(&mut vb[..kv_rows * k]);
                    }
                    (Some(UploadMutant::SwapGateUp { rows }), DenseTensor::GateUp) => {
                        let (gb, ub) = stored.split_at_mut(rows * k);
                        gb.swap_with_slice(ub);
                    }
                    _ => {}
                }
                Arc::new(HostGemv { n, k, bits: stored })
            });
            self.records.lock().unwrap().push(UploadRecord {
                layer,
                what,
                n,
                k,
                bytes: bits.len() * 2,
                matrix: matrix.clone(),
            });
            Ok(match matrix {
                Some(m) => m,
                None => Arc::new(RecordedShape { n, k }),
            })
        }
    }

    /// The byte test: the uploads must be, in order, each layer's q|k|v, o, gate|up, down, then
    /// the lm_head, with the right shapes, and every stored row must equal the checkpoint row it
    /// stands for (read again, independently, from `source`). The first wrong row is named.
    pub fn check_upload_bytes(
        c: &Qwen3DenseConfig,
        source: &mut dyn TensorSource,
        records: &[UploadRecord],
    ) -> Result<(), String> {
        let (h, i, qd, kvd) = (c.hidden, c.intermediate, c.q_dim(), c.kv_dim());
        // (tensor name, rows, cols) of each checkpoint part, in row order.
        type Part = (String, usize, usize);
        type Expected = (Option<usize>, DenseTensor, usize, usize, Vec<Part>);
        let mut expected: Vec<Expected> = Vec::new();
        for l in 0..c.layers {
            let p = format!("model.layers.{l}");
            let a = |n: &str| format!("{p}.self_attn.{n}.weight");
            let m = |n: &str| format!("{p}.mlp.{n}.weight");
            expected.push((
                Some(l),
                DenseTensor::Qkv,
                c.qkv_n(),
                h,
                vec![
                    (a("q_proj"), qd, h),
                    (a("k_proj"), kvd, h),
                    (a("v_proj"), kvd, h),
                ],
            ));
            expected.push((Some(l), DenseTensor::O, h, qd, vec![(a("o_proj"), h, qd)]));
            expected.push((
                Some(l),
                DenseTensor::GateUp,
                2 * i,
                h,
                vec![(m("gate_proj"), i, h), (m("up_proj"), i, h)],
            ));
            expected.push((
                Some(l),
                DenseTensor::Down,
                h,
                i,
                vec![(m("down_proj"), h, i)],
            ));
        }
        expected.push((
            None,
            DenseTensor::LmHead,
            c.vocab,
            h,
            vec![("lm_head.weight".into(), c.vocab, h)],
        ));
        if records.len() != expected.len() {
            return Err(format!(
                "{} uploads, expected {}",
                records.len(),
                expected.len()
            ));
        }
        for (rec, (layer, what, n, k, parts)) in records.iter().zip(expected) {
            let at = format!("upload {:?} {}", layer, what.name());
            if (rec.layer, rec.what, rec.n, rec.k, rec.bytes) != (layer, what, n, k, 2 * n * k) {
                return Err(format!(
                    "{at}: got {:?} {} [{}][{}] {} bytes, expected [{n}][{k}] {} bytes",
                    rec.layer,
                    rec.what.name(),
                    rec.n,
                    rec.k,
                    rec.bytes,
                    2 * n * k
                ));
            }
            let m = rec
                .matrix
                .as_ref()
                .ok_or_else(|| format!("{at}: not kept"))?;
            let mut row0 = 0usize;
            for (name, rows, cols) in parts {
                let mut want = vec![0u16; rows * cols];
                read_bf16(source, &name, &[rows, cols], &mut want)?;
                for r in 0..rows {
                    let got = &m.bits[(row0 + r) * k..(row0 + r + 1) * k];
                    if got != &want[r * cols..(r + 1) * cols] {
                        return Err(format!(
                            "{at}: row {} is not {name} row {r} (rows {row0}..{} must be {name})",
                            row0 + r,
                            row0 + rows
                        ));
                    }
                }
                row0 += rows;
            }
        }
        Ok(())
    }
}

/// Host-only test model (feature `host-test-engine`): the tiny dense checkpoint and the recording
/// uploader, so other crates can load a dense model without a chip. Never in a server build.
#[cfg(feature = "host-test-engine")]
pub mod host_test_model {
    use super::test_support::{memory_source, tiny_config, write_checkpoint};
    /// The recording uploader (`keeping()` for host GEMVs), to load the written checkpoint
    /// with [`load_qwen3_dense_omega`].
    pub use super::test_support::{HostGemv, RecordingDenseUploader, UploadRecord};
    use super::*;

    /// 2 layers, hidden 256, 4 query heads over 2 kv heads of 128 (q_dim 512 differs from hidden,
    /// so a transposed o_proj fails), intermediate 512, vocab 64.
    pub fn config() -> Qwen3DenseConfig {
        tiny_config()
    }

    /// Writes the tiny checkpoint (config, generation config, two shards, index) into `dir`.
    pub fn write(dir: &Path) -> Result<(), String> {
        write_checkpoint(dir, &tiny_config())
    }

    /// The tiny model loaded from memory, every matrix kept on the host for host GEMVs.
    pub fn load() -> Result<(Qwen3DenseOmegaWeights, DenseLoadReport), String> {
        let c = tiny_config();
        load_qwen3_dense_omega_from(
            &c,
            &mut memory_source(&c),
            &RecordingDenseUploader::keeping(),
            &mut |_| {},
        )
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::qwen3_moe::f32_to_bf16;

    /// `~/models/qwen3-8b/config.json`, verbatim.
    const QWEN3_8B_CONFIG: &str = r#"{
  "architectures": ["Qwen3ForCausalLM"], "attention_bias": false, "attention_dropout": 0.0,
  "bos_token_id": 151643, "eos_token_id": 151645, "head_dim": 128, "hidden_act": "silu",
  "hidden_size": 4096, "initializer_range": 0.02, "intermediate_size": 12288,
  "max_position_embeddings": 40960, "max_window_layers": 36, "model_type": "qwen3",
  "num_attention_heads": 32, "num_hidden_layers": 36, "num_key_value_heads": 8,
  "rms_norm_eps": 1e-06, "rope_scaling": null, "rope_theta": 1000000, "sliding_window": null,
  "tie_word_embeddings": false, "torch_dtype": "bfloat16", "transformers_version": "4.51.0",
  "use_cache": true, "use_sliding_window": false, "vocab_size": 151936
}"#;
    const QWEN3_8B_GENERATION: &str = r#"{"eos_token_id": [151645, 151643]}"#;

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("aien-own8b-{name}-{}", std::process::id()))
    }

    fn load_tiny(
        up: &RecordingDenseUploader,
    ) -> Result<(Qwen3DenseOmegaWeights, DenseLoadReport), String> {
        let c = tiny_config();
        load_qwen3_dense_omega_from(&c, &mut memory_source(&c), up, &mut |_| {})
    }

    /// The 8B config with one field replaced (`null` removes it).
    fn variant(key: &str, value: serde_json::Value) -> Result<Qwen3DenseConfig, String> {
        let mut v: serde_json::Value = serde_json::from_str(QWEN3_8B_CONFIG).unwrap();
        if value.is_null() {
            v.as_object_mut().unwrap().remove(key);
        } else {
            v[key] = value;
        }
        Qwen3DenseConfig::from_json(&v.to_string(), Some(QWEN3_8B_GENERATION))
    }

    fn refused(r: Result<Qwen3DenseConfig, String>, needle: &str) {
        let e = r.expect_err("config must be refused");
        assert!(e.contains(needle), "refusal {e:?} does not name {needle:?}");
    }

    #[test]
    fn reads_the_qwen3_8b_config() {
        let c = Qwen3DenseConfig::from_json(QWEN3_8B_CONFIG, Some(QWEN3_8B_GENERATION)).unwrap();
        assert_eq!(
            (
                c.layers,
                c.hidden,
                c.q_heads,
                c.kv_heads,
                c.head_dim,
                c.intermediate,
                c.vocab
            ),
            (36, 4096, 32, 8, 128, 12288, 151936)
        );
        assert_eq!((c.rms_eps, c.rope_theta), (1e-6, 1e6));
        assert_eq!(c.eos_token_ids, vec![151645, 151643]);
        assert_eq!((c.qkv_n(), c.gate_up_n()), (6144, 24576));
        // Design section 0: 7,568,097,280 weights streamed per token (lm_head in, embedding out).
        assert_eq!(c.model_resident_bytes(), 2 * 7_568_097_280);
        assert_eq!(c.layer_resident_bytes(), 2 * 192_937_984);
        let real = std::path::Path::new(env!("HOME")).join("models/qwen3-8b");
        if real.join("config.json").exists() {
            assert_eq!(Qwen3DenseConfig::from_checkpoint(&real).unwrap(), c);
        }
    }

    #[test]
    fn refuses_qwen3_moe() {
        let moe = r#"{"architectures": ["Qwen3MoeForCausalLM"], "model_type": "qwen3_moe",
            "hidden_size": 2048, "num_experts": 128, "tie_word_embeddings": false}"#;
        refused(Qwen3DenseConfig::from_json(moe, None), "qwen3_moe");
        refused(variant("model_type", "qwen3_moe".into()), "qwen3_moe");
        refused(variant("model_type", "llama".into()), "model_type");
    }

    #[test]
    fn refuses_tied_embeddings() {
        refused(
            variant("tie_word_embeddings", true.into()),
            "tie_word_embeddings",
        );
    }

    #[test]
    fn refuses_k_not_a_multiple_of_256() {
        // hidden 4224 = 16.5 * 256: q|k|v, gate|up and lm_head have k = 4224.
        refused(variant("hidden_size", 4224.into()), "k a multiple of 256");
        // intermediate 12416: down has k = 12416.
        refused(
            variant("intermediate_size", 12416.into()),
            "down [4096][12416]",
        );
    }

    #[test]
    fn refuses_k_past_the_gemv_and_matmul_limits() {
        // down k = 16640 > 16384.
        refused(
            variant("intermediate_size", 16640.into()),
            "down [4096][16640]",
        );
        // lm_head n = 262400 > the resident matmul N limit 262144 (the GEMV itself would take it).
        refused(
            variant("vocab_size", 262400.into()),
            "resident matmul limit",
        );
    }

    #[test]
    fn refuses_attention_shapes_omega_cannot_run() {
        refused(variant("head_dim", 96.into()), "head_dim 96");
        refused(
            variant("num_key_value_heads", 5.into()),
            "QueryHeadsNotDivisible",
        );
        let mut c = tiny_config();
        c.kv_heads = 3;
        refused(c.validate().map(|_| c.clone()), "GQA");
        refused(variant("num_attention_heads", 128.into()), "GQA");
        refused(variant("head_dim", serde_json::Value::Null), "head_dim");
    }

    #[test]
    fn refuses_what_the_engine_does_not_implement() {
        refused(
            variant(
                "rope_scaling",
                serde_json::json!({"rope_type": "yarn", "factor": 4.0}),
            ),
            "rope_scaling",
        );
        refused(
            variant(
                "rope_scaling",
                serde_json::json!({"rope_type": "llama3", "factor": 8.0, "low_freq_factor": 1.0,
                    "high_freq_factor": 4.0, "original_max_position_embeddings": 8192}),
            ),
            "rope_scaling is not implemented",
        );
        refused(variant("use_sliding_window", true.into()), "sliding_window");
        refused(variant("attention_bias", true.into()), "bias");
        refused(
            variant("architectures", serde_json::json!(["LlamaForCausalLM"])),
            "Qwen3ForCausalLM",
        );
        refused(variant("num_experts", 128.into()), "num_experts");
        refused(variant("torch_dtype", "float16".into()), "torch_dtype");
        refused(variant("dtype", "float32".into()), "dtype");
    }

    #[test]
    fn tiny_load_shapes_byte_counts_and_order() {
        let c = tiny_config();
        let up = RecordingDenseUploader::keeping();
        let (w, report) = load_tiny(&up).unwrap();
        let recs = up.records.lock().unwrap();
        let got: Vec<_> = recs
            .iter()
            .map(|r| (r.layer, r.what, r.n, r.k, r.bytes))
            .collect();
        let mut want = Vec::new();
        for l in 0..c.layers {
            want.push((Some(l), DenseTensor::Qkv, 1024, 256, 2 * 1024 * 256));
            want.push((Some(l), DenseTensor::O, 256, 512, 2 * 256 * 512));
            want.push((Some(l), DenseTensor::GateUp, 1024, 256, 2 * 1024 * 256));
            want.push((Some(l), DenseTensor::Down, 256, 512, 2 * 256 * 512));
        }
        want.push((None, DenseTensor::LmHead, 64, 256, 2 * 64 * 256));
        assert_eq!(got, want);
        assert_eq!(report.uploads, want.len());
        assert_eq!(report.resident_bytes, c.model_resident_bytes());
        assert_eq!(
            report.resident_bytes,
            want.iter().map(|w| w.4).sum::<usize>()
        );
        assert_eq!(w.embed_bf16.len(), c.vocab * c.hidden);
        assert_eq!(
            report.host_bytes,
            c.embed_host_bytes() + 4 * (c.hidden + c.layers * (2 * c.hidden + 2 * c.head_dim))
        );
        for l in &w.layers {
            assert_eq!((l.qkv.n(), l.qkv.k()), (c.qkv_n(), c.hidden));
            assert_eq!((l.gate_up.n(), l.gate_up.k()), (c.gate_up_n(), c.hidden));
            assert_eq!((l.q_norm.len(), l.k_norm.len()), (c.head_dim, c.head_dim));
        }
        assert!(w.embed_row(63).is_ok());
        assert!(
            w.embed_row(64).is_err(),
            "out-of-vocabulary token refused, not wrapped"
        );
    }

    #[test]
    fn byte_test_passes_and_concat_order_is_q_k_v_and_gate_up() {
        let c = tiny_config();
        let up = RecordingDenseUploader::keeping();
        load_tiny(&up).unwrap();
        let recs = up.records.lock().unwrap();
        check_upload_bytes(&c, &mut memory_source(&c), &recs).unwrap();

        // Spot check by hand: first row of each block of the layer-1 fused tensors.
        let src = memory_source(&c);
        let row0 = |name: &str, k: usize| -> Vec<u16> {
            src.tensors[name].2[..2 * k]
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect()
        };
        let (h, qd, kvd, i) = (c.hidden, c.q_dim(), c.kv_dim(), c.intermediate);
        let qkv = recs[4].matrix.as_ref().unwrap();
        let row = |m: &HostGemv, r: usize| m.bits[r * m.k..(r + 1) * m.k].to_vec();
        assert_eq!(
            row(qkv, 0),
            row0("model.layers.1.self_attn.q_proj.weight", h)
        );
        assert_eq!(
            row(qkv, qd),
            row0("model.layers.1.self_attn.k_proj.weight", h)
        );
        assert_eq!(
            row(qkv, qd + kvd),
            row0("model.layers.1.self_attn.v_proj.weight", h)
        );
        let gu = recs[6].matrix.as_ref().unwrap();
        assert_eq!(row(gu, 0), row0("model.layers.1.mlp.gate_proj.weight", h));
        assert_eq!(row(gu, i), row0("model.layers.1.mlp.up_proj.weight", h));
    }

    /// Negative control: an uploader that stores q|k|v with the k and v rows exchanged must fail
    /// the byte test, naming the first wrong row.
    #[test]
    fn negative_control_kv_swap_fails_the_byte_test() {
        let c = tiny_config();
        let up = RecordingDenseUploader::swapping_kv(&c);
        load_tiny(&up).unwrap();
        let err = check_upload_bytes(&c, &mut memory_source(&c), &up.records.lock().unwrap())
            .expect_err("a k/v row swap must fail the byte test");
        assert!(
            err.contains(
                "upload Some(0) q|k|v: row 512 is not model.layers.0.self_attn.k_proj.weight row 0"
            ),
            "{err}"
        );
    }

    /// Negative control for the MLP concat: gate and up rows exchanged must fail the byte test.
    #[test]
    fn negative_control_gate_up_swap_fails_the_byte_test() {
        let c = tiny_config();
        let up = RecordingDenseUploader::swapping_gate_up(&c);
        load_tiny(&up).unwrap();
        let err = check_upload_bytes(&c, &mut memory_source(&c), &up.records.lock().unwrap())
            .expect_err("a gate/up row swap must fail the byte test");
        assert!(
            err.contains(
                "upload Some(0) gate|up: row 0 is not model.layers.0.mlp.gate_proj.weight row 0"
            ),
            "{err}"
        );
    }

    #[test]
    fn nan_and_missing_tensors_are_refused() {
        let c = tiny_config();
        let mut src = memory_source(&c);
        let name = "model.layers.1.mlp.up_proj.weight";
        let bytes = &mut src.tensors.get_mut(name).unwrap().2;
        bytes[10..12].copy_from_slice(&f32_to_bf16(f32::NAN).to_le_bytes());
        let e = load_qwen3_dense_omega_from(
            &c,
            &mut src,
            &RecordingDenseUploader::keeping(),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(e.contains("layer 1 gate|up") && e.contains("NaN"), "{e}");

        let mut src = memory_source(&c);
        src.tensors.remove("model.layers.1.self_attn.v_proj.weight");
        let e = load_qwen3_dense_omega_from(
            &c,
            &mut src,
            &RecordingDenseUploader::keeping(),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(e.contains("model.layers.1.self_attn.v_proj.weight"), "{e}");

        let mut src = memory_source(&c);
        let k = src
            .tensors
            .get_mut("model.layers.0.self_attn.k_proj.weight")
            .unwrap();
        k.1 = vec![c.kv_dim() / 2, 2 * c.hidden];
        let e = load_qwen3_dense_omega_from(
            &c,
            &mut src,
            &RecordingDenseUploader::keeping(),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(e.contains("k_proj"), "{e}");
    }

    /// Design gate: peak host memory under 2 GB over the embedding. The loader's one buffer is
    /// the largest matrix; at the 8B shape that is the lm_head, 1.24 GB.
    #[test]
    fn transient_is_one_buffer_under_2gb_at_the_8b_shape() {
        let c8 = Qwen3DenseConfig::from_json(QWEN3_8B_CONFIG, Some(QWEN3_8B_GENERATION)).unwrap();
        let t = dense_loader_transient_bytes(&c8);
        assert_eq!(t, 2 * 151936 * 4096);
        assert!(t < 2_000_000_000, "{t}");
        let (_, report) = load_tiny(&RecordingDenseUploader::keeping()).unwrap();
        assert_eq!(
            report.transient_bytes,
            dense_loader_transient_bytes(&tiny_config())
        );
        assert!(report.peak_rss_bytes.is_some_and(|b| b > 0));
    }

    /// Fills every read with a finite bf16 pattern (0x3c3c), so a full-width model can be loaded
    /// without a checkpoint on disk.
    struct PatternSource;

    impl TensorSource for PatternSource {
        fn read_into(
            &mut self,
            _: &str,
            dtype: &str,
            _: &[usize],
            dst: &mut [u8],
        ) -> Result<(), String> {
            assert_eq!(dtype, "BF16");
            dst.fill(0x3c);
            Ok(())
        }
    }

    /// Process peak RSS growth over one load, with the high-water mark reset first
    /// (`/proc/self/clear_refs` 5). `None` when `/proc` cannot do that.
    fn measured_peak_growth(c: &Qwen3DenseConfig, up: &RecordingDenseUploader) -> Option<u64> {
        std::fs::write("/proc/self/clear_refs", "5").ok()?;
        let before = crate::qwen3_moe_omega::current_rss_bytes()?;
        let (w, report) =
            load_qwen3_dense_omega_from(c, &mut PatternSource, up, &mut |_| {}).unwrap();
        drop(w);
        Some(report.peak_rss_bytes?.saturating_sub(before))
    }

    /// The memory gate measured, not just computed: one layer at the 8B widths with a 32k
    /// vocabulary (embedding 268 MB, model 654 MB) must raise the peak RSS by at most the
    /// embedding plus the one buffer plus 128 MB slack. Control: the same load through the
    /// keeping uploader (a full host copy of the model, as an f32 or bf16 image would be) must
    /// break the same bound, so the check cannot pass vacuously.
    #[test]
    fn measured_peak_rss_is_the_embedding_plus_one_buffer() {
        let mut c =
            Qwen3DenseConfig::from_json(QWEN3_8B_CONFIG, Some(QWEN3_8B_GENERATION)).unwrap();
        c.layers = 1;
        c.vocab = 32768;
        let bound = (c.embed_host_bytes() + dense_loader_transient_bytes(&c) + (128 << 20)) as u64;
        let Some(streaming) = measured_peak_growth(&c, &RecordingDenseUploader::record_only())
        else {
            println!("no /proc/self/clear_refs: measured peak RSS check skipped");
            return;
        };
        assert!(
            streaming <= bound,
            "streaming load grew peak RSS by {streaming}, bound {bound}"
        );
        let keeping = measured_peak_growth(&c, &RecordingDenseUploader::keeping()).unwrap();
        assert!(
            keeping > bound,
            "control: a full host copy grew peak RSS by only {keeping}, bound {bound}"
        );
    }

    /// The tiny model through the shard files: the loader's values equal
    /// `TransformerWeights::load_from_safetensors` (the f32 reference loader) bit for bit.
    #[test]
    fn loader_equals_transformer_weights_on_the_tiny_model() {
        let c = tiny_config();
        let dir = scratch("parity");
        let _ = std::fs::remove_dir_all(&dir);
        write_checkpoint(&dir, &c).unwrap();
        let up = RecordingDenseUploader::keeping();
        let (w, _) = load_qwen3_dense_omega(&dir, &up, &mut |_| {}).unwrap();
        assert_eq!(w.config, c);
        let mc = crate::model_dir::load_model_config(&dir).unwrap();
        let tw = crate::weights::TransformerWeights::load_from_safetensors(
            dir.join("model.safetensors.index.json"),
            &mc,
        )
        .unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        let wide = |m: &HostGemv| -> Vec<f32> { m.bits.iter().map(|&b| bf16_to_f32(b)).collect() };
        let cat = |parts: &[&[f32]]| -> Vec<f32> { parts.concat() };
        let bits = |v: &[f32]| -> Vec<u32> { v.iter().map(|x| x.to_bits()).collect() };
        let recs = up.records.lock().unwrap();
        let embed: Vec<f32> = w.embed_bf16.iter().map(|&b| bf16_to_f32(b)).collect();
        assert_eq!(bits(&embed), bits(&tw.embed_tokens));
        for (l, (dl, tl)) in w.layers.iter().zip(&tw.layers).enumerate() {
            let m = |j: usize| recs[4 * l + j].matrix.as_ref().unwrap();
            assert_eq!(
                bits(&wide(m(0))),
                bits(&cat(&[&tl.q_proj, &tl.k_proj, &tl.v_proj]))
            );
            assert_eq!(bits(&wide(m(1))), bits(&tl.o_proj));
            assert_eq!(bits(&wide(m(2))), bits(&cat(&[&tl.gate_proj, &tl.up_proj])));
            assert_eq!(bits(&wide(m(3))), bits(&tl.down_proj));
            assert_eq!(bits(&dl.input_norm), bits(&tl.input_layernorm));
            assert_eq!(bits(&dl.post_norm), bits(&tl.post_attention_layernorm));
            assert_eq!(bits(&dl.q_norm), bits(tl.q_norm.as_ref().unwrap()));
            assert_eq!(bits(&dl.k_norm), bits(tl.k_norm.as_ref().unwrap()));
        }
        assert_eq!(bits(&w.final_norm), bits(&tw.final_norm));
        let head = recs.last().unwrap().matrix.as_ref().unwrap();
        assert_eq!(bits(&wide(head)), bits(tw.lm_head.as_ref().unwrap()));

        // The fused GEMV gives the three separate projections' rows.
        let x: Vec<f32> = (0..c.hidden).map(prand_x).collect();
        let mut y = vec![0f32; c.qkv_n()];
        w.layers[0].qkv.gemv_f32(&x, &mut y).unwrap();
        let kv0 = c.q_dim();
        let k_row0: f32 = tw.layers[0].k_proj[..c.hidden]
            .iter()
            .zip(&x)
            .map(|(a, b)| a * b)
            .sum();
        assert_eq!(y[kv0].to_bits(), k_row0.to_bits());
    }

    fn prand_x(j: usize) -> f32 {
        crate::qwen3_moe_omega::test_support::prand(77, j)
    }

    /// The feature-gated door other crates use: the same tiny model from memory and from disk.
    #[cfg(feature = "host-test-engine")]
    #[test]
    fn host_test_model_loads_from_memory_and_from_disk() {
        let (mem, _) = super::host_test_model::load().unwrap();
        let dir = scratch("host-test-model");
        let _ = std::fs::remove_dir_all(&dir);
        super::host_test_model::write(&dir).unwrap();
        let (disk, _) =
            load_qwen3_dense_omega(&dir, &RecordingDenseUploader::keeping(), &mut |_| {}).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(mem.config, super::host_test_model::config());
        assert_eq!(disk.config, mem.config);
        assert_eq!(disk.embed_bf16, mem.embed_bf16);
        let x: Vec<f32> = (0..mem.config.hidden).map(prand_x).collect();
        let (mut a, mut b) = (vec![0f32; mem.config.vocab], vec![0f32; mem.config.vocab]);
        mem.lm_head.gemv_f32(&x, &mut a).unwrap();
        disk.lm_head.gemv_f32(&x, &mut b).unwrap();
        assert_eq!(a, b);
    }

    /// The real Qwen3-8B through the streaming loader and the record-only fake (host memory
    /// only, no chip): per-tensor bytes, total, peak RSS, wall time. Run with
    /// `cargo test -p aien-inference-abi --lib qwen3_dense_omega::tests::real_qwen3_8b -- --ignored --nocapture --test-threads=1`
    /// (`AIEN_QWEN3_8B_DIR` overrides `~/models/qwen3-8b`).
    #[test]
    #[ignore = "reads the 16 GB Qwen3-8B checkpoint"]
    fn real_qwen3_8b_streaming_load_host_only() {
        let dir = std::env::var_os("AIEN_QWEN3_8B_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::Path::new(env!("HOME")).join("models/qwen3-8b"));
        let rss_before = crate::qwen3_moe_omega::current_rss_bytes().unwrap();
        let up = RecordingDenseUploader::record_only();
        let (w, report) = load_qwen3_dense_omega(&dir, &up, &mut |_| {}).unwrap();
        let c = &w.config;
        let recs = up.records.lock().unwrap();
        let mut per: Vec<(DenseTensor, usize, usize, usize, usize)> = Vec::new();
        for r in recs.iter() {
            match per.iter_mut().find(|p| p.0 == r.what) {
                Some(p) => {
                    p.3 += 1;
                    p.4 += r.bytes;
                }
                None => per.push((r.what, r.n, r.k, 1, r.bytes)),
            }
        }
        for (what, n, k, count, bytes) in &per {
            println!(
                "OWN8B-LOAD tensor {:<8} [{n}][{k}] bytes_each={} count={count} bytes_total={bytes}",
                what.name(),
                2 * n * k
            );
        }
        let peak = report.peak_rss_bytes.unwrap();
        println!(
            "OWN8B-LOAD resident_bytes={} uploads={} host_bytes={} embed_bytes={} transient_bytes={} \
             rss_before={rss_before} peak_rss_before={} peak_rss={peak} peak_over_start={} seconds={:.2}",
            report.resident_bytes,
            report.uploads,
            report.host_bytes,
            c.embed_host_bytes(),
            report.transient_bytes,
            report.peak_rss_before_bytes.unwrap(),
            peak - rss_before,
            report.seconds
        );
        assert_eq!(report.resident_bytes, 2 * 7_568_097_280);
        assert_eq!(report.uploads, 4 * 36 + 1);
        assert!(
            (peak - rss_before) as usize <= c.embed_host_bytes() + 2_000_000_000,
            "peak RSS grew {} bytes, limit is the embedding plus 2 GB",
            peak - rss_before
        );
    }
}

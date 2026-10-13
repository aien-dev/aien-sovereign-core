//! Qwen3-MoE (Qwen3-30B-A3B family) on AIEN's own GPU engine (Omega), OWN-30B Stage A.
//!
//! Design: `~/handoffs/2026-10-13-own30b-design.md` (section 2, Stage A). No CUDA, no Mojo, no
//! Python: attention and every matmul run as Omega launches, routing / norms / rope / swiglu run
//! on the host for one row.
//!
//! Cut 1 (this file's first half): a general-model loader. The checkpoint path and the model
//! shape come from the caller and `config.json` (nothing is hard-coded to the Coder snapshot).
//! Each layer's FP8 block-128 weights are dequantised with the exact e4m3 conversion
//! (`fp8_e4m3fn_to_f32`) times the bf16 inverse scale, rounded to bf16 (round-to-nearest-even),
//! written straight into the `[k][n]` layout the engine wants, and uploaded as four resident
//! tensors per layer: fused q|k|v, o, every expert's gate|up side by side along N, and every
//! expert's down side by side along N. The host copy of a layer is reused for the next one, so
//! the peak host memory stays near one layer's transient plus the bf16 embedding.
//!
//! Second half: the Stage A forward (`Qwen3MoeOmegaEngine`): paged bf16 KV, host top-8 routing
//! with `norm_topk_prob`, expert windows through `omega_gpu_matmul_resident_windows_f32`,
//! sampling, prefill and decode, and a library entry (`load` / `generate` with a token callback)
//! for the HTTP server.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rayon::prelude::*;

use crate::qwen3_moe::{bf16_to_f32, f32_to_bf16, fp8_table, shard_path, Shard, FP8_BLOCK};
use crate::resident::transpose_bf16_for_upload;

/// Column tile of the engine's tile-major B layout: every expert window must start on one.
pub const WINDOW_TILE: usize = aien_omega_moe::TILE_N;
/// Engine limits for one resident tensor (`omega_blackwell_matmul.h` at omega.lock).
pub const MAX_RESIDENT_K: usize = 16384;
pub const MAX_RESIDENT_N: usize = 262144;

// ---------------------------------------------------------------------------
// Model shape, read from the checkpoint.
// ---------------------------------------------------------------------------

/// The Qwen3-MoE shape, read from `config.json` and validated against what Stage A can run.
#[derive(Debug, Clone, PartialEq)]
pub struct Qwen3MoeConfig {
    pub layers: usize,
    pub hidden: usize,
    pub q_heads: usize,
    pub kv_heads: usize,
    pub head_dim: usize,
    pub experts: usize,
    pub top_k: usize,
    pub moe_intermediate: usize,
    pub vocab: usize,
    pub rms_eps: f32,
    pub rope_theta: f32,
    pub norm_topk_prob: bool,
    pub tie_word_embeddings: bool,
    /// Ids generation stops on (`config.json` eos plus `generation_config.json` eos list).
    pub eos_token_ids: Vec<u32>,
}

impl Qwen3MoeConfig {
    pub fn from_checkpoint(dir: &Path) -> Result<Self, String> {
        let path = dir.join("config.json");
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut cfg = Self::from_json(&text)?;
        let gen_path = dir.join("generation_config.json");
        if let Ok(gen) = std::fs::read_to_string(&gen_path) {
            let v: serde_json::Value =
                serde_json::from_str(&gen).map_err(|e| format!("{}: {e}", gen_path.display()))?;
            for id in eos_ids(v.get("eos_token_id")) {
                if !cfg.eos_token_ids.contains(&id) {
                    cfg.eos_token_ids.push(id);
                }
            }
        }
        Ok(cfg)
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("config.json: {e}"))?;
        let model_type = v.get("model_type").and_then(|m| m.as_str()).unwrap_or("");
        if model_type != "qwen3_moe" {
            return Err(format!("config.json model_type {model_type:?}, this loader is for qwen3_moe"));
        }
        let num = |key: &str| -> Result<usize, String> {
            v.get(key)
                .and_then(|x| x.as_u64())
                .map(|x| x as usize)
                .ok_or_else(|| format!("config.json: missing integer {key}"))
        };
        let flt = |key: &str| -> Result<f32, String> {
            v.get(key)
                .and_then(|x| x.as_f64())
                .map(|x| x as f32)
                .ok_or_else(|| format!("config.json: missing number {key}"))
        };
        let cfg = Self {
            layers: num("num_hidden_layers")?,
            hidden: num("hidden_size")?,
            q_heads: num("num_attention_heads")?,
            kv_heads: num("num_key_value_heads")?,
            head_dim: num("head_dim")?,
            experts: num("num_experts")?,
            top_k: num("num_experts_per_tok")?,
            moe_intermediate: num("moe_intermediate_size")?,
            vocab: num("vocab_size")?,
            rms_eps: flt("rms_norm_eps")?,
            rope_theta: flt("rope_theta")?,
            norm_topk_prob: v.get("norm_topk_prob").and_then(|x| x.as_bool()).unwrap_or(false),
            tie_word_embeddings: v
                .get("tie_word_embeddings")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
            eos_token_ids: eos_ids(v.get("eos_token_id")),
        };
        if !v
            .get("mlp_only_layers")
            .and_then(|x| x.as_array())
            .is_none_or(|a| a.is_empty())
        {
            return Err("config.json: mlp_only_layers is not empty; every layer must be MoE".into());
        }
        if v.get("decoder_sparse_step").and_then(|x| x.as_u64()) != Some(1) {
            return Err("config.json: decoder_sparse_step must be 1 (every layer MoE)".into());
        }
        cfg.validate()?;
        Ok(cfg)
    }

    /// Stage A limits: block-128 quantisation, head_dim 128 attention, one resident tensor per
    /// projection within the engine's K and N limits, top-k within one window call.
    pub fn validate(&self) -> Result<(), String> {
        let bad = |what: &str| Err(format!("config refused: {what}"));
        if self.layers == 0 || self.experts == 0 || self.top_k == 0 || self.vocab == 0 {
            return bad("zero layers, experts, top_k or vocab");
        }
        if !self.hidden.is_multiple_of(FP8_BLOCK) || !self.moe_intermediate.is_multiple_of(FP8_BLOCK) {
            return bad("hidden and moe_intermediate must be multiples of 128 (FP8 block)");
        }
        if self.head_dim != 128 || !self.q_heads.is_multiple_of(self.kv_heads) || self.q_heads > 64 {
            return bad("attention must be head_dim 128, q_heads a multiple of kv_heads, q_heads <= 64");
        }
        if self.top_k > self.experts || self.top_k > aien_omega_moe::MAX_WINDOWS {
            return bad("top_k must be <= experts and <= one window call");
        }
        if !self.norm_topk_prob {
            return bad("norm_topk_prob=false is not implemented (Qwen3-MoE uses true)");
        }
        if self.tie_word_embeddings {
            return bad("tie_word_embeddings=true is not implemented");
        }
        for (what, k, n) in [
            ("q|k|v", self.hidden, self.qkv_n()),
            ("o", self.q_dim(), self.hidden),
            ("gate|up", self.hidden, self.gate_up_n()),
            ("down", self.moe_intermediate, self.down_n()),
            ("lm_head", self.hidden, self.vocab),
        ] {
            if k > MAX_RESIDENT_K || n > MAX_RESIDENT_N {
                return Err(format!(
                    "config refused: {what} tensor {k}x{n} exceeds the engine limit {MAX_RESIDENT_K}x{MAX_RESIDENT_N}"
                ));
            }
        }
        if !(2 * self.moe_intermediate).is_multiple_of(WINDOW_TILE) || !self.hidden.is_multiple_of(WINDOW_TILE) {
            return bad("expert windows must start on a column tile of 8");
        }
        Ok(())
    }

    pub fn q_dim(&self) -> usize {
        self.q_heads * self.head_dim
    }
    pub fn kv_dim(&self) -> usize {
        self.kv_heads * self.head_dim
    }
    /// Fused q|k|v output width.
    pub fn qkv_n(&self) -> usize {
        self.q_dim() + 2 * self.kv_dim()
    }
    /// Width of one expert's gate|up window.
    pub fn gate_up_cols(&self) -> usize {
        2 * self.moe_intermediate
    }
    /// All experts' gate|up side by side.
    pub fn gate_up_n(&self) -> usize {
        self.experts * self.gate_up_cols()
    }
    /// Width of one expert's down window.
    pub fn down_cols(&self) -> usize {
        self.hidden
    }
    pub fn down_n(&self) -> usize {
        self.experts * self.down_cols()
    }
    /// First column of expert `e` in the gate|up tensor (gate first, then up).
    pub fn gate_up_col0(&self, expert: usize) -> usize {
        expert * self.gate_up_cols()
    }
    pub fn down_col0(&self, expert: usize) -> usize {
        expert * self.down_cols()
    }

    /// Bytes of one layer's four resident bf16 tensors.
    pub fn layer_resident_bytes(&self) -> usize {
        2 * (self.hidden * self.qkv_n()
            + self.q_dim() * self.hidden
            + self.hidden * self.gate_up_n()
            + self.moe_intermediate * self.down_n())
    }
    /// Bytes resident for the whole model (layers plus lm_head).
    pub fn model_resident_bytes(&self) -> usize {
        self.layers * self.layer_resident_bytes() + 2 * self.hidden * self.vocab
    }
    /// Bytes the engine reads per generated token (attention + top_k experts + lm_head), bf16.
    pub fn bytes_per_token_bf16(&self) -> usize {
        2 * self.layers * (self.hidden * self.qkv_n() + self.q_dim() * self.hidden)
            + 2 * self.layers * self.top_k * (self.hidden * self.gate_up_cols() + self.moe_intermediate * self.down_cols())
            + 2 * self.hidden * self.vocab
    }
}

fn eos_ids(v: Option<&serde_json::Value>) -> Vec<u32> {
    match v {
        Some(serde_json::Value::Number(n)) => n.as_u64().map(|x| vec![x as u32]).unwrap_or_default(),
        Some(serde_json::Value::Array(a)) => a.iter().filter_map(|x| x.as_u64().map(|x| x as u32)).collect(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Where the tensors come from: the checkpoint shards, or a fake in tests.
// ---------------------------------------------------------------------------

/// Reads one raw tensor (dtype and shape checked) into `dst`.
pub trait TensorSource {
    fn read_into(&mut self, name: &str, dtype: &str, shape: &[usize], dst: &mut [u8]) -> Result<(), String>;
}

/// The safetensors shards behind `model.safetensors.index.json` (the same validated reader as
/// the MoE capsule loader: dtype, shape and byte range are checked per tensor).
pub struct ShardSource {
    dir: PathBuf,
    weight_map: serde_json::Map<String, serde_json::Value>,
    shards: HashMap<String, Shard>,
}

impl ShardSource {
    pub fn open(dir: &Path) -> Result<Self, String> {
        let index_path = dir.join("model.safetensors.index.json");
        let index: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&index_path).map_err(|e| format!("{}: {e}", index_path.display()))?,
        )
        .map_err(|e| format!("{}: {e}", index_path.display()))?;
        let weight_map = index
            .get("weight_map")
            .and_then(|v| v.as_object())
            .cloned()
            .ok_or_else(|| "index: missing weight_map object".to_string())?;
        Ok(Self {
            dir: dir.to_path_buf(),
            weight_map,
            shards: HashMap::new(),
        })
    }
}

impl TensorSource for ShardSource {
    fn read_into(&mut self, name: &str, dtype: &str, shape: &[usize], dst: &mut [u8]) -> Result<(), String> {
        let filename = self
            .weight_map
            .get(name)
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("index: no shard for {name}"))?;
        if !self.shards.contains_key(filename) {
            let path = shard_path(&self.dir, filename).map_err(|e| e.to_string())?;
            let shard = Shard::open(&path).map_err(|e| e.to_string())?;
            self.shards.insert(filename.to_string(), shard);
        }
        self.shards[filename]
            .read(name, dtype, shape, dst)
            .map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// FP8 block-128 -> bf16, written transposed into column windows.
// ---------------------------------------------------------------------------

/// One `[rows][k]` FP8 matrix with its `[ceil(rows/128)][k/128]` bf16 inverse scales, and the
/// column of the destination where its rows land (row `r` becomes column `col0 + r`).
pub struct Fp8Source<'a> {
    pub weights: &'a [u8],
    pub scales: &'a [u16],
    pub rows: usize,
    pub col0: usize,
}

/// Dequantises every source exactly (`fp8_e4m3fn_to_f32(w) * bf16(scale)`), rounds to bf16
/// (round-to-nearest-even, as the engine's own upload does) and writes `dst[k][col0 + r]`,
/// `dst` being `[k_dim][dst_n]` row-major: the transposed `[in][out]` layout a resident tensor
/// takes. The e4m3 NaN codes (0x7f, 0xff) are refused: a weight is never NaN, so one in the
/// file means a corrupt or foreign checkpoint.
pub fn dequant_fp8_sources_to_bf16_cols(
    sources: &[Fp8Source<'_>],
    k_dim: usize,
    dst: &mut [u16],
    dst_n: usize,
) -> Result<(), String> {
    if k_dim == 0 || !k_dim.is_multiple_of(FP8_BLOCK) {
        return Err(format!("k_dim {k_dim} must be a positive multiple of {FP8_BLOCK}"));
    }
    if dst.len() != k_dim * dst_n {
        return Err(format!("dst holds {} values, {k_dim}x{dst_n} expected", dst.len()));
    }
    let k_blocks = k_dim / FP8_BLOCK;
    for (i, s) in sources.iter().enumerate() {
        if s.weights.len() != s.rows * k_dim {
            return Err(format!("source {i}: {} weight bytes, {}x{k_dim} expected", s.weights.len(), s.rows));
        }
        let want = s.rows.div_ceil(FP8_BLOCK) * k_blocks;
        if s.scales.len() != want {
            return Err(format!("source {i}: {} scales, {want} expected", s.scales.len()));
        }
        if s.col0 + s.rows > dst_n {
            return Err(format!("source {i}: columns {}..{} past dst_n {dst_n}", s.col0, s.col0 + s.rows));
        }
    }
    let lut = fp8_table();
    let nan_seen = std::sync::atomic::AtomicBool::new(false);
    // Each task owns one 128-deep band of k (one scale block), all destination columns.
    dst.par_chunks_mut(dst_n * FP8_BLOCK)
        .enumerate()
        .for_each(|(kb, band)| {
            let k0 = kb * FP8_BLOCK;
            for s in sources {
                for r in 0..s.rows {
                    let scale = bf16_to_f32(s.scales[(r / FP8_BLOCK) * k_blocks + kb]);
                    let row = &s.weights[r * k_dim + k0..r * k_dim + k0 + FP8_BLOCK];
                    let col = s.col0 + r;
                    for (j, &w) in row.iter().enumerate() {
                        if w & 0x7f == 0x7f {
                            nan_seen.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        band[j * dst_n + col] = f32_to_bf16(lut[w as usize] * scale);
                    }
                }
            }
        });
    if nan_seen.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("FP8 weight holds an e4m3 NaN code (0x7f/0xff): refused".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The device side: resident matmuls, uploaded through a trait so tests need no chip.
// ---------------------------------------------------------------------------

/// A resident `k x n` weight used whole: `c[m x n] = a[m x k] * W`. Returns chip nanoseconds.
pub trait DenseMatmul: Send + Sync + std::fmt::Debug {
    fn k(&self) -> usize;
    fn n(&self) -> usize;
    fn matmul_f32(&self, m: usize, a: &[f32], c: &mut [f32]) -> Result<u64, String>;
}

/// A resident `k x n` tensor of experts side by side, used by column windows: for each window
/// `w`, `c[w][m x cols] = a[m x k] * W[:, col0s[w] .. col0s[w] + cols)`. Returns chip nanoseconds.
pub trait ExpertMatmul: Send + Sync + std::fmt::Debug {
    fn k(&self) -> usize;
    fn n(&self) -> usize;
    fn windows_f32(&self, m: usize, a: &[f32], col0s: &[u32], cols: usize, c: &mut [f32]) -> Result<u64, String>;
}

/// Makes `[k][n]` bf16 matrices resident. The real one is [`OmegaModelUploader`].
pub trait ModelUploader {
    fn upload_dense(&self, k: usize, n: usize, bits: &[u16]) -> Result<Arc<dyn DenseMatmul>, String>;
    fn upload_experts(&self, k: usize, n: usize, bits: &[u16]) -> Result<Arc<dyn ExpertMatmul>, String>;
}

/// Omega's whole-tensor resident matmul behind a lock.
#[derive(Debug)]
pub struct OmegaDense(Mutex<aien_omega_gpu::ResidentTensor>, usize, usize);

impl DenseMatmul for OmegaDense {
    fn k(&self) -> usize {
        self.1
    }
    fn n(&self) -> usize {
        self.2
    }
    fn matmul_f32(&self, m: usize, a: &[f32], c: &mut [f32]) -> Result<u64, String> {
        let t = self.0.lock().map_err(|_| "resident tensor lock poisoned".to_string())?;
        t.matmul_f32(m, a, c).map(|i| i.raw.elapsed_ns).map_err(|e| e.to_string())
    }
}

/// Omega's column-window matmul (cut 2) behind a lock.
#[derive(Debug)]
pub struct OmegaExperts(Mutex<aien_omega_moe::MoeTensor>, usize, usize);

impl ExpertMatmul for OmegaExperts {
    fn k(&self) -> usize {
        self.1
    }
    fn n(&self) -> usize {
        self.2
    }
    fn windows_f32(&self, m: usize, a: &[f32], col0s: &[u32], cols: usize, c: &mut [f32]) -> Result<u64, String> {
        let t = self.0.lock().map_err(|_| "expert tensor lock poisoned".to_string())?;
        t.windows_f32(m, a, col0s, cols, c).map(|i| i.raw.elapsed_ns).map_err(|e| e.to_string())
    }
}

/// Fallback when the window API is not linked: the whole-tensor product, windows copied out.
/// Correct, and far slower (every expert is computed): only for bring-up without cut 2.
#[derive(Debug)]
pub struct WholeTensorExperts {
    dense: OmegaDense,
    scratch: Mutex<Vec<f32>>,
}

impl ExpertMatmul for WholeTensorExperts {
    fn k(&self) -> usize {
        self.dense.k()
    }
    fn n(&self) -> usize {
        self.dense.n()
    }
    fn windows_f32(&self, m: usize, a: &[f32], col0s: &[u32], cols: usize, c: &mut [f32]) -> Result<u64, String> {
        let n = self.n();
        let mut full = self.scratch.lock().map_err(|_| "scratch lock poisoned".to_string())?;
        full.resize(m * n, 0.0);
        let ns = self.dense.matmul_f32(m, a, &mut full)?;
        copy_windows_out(&full, m, n, col0s, cols, c)?;
        Ok(ns)
    }
}

/// Copies `col0s.len()` column windows of a `[m][n]` product into `c` (window-major, `[m][cols]`
/// each). Shared by the fallback and the test fake.
pub fn copy_windows_out(full: &[f32], m: usize, n: usize, col0s: &[u32], cols: usize, c: &mut [f32]) -> Result<(), String> {
    if c.len() != col0s.len() * m * cols {
        return Err(format!("window output holds {} values, {} expected", c.len(), col0s.len() * m * cols));
    }
    for (w, &col0) in col0s.iter().enumerate() {
        aien_omega_moe::check_window(n, col0 as usize, cols).map_err(|e| e.to_string())?;
        let col0 = col0 as usize;
        for r in 0..m {
            let src = &full[r * n + col0..r * n + col0 + cols];
            c[(w * m + r) * cols..(w * m + r + 1) * cols].copy_from_slice(src);
        }
    }
    Ok(())
}

/// The real uploader: Omega resident tensors; experts through the window API when it is linked,
/// the whole-tensor fallback otherwise (`uses_window_api` says which).
pub struct OmegaModelUploader;

impl OmegaModelUploader {
    pub fn uses_window_api() -> bool {
        aien_omega_moe::is_native()
    }
}

impl ModelUploader for OmegaModelUploader {
    fn upload_dense(&self, k: usize, n: usize, bits: &[u16]) -> Result<Arc<dyn DenseMatmul>, String> {
        let t = aien_omega_gpu::ResidentTensor::upload_bf16(k, n, bits).map_err(|e| e.to_string())?;
        Ok(Arc::new(OmegaDense(Mutex::new(t), k, n)))
    }
    fn upload_experts(&self, k: usize, n: usize, bits: &[u16]) -> Result<Arc<dyn ExpertMatmul>, String> {
        if Self::uses_window_api() {
            let t = aien_omega_moe::MoeTensor::upload_bf16(k, n, bits).map_err(|e| e.to_string())?;
            Ok(Arc::new(OmegaExperts(Mutex::new(t), k, n)))
        } else {
            let t = aien_omega_gpu::ResidentTensor::upload_bf16(k, n, bits).map_err(|e| e.to_string())?;
            Ok(Arc::new(WholeTensorExperts {
                dense: OmegaDense(Mutex::new(t), k, n),
                scratch: Mutex::new(Vec::new()),
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// The loaded model.
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct OmegaLayer {
    pub input_norm: Vec<f32>,
    pub post_norm: Vec<f32>,
    pub q_norm: Vec<f32>,
    pub k_norm: Vec<f32>,
    /// Router `[experts][hidden]` f32.
    pub router: Vec<f32>,
    pub qkv: Arc<dyn DenseMatmul>,
    pub o: Arc<dyn DenseMatmul>,
    pub gate_up: Arc<dyn ExpertMatmul>,
    pub down: Arc<dyn ExpertMatmul>,
}

/// The whole model: resident matmuls on the device, bf16 embedding and f32 norms/routers on the host.
pub struct Qwen3MoeOmegaWeights {
    pub config: Qwen3MoeConfig,
    /// `[vocab][hidden]` bf16 bits; a row is widened to f32 at lookup.
    pub embed_bf16: Vec<u16>,
    pub layers: Vec<OmegaLayer>,
    pub final_norm: Vec<f32>,
    pub lm_head: Arc<dyn DenseMatmul>,
}

impl Qwen3MoeOmegaWeights {
    pub fn embed_row(&self, token: u32) -> Vec<f32> {
        let h = self.config.hidden;
        let t = (token as usize) % self.config.vocab;
        self.embed_bf16[t * h..(t + 1) * h].iter().map(|&b| bf16_to_f32(b)).collect()
    }

    /// Bytes the host keeps after the load (embedding, norms, routers).
    pub fn host_bytes(&self) -> usize {
        self.embed_bf16.len() * 2
            + self.final_norm.len() * 4
            + self
                .layers
                .iter()
                .map(|l| 4 * (l.input_norm.len() + l.post_norm.len() + l.q_norm.len() + l.k_norm.len() + l.router.len()))
                .sum::<usize>()
    }
}

/// What a load measured.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadReport {
    pub resident_bytes: usize,
    pub host_bytes: usize,
    /// Process peak resident set (VmHWM) after the load, if `/proc` is readable.
    pub peak_rss_bytes: Option<u64>,
    pub peak_rss_before_bytes: Option<u64>,
    pub seconds: f64,
    pub window_api: bool,
}

/// Peak resident set size of this process (`VmHWM` in `/proc/self/status`), bytes.
pub fn peak_rss_bytes() -> Option<u64> {
    proc_status_kb("VmHWM:").map(|kb| kb * 1024)
}

/// Current resident set size (`VmRSS`), bytes.
pub fn current_rss_bytes() -> Option<u64> {
    proc_status_kb("VmRSS:").map(|kb| kb * 1024)
}

fn proc_status_kb(key: &str) -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    text.lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
}

/// Loads a checkpoint directory: `config.json` for the shape, the shards for the weights.
pub fn load_qwen3_moe_omega(
    dir: &Path,
    uploader: &dyn ModelUploader,
    progress: &mut dyn FnMut(&str),
) -> Result<(Qwen3MoeOmegaWeights, LoadReport), String> {
    let config = Qwen3MoeConfig::from_checkpoint(dir)?;
    let mut source = ShardSource::open(dir)?;
    load_qwen3_moe_omega_from(&config, &mut source, uploader, progress)
}

/// The loader proper, over any tensor source (tests use an in-memory one).
pub fn load_qwen3_moe_omega_from(
    config: &Qwen3MoeConfig,
    source: &mut dyn TensorSource,
    uploader: &dyn ModelUploader,
    progress: &mut dyn FnMut(&str),
) -> Result<(Qwen3MoeOmegaWeights, LoadReport), String> {
    config.validate()?;
    let t0 = Instant::now();
    let peak_before = peak_rss_bytes();
    let (h, i, e) = (config.hidden, config.moe_intermediate, config.experts);
    let (qd, kvd, hd) = (config.q_dim(), config.kv_dim(), config.head_dim);
    let kb = h / FP8_BLOCK;
    let ib = i / FP8_BLOCK;
    let qkv_n = config.qkv_n();

    // Transient host buffers, sized once for the largest use and reused for every layer.
    let mut bits: Vec<u16> = vec![0; h * config.gate_up_n().max(qkv_n).max(qd)];
    let mut fp8: Vec<u8> = vec![0; (e * 2 * i * h).max(qd * h + 2 * kvd * h)];
    let mut scales: Vec<u16> = vec![0; (e * 2 * ib * kb).max((qd / FP8_BLOCK + 2 * (kvd / FP8_BLOCK)) * kb)];
    let mut small: Vec<u8> = Vec::new();

    let read_bf16_f32 = |source: &mut dyn TensorSource, name: &str, shape: &[usize], small: &mut Vec<u8>| -> Result<Vec<f32>, String> {
        let n: usize = shape.iter().product();
        small.resize(n * 2, 0);
        source.read_into(name, "BF16", shape, small)?;
        Ok(small
            .chunks_exact(2)
            .map(|b| bf16_to_f32(u16::from_le_bytes([b[0], b[1]])))
            .collect())
    };

    let mut layers = Vec::with_capacity(config.layers);
    for layer in 0..config.layers {
        let p = format!("model.layers.{layer}");
        // --- fused q|k|v: sources q [qd][h], k [kvd][h], v [kvd][h] at columns 0, qd, qd+kvd ---
        let specs = [("q_proj", qd, 0usize), ("k_proj", kvd, qd), ("v_proj", kvd, qd + kvd)];
        let mut w_off = 0usize;
        let mut s_off = 0usize;
        let mut ranges = Vec::new();
        for (name, rows, col0) in specs {
            let wl = rows * h;
            let sl = (rows / FP8_BLOCK) * kb;
            source.read_into(&format!("{p}.self_attn.{name}.weight"), "F8_E4M3", &[rows, h], &mut fp8[w_off..w_off + wl])?;
            read_scales(source, &format!("{p}.self_attn.{name}.weight_scale_inv"), &[rows / FP8_BLOCK, kb], &mut scales[s_off..s_off + sl])?;
            ranges.push((w_off..w_off + wl, s_off..s_off + sl, rows, col0));
            w_off += wl;
            s_off += sl;
        }
        let sources: Vec<Fp8Source> = ranges
            .iter()
            .map(|(w, s, rows, col0)| Fp8Source { weights: &fp8[w.clone()], scales: &scales[s.clone()], rows: *rows, col0: *col0 })
            .collect();
        dequant_fp8_sources_to_bf16_cols(&sources, h, &mut bits[..h * qkv_n], qkv_n)?;
        let qkv = uploader.upload_dense(h, qkv_n, &bits[..h * qkv_n])?;
        progress(&format!("{p}.self_attn.qkv"));

        // --- o: [h][qd] -> dst [qd][h] ---
        source.read_into(&format!("{p}.self_attn.o_proj.weight"), "F8_E4M3", &[h, qd], &mut fp8[..h * qd])?;
        read_scales(source, &format!("{p}.self_attn.o_proj.weight_scale_inv"), &[kb, qd / FP8_BLOCK], &mut scales[..kb * (qd / FP8_BLOCK)])?;
        dequant_fp8_sources_to_bf16_cols(
            &[Fp8Source { weights: &fp8[..h * qd], scales: &scales[..kb * (qd / FP8_BLOCK)], rows: h, col0: 0 }],
            qd,
            &mut bits[..qd * h],
            h,
        )?;
        let o = uploader.upload_dense(qd, h, &bits[..qd * h])?;
        progress(&format!("{p}.self_attn.o_proj"));

        // --- experts gate|up: per expert gate [i][h] then up [i][h]; columns e*2i (gate), e*2i+i (up) ---
        let gu_n = config.gate_up_n();
        for ex in 0..e {
            for (j, proj) in ["gate_proj", "up_proj"].into_iter().enumerate() {
                let w0 = (ex * 2 + j) * i * h;
                let s0 = (ex * 2 + j) * ib * kb;
                let base = format!("{p}.mlp.experts.{ex}.{proj}");
                source.read_into(&format!("{base}.weight"), "F8_E4M3", &[i, h], &mut fp8[w0..w0 + i * h])?;
                read_scales(source, &format!("{base}.weight_scale_inv"), &[ib, kb], &mut scales[s0..s0 + ib * kb])?;
            }
        }
        let sources: Vec<Fp8Source> = (0..e)
            .flat_map(|ex| {
                (0..2).map(move |j| (ex, j))
            })
            .map(|(ex, j)| {
                let w0 = (ex * 2 + j) * i * h;
                let s0 = (ex * 2 + j) * ib * kb;
                Fp8Source {
                    weights: &fp8[w0..w0 + i * h],
                    scales: &scales[s0..s0 + ib * kb],
                    rows: i,
                    col0: config.gate_up_col0(ex) + j * i,
                }
            })
            .collect();
        dequant_fp8_sources_to_bf16_cols(&sources, h, &mut bits[..h * gu_n], gu_n)?;
        let gate_up = uploader.upload_experts(h, gu_n, &bits[..h * gu_n])?;
        progress(&format!("{p}.mlp.experts.gate_up"));

        // --- experts down: per expert [h][i]; columns e*h ---
        let dn_n = config.down_n();
        for ex in 0..e {
            let base = format!("{p}.mlp.experts.{ex}.down_proj");
            source.read_into(&format!("{base}.weight"), "F8_E4M3", &[h, i], &mut fp8[ex * h * i..(ex + 1) * h * i])?;
            read_scales(source, &format!("{base}.weight_scale_inv"), &[kb, ib], &mut scales[ex * kb * ib..(ex + 1) * kb * ib])?;
        }
        let sources: Vec<Fp8Source> = (0..e)
            .map(|ex| Fp8Source {
                weights: &fp8[ex * h * i..(ex + 1) * h * i],
                scales: &scales[ex * kb * ib..(ex + 1) * kb * ib],
                rows: h,
                col0: config.down_col0(ex),
            })
            .collect();
        dequant_fp8_sources_to_bf16_cols(&sources, i, &mut bits[..i * dn_n], dn_n)?;
        let down = uploader.upload_experts(i, dn_n, &bits[..i * dn_n])?;
        progress(&format!("{p}.mlp.experts.down"));

        layers.push(OmegaLayer {
            input_norm: read_bf16_f32(source, &format!("{p}.input_layernorm.weight"), &[h], &mut small)?,
            post_norm: read_bf16_f32(source, &format!("{p}.post_attention_layernorm.weight"), &[h], &mut small)?,
            q_norm: read_bf16_f32(source, &format!("{p}.self_attn.q_norm.weight"), &[hd], &mut small)?,
            k_norm: read_bf16_f32(source, &format!("{p}.self_attn.k_norm.weight"), &[hd], &mut small)?,
            router: read_bf16_f32(source, &format!("{p}.mlp.gate.weight"), &[e, h], &mut small)?,
            qkv,
            o,
            gate_up,
            down,
        });
    }
    drop(fp8);
    drop(scales);

    // lm_head: bf16 [vocab][h] -> [h][vocab].
    let vocab = config.vocab;
    let mut raw = vec![0u8; vocab * h * 2];
    source.read_into("lm_head.weight", "BF16", &[vocab, h], &mut raw)?;
    bits.clear();
    transpose_bf16_for_upload(&raw, vocab, h, &mut bits);
    let lm_head = uploader.upload_dense(h, vocab, &bits)?;
    progress("lm_head");
    drop(bits);

    // Embedding stays on the host as bf16 bits (half the f32 size).
    source.read_into("model.embed_tokens.weight", "BF16", &[vocab, h], &mut raw)?;
    let embed_bf16: Vec<u16> = raw.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
    drop(raw);
    let final_norm = read_bf16_f32(source, "model.norm.weight", &[h], &mut small)?;

    let weights = Qwen3MoeOmegaWeights {
        config: config.clone(),
        embed_bf16,
        layers,
        final_norm,
        lm_head,
    };
    let report = LoadReport {
        resident_bytes: config.model_resident_bytes(),
        host_bytes: weights.host_bytes(),
        peak_rss_bytes: peak_rss_bytes(),
        peak_rss_before_bytes: peak_before,
        seconds: t0.elapsed().as_secs_f64(),
        window_api: aien_omega_moe::is_native(),
    };
    Ok((weights, report))
}

fn read_scales(source: &mut dyn TensorSource, name: &str, shape: &[usize], dst: &mut [u16]) -> Result<(), String> {
    // SAFETY: a u16 slice viewed as bytes of twice the length; the source writes little-endian
    // bf16 words, which is this host's byte order (aarch64 / x86 little endian).
    let bytes = unsafe { std::slice::from_raw_parts_mut(dst.as_mut_ptr().cast::<u8>(), dst.len() * 2) };
    source.read_into(name, "BF16", shape, bytes)
}

/// Largest transient the loader holds beyond the model's own host bytes, for the given shape:
/// the bf16 upload buffer, the FP8 layer buffer, its scales and the raw lm_head/embedding read.
pub fn loader_transient_bytes(config: &Qwen3MoeConfig) -> usize {
    let (h, i, e) = (config.hidden, config.moe_intermediate, config.experts);
    let bits = 2 * h * config.gate_up_n().max(config.qkv_n()).max(config.q_dim());
    let fp8 = (e * 2 * i * h).max(config.q_dim() * h + 2 * config.kv_dim() * h);
    let scales = 2 * (e * 2 * (i / FP8_BLOCK) * (h / FP8_BLOCK)).max(((config.q_dim() + 2 * config.kv_dim()) / FP8_BLOCK) * (h / FP8_BLOCK));
    let raw = 2 * config.vocab * h;
    bits + fp8 + scales + raw
}

#[cfg(test)]
pub(crate) mod test_support {
    //! An in-memory checkpoint and a recording uploader with host matmuls, so the loader and the
    //! forward can be exercised end to end without a chip.
    use super::*;
    use crate::qwen3_moe::{f32_to_bf16, f32_to_fp8_e4m3fn};

    pub fn tiny_config() -> Qwen3MoeConfig {
        Qwen3MoeConfig {
            layers: 2,
            hidden: 256,
            q_heads: 2,
            kv_heads: 1,
            head_dim: 128,
            experts: 4,
            top_k: 2,
            moe_intermediate: 128,
            vocab: 64,
            rms_eps: 1e-6,
            rope_theta: 1e7,
            norm_topk_prob: true,
            tie_word_embeddings: false,
            eos_token_ids: vec![3],
        }
    }

    /// Deterministic pseudo-random values in [-1, 1).
    pub fn prand(seed: u64, idx: usize) -> f32 {
        let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(idx as u64 + 1);
        x ^= x >> 29;
        x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x ^= x >> 32;
        ((x & 0xFFFF) as f32 / 32768.0) - 1.0
    }

    #[derive(Default)]
    pub struct MemorySource {
        pub tensors: HashMap<String, (String, Vec<usize>, Vec<u8>)>,
        pub reads: Vec<String>,
    }

    impl MemorySource {
        fn put_bf16(&mut self, name: &str, shape: &[usize], seed: u64, scale: f32) {
            let n: usize = shape.iter().product();
            let bytes: Vec<u8> = (0..n)
                .flat_map(|i| f32_to_bf16(prand(seed, i) * scale).to_le_bytes())
                .collect();
            self.tensors.insert(name.to_string(), ("BF16".into(), shape.to_vec(), bytes));
        }
        fn put_fp8(&mut self, name: &str, rows: usize, cols: usize, seed: u64) {
            let w: Vec<u8> = (0..rows * cols).map(|i| f32_to_fp8_e4m3fn(prand(seed, i) * 8.0)).collect();
            self.tensors.insert(name.to_string(), ("F8_E4M3".into(), vec![rows, cols], w));
            let sshape = [rows.div_ceil(FP8_BLOCK), cols / FP8_BLOCK];
            let s: Vec<u8> = (0..sshape[0] * sshape[1])
                .flat_map(|i| f32_to_bf16(0.02 + 0.01 * (prand(seed ^ 0x55, i) + 1.0)).to_le_bytes())
                .collect();
            self.tensors.insert(format!("{name}_scale_inv"), ("BF16".into(), sshape.to_vec(), s));
        }

        pub fn for_config(c: &Qwen3MoeConfig) -> Self {
            let mut m = Self::default();
            let (h, i, e, hd) = (c.hidden, c.moe_intermediate, c.experts, c.head_dim);
            m.put_bf16("model.embed_tokens.weight", &[c.vocab, h], 1, 1.0);
            m.put_bf16("lm_head.weight", &[c.vocab, h], 2, 0.1);
            m.put_bf16("model.norm.weight", &[h], 3, 1.0);
            for l in 0..c.layers {
                let p = format!("model.layers.{l}");
                let seed = 100 + l as u64 * 1000;
                m.put_fp8(&format!("{p}.self_attn.q_proj.weight"), c.q_dim(), h, seed + 1);
                m.put_fp8(&format!("{p}.self_attn.k_proj.weight"), c.kv_dim(), h, seed + 2);
                m.put_fp8(&format!("{p}.self_attn.v_proj.weight"), c.kv_dim(), h, seed + 3);
                m.put_fp8(&format!("{p}.self_attn.o_proj.weight"), h, c.q_dim(), seed + 4);
                m.put_bf16(&format!("{p}.self_attn.q_norm.weight"), &[hd], seed + 5, 1.0);
                m.put_bf16(&format!("{p}.self_attn.k_norm.weight"), &[hd], seed + 6, 1.0);
                m.put_bf16(&format!("{p}.input_layernorm.weight"), &[h], seed + 7, 1.0);
                m.put_bf16(&format!("{p}.post_attention_layernorm.weight"), &[h], seed + 8, 1.0);
                m.put_bf16(&format!("{p}.mlp.gate.weight"), &[e, h], seed + 9, 1.0);
                for ex in 0..e {
                    let b = format!("{p}.mlp.experts.{ex}");
                    m.put_fp8(&format!("{b}.gate_proj.weight"), i, h, seed + 10 + ex as u64 * 3);
                    m.put_fp8(&format!("{b}.up_proj.weight"), i, h, seed + 11 + ex as u64 * 3);
                    m.put_fp8(&format!("{b}.down_proj.weight"), h, i, seed + 12 + ex as u64 * 3);
                }
            }
            m
        }

        pub fn fp8_f32(&self, name: &str) -> (Vec<f32>, usize, usize) {
            let (_, shape, w) = &self.tensors[name];
            let (_, _, s) = &self.tensors[&format!("{name}_scale_inv")];
            let (rows, cols) = (shape[0], shape[1]);
            let kb = cols / FP8_BLOCK;
            let scales: Vec<u16> = s.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
            let lut = fp8_table();
            let out = (0..rows * cols)
                .map(|idx| {
                    let (r, k) = (idx / cols, idx % cols);
                    lut[w[idx] as usize] * bf16_to_f32(scales[(r / FP8_BLOCK) * kb + k / FP8_BLOCK])
                })
                .collect();
            (out, rows, cols)
        }
    }

    impl TensorSource for MemorySource {
        fn read_into(&mut self, name: &str, dtype: &str, shape: &[usize], dst: &mut [u8]) -> Result<(), String> {
            let (dt, sh, bytes) = self.tensors.get(name).ok_or_else(|| format!("no tensor {name}"))?;
            if dt != dtype || sh != shape {
                return Err(format!("{name}: {dt} {sh:?} asked as {dtype} {shape:?}"));
            }
            if bytes.len() != dst.len() {
                return Err(format!("{name}: {} bytes, dst {}", bytes.len(), dst.len()));
            }
            dst.copy_from_slice(bytes);
            self.reads.push(name.to_string());
            Ok(())
        }
    }

    /// A `[k][n]` bf16 matrix on the host; matmul in f32 with `a` rounded to bf16 as omega does.
    #[derive(Debug)]
    pub struct HostMatrix {
        pub k: usize,
        pub n: usize,
        pub bits: Vec<u16>,
    }

    impl HostMatrix {
        pub fn full(&self, m: usize, a: &[f32], c: &mut [f32]) {
            assert_eq!(a.len(), m * self.k);
            assert_eq!(c.len(), m * self.n);
            for r in 0..m {
                let ar: Vec<f32> = a[r * self.k..(r + 1) * self.k].iter().map(|&v| bf16_to_f32(f32_to_bf16(v))).collect();
                for j in 0..self.n {
                    let mut acc = 0.0f32;
                    for kk in 0..self.k {
                        acc += ar[kk] * bf16_to_f32(self.bits[kk * self.n + j]);
                    }
                    c[r * self.n + j] = acc;
                }
            }
        }
    }

    impl DenseMatmul for HostMatrix {
        fn k(&self) -> usize {
            self.k
        }
        fn n(&self) -> usize {
            self.n
        }
        fn matmul_f32(&self, m: usize, a: &[f32], c: &mut [f32]) -> Result<u64, String> {
            self.full(m, a, c);
            Ok(0)
        }
    }

    impl ExpertMatmul for HostMatrix {
        fn k(&self) -> usize {
            self.k
        }
        fn n(&self) -> usize {
            self.n
        }
        fn windows_f32(&self, m: usize, a: &[f32], col0s: &[u32], cols: usize, c: &mut [f32]) -> Result<u64, String> {
            // Only the windows are computed (so a wrong col0 is a wrong answer, as on the chip).
            if c.len() != col0s.len() * m * cols {
                return Err("window output size".into());
            }
            for (w, &col0) in col0s.iter().enumerate() {
                aien_omega_moe::check_window(self.n, col0 as usize, cols).map_err(|e| e.to_string())?;
                for r in 0..m {
                    let ar: Vec<f32> = a[r * self.k..(r + 1) * self.k].iter().map(|&v| bf16_to_f32(f32_to_bf16(v))).collect();
                    for j in 0..cols {
                        let col = col0 as usize + j;
                        let mut acc = 0.0f32;
                        for kk in 0..self.k {
                            acc += ar[kk] * bf16_to_f32(self.bits[kk * self.n + col]);
                        }
                        c[(w * m + r) * cols + j] = acc;
                    }
                }
            }
            Ok(0)
        }
    }

    /// Records every upload (kind, k, n, bytes) and keeps the matrices for host matmuls.
    #[derive(Default)]
    pub struct RecordingUploader {
        pub uploads: Mutex<Vec<(&'static str, usize, usize, usize)>>,
        pub matrices: Mutex<Vec<Arc<HostMatrix>>>,
    }

    impl ModelUploader for RecordingUploader {
        fn upload_dense(&self, k: usize, n: usize, bits: &[u16]) -> Result<Arc<dyn DenseMatmul>, String> {
            assert_eq!(bits.len(), k * n);
            self.uploads.lock().unwrap().push(("dense", k, n, bits.len() * 2));
            let m = Arc::new(HostMatrix { k, n, bits: bits.to_vec() });
            self.matrices.lock().unwrap().push(m.clone());
            Ok(m)
        }
        fn upload_experts(&self, k: usize, n: usize, bits: &[u16]) -> Result<Arc<dyn ExpertMatmul>, String> {
            assert_eq!(bits.len(), k * n);
            self.uploads.lock().unwrap().push(("experts", k, n, bits.len() * 2));
            let m = Arc::new(HostMatrix { k, n, bits: bits.to_vec() });
            self.matrices.lock().unwrap().push(m.clone());
            Ok(m)
        }
    }
}

#[cfg(test)]
mod loader_tests {
    use super::test_support::*;
    use super::*;
    use crate::qwen3_moe::fp8_e4m3fn_to_f32;

    const REAL_CONFIG: &str = r#"{"architectures":["Qwen3MoeForCausalLM"],"bos_token_id":151643,"decoder_sparse_step":1,"eos_token_id":151645,"head_dim":128,"hidden_size":2048,"max_position_embeddings":262144,"mlp_only_layers":[],"model_type":"qwen3_moe","moe_intermediate_size":768,"norm_topk_prob":true,"num_attention_heads":32,"num_experts":128,"num_experts_per_tok":8,"num_hidden_layers":48,"num_key_value_heads":4,"rms_norm_eps":0.000001,"rope_theta":10000000,"tie_word_embeddings":false,"vocab_size":151936}"#;

    #[test]
    fn real_config_parses_and_sizes_match_the_design() {
        let c = Qwen3MoeConfig::from_json(REAL_CONFIG).unwrap();
        assert_eq!((c.layers, c.hidden, c.q_heads, c.kv_heads, c.head_dim), (48, 2048, 32, 4, 128));
        assert_eq!((c.experts, c.top_k, c.moe_intermediate, c.vocab), (128, 8, 768, 151936));
        assert_eq!(c.eos_token_ids, vec![151645]);
        assert_eq!(c.qkv_n(), 5120);
        assert_eq!(c.gate_up_n(), 196608);
        assert_eq!(c.down_n(), 262144);
        // Design section 0: experts 48 x 1.21 GB, attention 1.8 GB, lm_head 0.62 GB: about 60.4 GB.
        let layer = c.layer_resident_bytes();
        assert_eq!(layer, 2 * (2048 * 5120 + 4096 * 2048 + 2048 * 196608 + 768 * 262144));
        let total = c.model_resident_bytes();
        assert!((60.0e9..61.0e9).contains(&(total as f64)), "{total}");
        // bf16 bytes per generated token: about 6.1 GB (design section 0).
        let per_tok = c.bytes_per_token_bf16() as f64;
        assert!((6.0e9..6.2e9).contains(&per_tok), "{per_tok}");
    }

    #[test]
    fn config_refuses_shapes_stage_a_cannot_run() {
        let mut c = Qwen3MoeConfig::from_json(REAL_CONFIG).unwrap();
        c.head_dim = 64;
        assert!(c.validate().is_err());
        let mut c = Qwen3MoeConfig::from_json(REAL_CONFIG).unwrap();
        c.experts = 129;
        assert!(c.validate().unwrap_err().contains("down"));
        let mut c = Qwen3MoeConfig::from_json(REAL_CONFIG).unwrap();
        c.norm_topk_prob = false;
        assert!(c.validate().is_err());
        assert!(Qwen3MoeConfig::from_json(&REAL_CONFIG.replace("qwen3_moe", "qwen3")).is_err());
    }

    #[test]
    fn expert_windows_start_on_a_column_tile_and_stay_inside_the_tensor() {
        let c = Qwen3MoeConfig::from_json(REAL_CONFIG).unwrap();
        for e in 0..c.experts {
            let gu = c.gate_up_col0(e);
            let dn = c.down_col0(e);
            assert_eq!(gu % WINDOW_TILE, 0);
            assert_eq!(dn % WINDOW_TILE, 0);
            assert!(aien_omega_moe::check_window(c.gate_up_n(), gu, c.gate_up_cols()).is_ok());
            assert!(aien_omega_moe::check_window(c.down_n(), dn, c.down_cols()).is_ok());
        }
        assert_eq!(c.gate_up_col0(5), 7680);
        assert_eq!(c.down_col0(127), 260096);
        assert_eq!(c.down_col0(127) + c.down_cols(), MAX_RESIDENT_N);
        // Up half of expert e sits i columns after its gate half.
        assert_eq!(c.gate_up_col0(3) + c.moe_intermediate, 3 * 1536 + 768);
    }

    #[test]
    fn dequant_matches_the_exact_conversion_transposed_and_rounded() {
        // rows 200 (not a block multiple: 2 row blocks), k 256 (2 blocks).
        let (rows, k) = (200usize, 256usize);
        let w: Vec<u8> = (0..rows * k).map(|i| ((i * 37 + 11) % 256) as u8).map(|b| if b & 0x7f == 0x7f { 0 } else { b }).collect();
        let scales: Vec<u16> = (0..2 * 2).map(|i| f32_to_bf16(0.5 + i as f32 * 0.25)).collect();
        let dst_n = 208;
        let mut dst = vec![0u16; k * dst_n];
        dequant_fp8_sources_to_bf16_cols(&[Fp8Source { weights: &w, scales: &scales, rows, col0: 8 }], k, &mut dst, dst_n).unwrap();
        for r in 0..rows {
            for kk in 0..k {
                let s = bf16_to_f32(scales[(r / 128) * 2 + kk / 128]);
                let want = f32_to_bf16(fp8_e4m3fn_to_f32(w[r * k + kk]) * s);
                assert_eq!(dst[kk * dst_n + 8 + r], want, "r={r} k={kk}");
            }
            // Spot values: the exact table says 0x38 is 1.0 and 0xBC is -1.5.
        }
        assert_eq!(bf16_to_f32(f32_to_bf16(fp8_e4m3fn_to_f32(0x38))), 1.0);
        assert_eq!(fp8_e4m3fn_to_f32(0xBC), -1.5);
        // Untouched columns stay zero.
        assert!(dst.iter().enumerate().filter(|(i, _)| i % dst_n < 8).all(|(_, &v)| v == 0));
    }

    #[test]
    fn dequant_refuses_nan_codes_and_bad_shapes() {
        let w = vec![0x7fu8; 128];
        let s = vec![f32_to_bf16(1.0)];
        let mut dst = vec![0u16; 128 * 8];
        let err = dequant_fp8_sources_to_bf16_cols(&[Fp8Source { weights: &w, scales: &s, rows: 1, col0: 0 }], 128, &mut dst, 8).unwrap_err();
        assert!(err.contains("NaN"), "{err}");
        let w = vec![0u8; 128];
        let err = dequant_fp8_sources_to_bf16_cols(&[Fp8Source { weights: &w, scales: &s, rows: 1, col0: 8 }], 128, &mut dst, 8).unwrap_err();
        assert!(err.contains("past dst_n"), "{err}");
        let err = dequant_fp8_sources_to_bf16_cols(&[Fp8Source { weights: &w, scales: &[], rows: 1, col0: 0 }], 128, &mut dst, 8).unwrap_err();
        assert!(err.contains("scales"), "{err}");
    }

    #[test]
    fn loader_uploads_the_right_shapes_bytes_and_values() {
        let c = tiny_config();
        let mut src = MemorySource::for_config(&c);
        let up = RecordingUploader::default();
        let mut seen = Vec::new();
        let (w, report) = load_qwen3_moe_omega_from(&c, &mut src, &up, &mut |s| seen.push(s.to_string())).unwrap();
        let uploads = up.uploads.lock().unwrap().clone();
        // Per layer: qkv, o, gate_up, down; then lm_head.
        assert_eq!(uploads.len(), 4 * c.layers + 1);
        for l in 0..c.layers {
            assert_eq!(uploads[4 * l], ("dense", c.hidden, c.qkv_n(), 2 * c.hidden * c.qkv_n()));
            assert_eq!(uploads[4 * l + 1], ("dense", c.q_dim(), c.hidden, 2 * c.q_dim() * c.hidden));
            assert_eq!(uploads[4 * l + 2], ("experts", c.hidden, c.gate_up_n(), 2 * c.hidden * c.gate_up_n()));
            assert_eq!(uploads[4 * l + 3], ("experts", c.moe_intermediate, c.down_n(), 2 * c.moe_intermediate * c.down_n()));
        }
        assert_eq!(uploads[4 * c.layers], ("dense", c.hidden, c.vocab, 2 * c.hidden * c.vocab));
        let total: usize = uploads.iter().map(|u| u.3).sum();
        assert_eq!(total, c.model_resident_bytes());
        assert_eq!(report.resident_bytes, total);
        assert_eq!(seen.len(), uploads.len());
        assert_eq!(w.host_bytes(), report.host_bytes);
        assert_eq!(w.embed_bf16.len(), c.vocab * c.hidden);
        assert_eq!(w.layers[1].router.len(), c.experts * c.hidden);

        // Values: expert 2's up projection of layer 1, row 5, column k=130 must sit at
        // gate_up[k=130][col0(2) + i + 5], equal to the exact dequant rounded to bf16.
        let mats = up.matrices.lock().unwrap();
        let gate_up = &mats[4 + 2];
        let (upw, rows, cols) = src.fp8_f32("model.layers.1.mlp.experts.2.up_proj.weight");
        assert_eq!((rows, cols), (c.moe_intermediate, c.hidden));
        let col = c.gate_up_col0(2) + c.moe_intermediate + 5;
        assert_eq!(gate_up.bits[130 * c.gate_up_n() + col], f32_to_bf16(upw[5 * cols + 130]));
        // down of expert 3, layer 0: W[h=7][i=100] at down[k=100][col0(3) + 7].
        let down = &mats[3];
        let (dw, _, dcols) = src.fp8_f32("model.layers.0.mlp.experts.3.down_proj.weight");
        assert_eq!(down.bits[100 * c.down_n() + c.down_col0(3) + 7], f32_to_bf16(dw[7 * dcols + 100]));
        // v_proj row 3 of layer 0 lands at qkv column q_dim + kv_dim + 3.
        let qkv = &mats[0];
        let (vw, _, vcols) = src.fp8_f32("model.layers.0.self_attn.v_proj.weight");
        assert_eq!(qkv.bits[77 * c.qkv_n() + c.q_dim() + c.kv_dim() + 3], f32_to_bf16(vw[3 * vcols + 77]));
        // o_proj: W[h=9][qd=200] at o[k=200][9].
        let o = &mats[1];
        let (ow, _, ocols) = src.fp8_f32("model.layers.0.self_attn.o_proj.weight");
        assert_eq!(o.bits[200 * c.hidden + 9], f32_to_bf16(ow[9 * ocols + 200]));
        // lm_head [vocab][h] transposed: row 10, k 33 at lm_head[33][10].
        let lm = &mats[mats.len() - 1];
        let (_, _, raw) = &src.tensors["lm_head.weight"];
        let want = u16::from_le_bytes([raw[(10 * c.hidden + 33) * 2], raw[(10 * c.hidden + 33) * 2 + 1]]);
        assert_eq!(lm.bits[33 * c.vocab + 10], want);
        // Embedding row is the bf16 row widened.
        let (_, _, eraw) = &src.tensors["model.embed_tokens.weight"];
        let e0 = u16::from_le_bytes([eraw[(5 * c.hidden) * 2], eraw[(5 * c.hidden) * 2 + 1]]);
        assert_eq!(w.embed_row(5)[0], bf16_to_f32(e0));
    }

    #[test]
    fn loader_transient_budget_is_one_layer_plus_embedding() {
        let c = Qwen3MoeConfig::from_json(REAL_CONFIG).unwrap();
        let t = loader_transient_bytes(&c) as f64;
        // bits 805 MB + fp8 403 MB + scales 1.5 MB + raw lm_head 622 MB: about 1.83 GB.
        assert!((1.7e9..2.0e9).contains(&t), "{t}");
        // With the fake uploader the process holds the matrices itself, so only the report's
        // bookkeeping is checked here; the real-load assertion is in own30b_check.
        assert!(peak_rss_bytes().is_some_and(|b| b > 0));
    }
}

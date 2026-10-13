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
use crate::moe_plan::MoeBatchPlan;
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

/// `experts` resident FP8 projections of `[n][k]` with block-128 scales (the checkpoint bytes as
/// they are). `grouped_f32(ids, x, y)`: one launch for `ids.len()` experts; `x` is `k` values
/// shared by every group or `ids.len() * k` (one row per group); `y` is `ids.len() * n`,
/// group-major. A single projection is `ids == [0]`. Returns chip nanoseconds.
pub trait Fp8Gemv: Send + Sync + std::fmt::Debug {
    fn experts(&self) -> usize;
    fn n(&self) -> usize;
    fn k(&self) -> usize;
    fn grouped_f32(&self, ids: &[u32], x: &[f32], y: &mut [f32]) -> Result<u64, String>;
}

/// How the loader hands weights to the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeightFormat {
    /// Everything dequantised to bf16 (Stage A): about 60.4 GB resident.
    Bf16,
    /// Experts as FP8 with the checkpoint scales (grouped GEMV, one launch per projection per
    /// layer); attention and lm_head bf16. About 31.5 GB resident.
    Fp8Experts,
    /// Experts and the q/k/v/o projections as FP8; lm_head bf16. About 30.6 GB resident.
    Fp8All,
}

impl WeightFormat {
    pub fn fp8_experts(self) -> bool {
        self != WeightFormat::Bf16
    }
    pub fn fp8_attention(self) -> bool {
        self == WeightFormat::Fp8All
    }
}

/// Makes `[k][n]` bf16 matrices resident. The real one is [`OmegaModelUploader`].
pub trait ModelUploader {
    fn upload_dense(&self, k: usize, n: usize, bits: &[u16]) -> Result<Arc<dyn DenseMatmul>, String>;
    fn upload_experts(&self, k: usize, n: usize, bits: &[u16]) -> Result<Arc<dyn ExpertMatmul>, String>;
    /// FP8 projections as in the checkpoint: `w` is `experts * n * k` e4m3 bytes `[expert][n][k]`,
    /// `scales` is `experts * (n/128) * (k/128)` bf16 bits. Uploaders without FP8 refuse.
    fn upload_fp8(&self, experts: usize, n: usize, k: usize, w: &[u8], scales: &[u16]) -> Result<Arc<dyn Fp8Gemv>, String> {
        let _ = (experts, n, k, w, scales);
        Err("this uploader has no FP8 path".into())
    }
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
    pub fn uses_fp8_api() -> bool {
        aien_omega_fp8::is_native()
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
    fn upload_fp8(&self, experts: usize, n: usize, k: usize, w: &[u8], scales: &[u16]) -> Result<Arc<dyn Fp8Gemv>, String> {
        if !Self::uses_fp8_api() {
            return Err("this binary carries no FP8 GEMV (omega before OWN-30B cut 5, or AIEN_OMEGA_GPU_LIB without it)".into());
        }
        let t = aien_omega_fp8::Fp8Tensor::upload(experts, n, k, w, scales).map_err(|e| e.to_string())?;
        Ok(Arc::new(OmegaFp8(Mutex::new(t), experts, n, k)))
    }
}

/// Omega's FP8 grouped GEMV behind a lock.
#[derive(Debug)]
pub struct OmegaFp8(Mutex<aien_omega_fp8::Fp8Tensor>, usize, usize, usize);

impl Fp8Gemv for OmegaFp8 {
    fn experts(&self) -> usize {
        self.1
    }
    fn n(&self) -> usize {
        self.2
    }
    fn k(&self) -> usize {
        self.3
    }
    fn grouped_f32(&self, ids: &[u32], x: &[f32], y: &mut [f32]) -> Result<u64, String> {
        let t = self.0.lock().map_err(|_| "fp8 tensor lock poisoned".to_string())?;
        let info = t.gemv_grouped_f32(ids, x, y).map_err(|e| e.to_string())?;
        Ok(info.elapsed_ns)
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
    pub attn: AttnWeights,
    pub experts: ExpertWeights,
}

/// The q|k|v and o projections of one layer, in the format the loader chose.
#[derive(Debug)]
pub enum AttnWeights {
    Bf16 { qkv: Arc<dyn DenseMatmul>, o: Arc<dyn DenseMatmul> },
    Fp8 { qkv: Arc<dyn Fp8Gemv>, o: Arc<dyn Fp8Gemv> },
}

/// The expert projections of one layer, in the format the loader chose.
#[derive(Debug)]
pub enum ExpertWeights {
    Bf16 { gate_up: Arc<dyn ExpertMatmul>, down: Arc<dyn ExpertMatmul> },
    Fp8 { gate_up: Arc<dyn Fp8Gemv>, down: Arc<dyn Fp8Gemv> },
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
    pub format: WeightFormat,
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
    load_qwen3_moe_omega_with(dir, uploader, WeightFormat::Bf16, progress)
}

/// As [`load_qwen3_moe_omega`] with a chosen [`WeightFormat`].
pub fn load_qwen3_moe_omega_with(
    dir: &Path,
    uploader: &dyn ModelUploader,
    format: WeightFormat,
    progress: &mut dyn FnMut(&str),
) -> Result<(Qwen3MoeOmegaWeights, LoadReport), String> {
    let config = Qwen3MoeConfig::from_checkpoint(dir)?;
    let mut source = ShardSource::open(dir)?;
    load_qwen3_moe_omega_from_with(&config, &mut source, uploader, format, progress)
}

/// The loader proper, over any tensor source (tests use an in-memory one).
pub fn load_qwen3_moe_omega_from(
    config: &Qwen3MoeConfig,
    source: &mut dyn TensorSource,
    uploader: &dyn ModelUploader,
    progress: &mut dyn FnMut(&str),
) -> Result<(Qwen3MoeOmegaWeights, LoadReport), String> {
    load_qwen3_moe_omega_from_with(config, source, uploader, WeightFormat::Bf16, progress)
}

/// The loader proper with a chosen [`WeightFormat`]. FP8 projections go up as the checkpoint
/// bytes (`[n][k]` per expert, scales `[n/128][k/128]`), bf16 ones through the exact dequant.
pub fn load_qwen3_moe_omega_from_with(
    config: &Qwen3MoeConfig,
    source: &mut dyn TensorSource,
    uploader: &dyn ModelUploader,
    format: WeightFormat,
    progress: &mut dyn FnMut(&str),
) -> Result<(Qwen3MoeOmegaWeights, LoadReport), String> {
    config.validate()?;
    let mut resident = 0usize;
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
        let qkv_fp8 = if format.fp8_attention() {
            // q, k, v rows stacked: [qkv_n][h] with scales [qkv_n/128][kb], exactly the chip layout.
            resident += w_off + s_off * 4;
            Some(uploader.upload_fp8(1, qkv_n, h, &fp8[..w_off], &scales[..s_off])?)
        } else {
            None
        };
        let qkv = if qkv_fp8.is_none() {
            dequant_fp8_sources_to_bf16_cols(&sources, h, &mut bits[..h * qkv_n], qkv_n)?;
            resident += 2 * h * qkv_n;
            Some(uploader.upload_dense(h, qkv_n, &bits[..h * qkv_n])?)
        } else {
            None
        };
        progress(&format!("{p}.self_attn.qkv"));

        // --- o: [h][qd] -> dst [qd][h] ---
        source.read_into(&format!("{p}.self_attn.o_proj.weight"), "F8_E4M3", &[h, qd], &mut fp8[..h * qd])?;
        read_scales(source, &format!("{p}.self_attn.o_proj.weight_scale_inv"), &[kb, qd / FP8_BLOCK], &mut scales[..kb * (qd / FP8_BLOCK)])?;
        let attn = if format.fp8_attention() {
            resident += h * qd + kb * (qd / FP8_BLOCK) * 4;
            let o = uploader.upload_fp8(1, h, qd, &fp8[..h * qd], &scales[..kb * (qd / FP8_BLOCK)])?;
            AttnWeights::Fp8 { qkv: qkv_fp8.expect("fp8 qkv"), o }
        } else {
            dequant_fp8_sources_to_bf16_cols(
                &[Fp8Source { weights: &fp8[..h * qd], scales: &scales[..kb * (qd / FP8_BLOCK)], rows: h, col0: 0 }],
                qd,
                &mut bits[..qd * h],
                h,
            )?;
            resident += 2 * qd * h;
            let o = uploader.upload_dense(qd, h, &bits[..qd * h])?;
            AttnWeights::Bf16 { qkv: qkv.expect("bf16 qkv"), o }
        };
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
        let gate_up_fp8 = if format.fp8_experts() {
            // [e][2i][h] bytes with scales [e][2ib][kb]: the capsule GATE_UP layout, as the chip wants it.
            resident += e * 2 * i * h + e * 2 * ib * kb * 4;
            Some(uploader.upload_fp8(e, 2 * i, h, &fp8[..e * 2 * i * h], &scales[..e * 2 * ib * kb])?)
        } else {
            None
        };
        let gate_up = if gate_up_fp8.is_none() {
            dequant_fp8_sources_to_bf16_cols(&sources, h, &mut bits[..h * gu_n], gu_n)?;
            resident += 2 * h * gu_n;
            Some(uploader.upload_experts(h, gu_n, &bits[..h * gu_n])?)
        } else {
            None
        };
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
        let experts = if format.fp8_experts() {
            resident += e * h * i + e * kb * ib * 4;
            let down = uploader.upload_fp8(e, h, i, &fp8[..e * h * i], &scales[..e * kb * ib])?;
            ExpertWeights::Fp8 { gate_up: gate_up_fp8.expect("fp8 gate_up"), down }
        } else {
            dequant_fp8_sources_to_bf16_cols(&sources, i, &mut bits[..i * dn_n], dn_n)?;
            resident += 2 * i * dn_n;
            let down = uploader.upload_experts(i, dn_n, &bits[..i * dn_n])?;
            ExpertWeights::Bf16 { gate_up: gate_up.expect("bf16 gate_up"), down }
        };
        progress(&format!("{p}.mlp.experts.down"));

        layers.push(OmegaLayer {
            input_norm: read_bf16_f32(source, &format!("{p}.input_layernorm.weight"), &[h], &mut small)?,
            post_norm: read_bf16_f32(source, &format!("{p}.post_attention_layernorm.weight"), &[h], &mut small)?,
            q_norm: read_bf16_f32(source, &format!("{p}.self_attn.q_norm.weight"), &[hd], &mut small)?,
            k_norm: read_bf16_f32(source, &format!("{p}.self_attn.k_norm.weight"), &[hd], &mut small)?,
            router: read_bf16_f32(source, &format!("{p}.mlp.gate.weight"), &[e, h], &mut small)?,
            attn,
            experts,
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
    resident += 2 * h * vocab;
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
        resident_bytes: resident,
        format,
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

    /// Exact host FP8 GEMV (`e4m3(w) * bf16(scale)` in f32) over the checkpoint bytes, the
    /// oracle the chip path is measured against in `own30b_check` and the fake for host tests.
    #[derive(Debug)]
    pub struct HostFp8 {
        pub experts: usize,
        pub n: usize,
        pub k: usize,
        pub w: Vec<u8>,
        pub scales: Vec<u16>,
    }

    impl Fp8Gemv for HostFp8 {
        fn experts(&self) -> usize {
            self.experts
        }
        fn n(&self) -> usize {
            self.n
        }
        fn k(&self) -> usize {
            self.k
        }
        fn grouped_f32(&self, ids: &[u32], x: &[f32], y: &mut [f32]) -> Result<u64, String> {
            let (n, k) = (self.n, self.k);
            let kb = k / FP8_BLOCK;
            let lut = fp8_table();
            assert!(x.len() == k || x.len() == ids.len() * k);
            assert_eq!(y.len(), ids.len() * n);
            for (g, &id) in ids.iter().enumerate() {
                let id = id as usize;
                assert!(id < self.experts);
                let xr = if x.len() == k { x } else { &x[g * k..(g + 1) * k] };
                let w = &self.w[id * n * k..(id + 1) * n * k];
                let s = &self.scales[id * (n / FP8_BLOCK) * kb..(id + 1) * (n / FP8_BLOCK) * kb];
                for r in 0..n {
                    let mut acc = 0.0f32;
                    for c in 0..k {
                        acc += lut[w[r * k + c] as usize] * bf16_to_f32(s[(r / FP8_BLOCK) * kb + c / FP8_BLOCK]) * xr[c];
                    }
                    y[g * n + r] = acc;
                }
            }
            Ok(0)
        }
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
        fn upload_fp8(&self, experts: usize, n: usize, k: usize, w: &[u8], scales: &[u16]) -> Result<Arc<dyn Fp8Gemv>, String> {
            assert_eq!(w.len(), experts * n * k);
            assert_eq!(scales.len(), experts * (n / FP8_BLOCK) * (k / FP8_BLOCK));
            self.uploads.lock().unwrap().push(("fp8", experts * n, k, w.len() + scales.len() * 4));
            Ok(Arc::new(HostFp8 { experts, n, k, w: w.to_vec(), scales: scales.to_vec() }))
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

// ===========================================================================
// Cut 3: the Stage A forward.
// ===========================================================================

use crate::qwen3_coder::rmsnorm_per_head;
use crate::tensor::{apply_rope, rmsnorm, scaled_dot_product_attention_single};
use crate::transformer_backend::{decode_sampling_seed, sample_with_params};
use aien_abi_core::SamplingParams;

/// The KV cache of one sequence plus the attention over it. Omega's paged bf16 pool in
/// production; a host f32 cache in tests.
pub trait KvAttention: Send {
    /// Tokens the cache can hold.
    fn capacity(&self) -> usize;
    fn reset(&mut self);
    /// Drops every position at or past `len` (the next append is at `len`). Prefix reuse.
    fn truncate(&mut self, len: usize);
    /// Stores the roped key and the value of position `pos` for `layer`.
    fn append(&mut self, layer: usize, pos: usize, k: &[f32], v: &[f32]) -> Result<(), String>;
    /// `out[q_heads x head_dim]` = attention of the (unscaled) query over positions `0..context_len`.
    fn attend(&mut self, layer: usize, q: &[f32], context_len: usize, out: &mut [f32]) -> Result<u64, String>;
    /// Whether the chip reads the pool in place (no per-call staging).
    fn resident(&self) -> bool;
}

/// Host f32 cache with the reference attention (tests, and the oracle side of parity runs).
pub struct HostKv {
    q_heads: usize,
    kv_heads: usize,
    head_dim: usize,
    capacity: usize,
    keys: Vec<Vec<Vec<f32>>>,
    values: Vec<Vec<Vec<f32>>>,
}

impl HostKv {
    pub fn new(config: &Qwen3MoeConfig, capacity: usize) -> Self {
        Self {
            q_heads: config.q_heads,
            kv_heads: config.kv_heads,
            head_dim: config.head_dim,
            capacity,
            keys: vec![Vec::new(); config.layers],
            values: vec![Vec::new(); config.layers],
        }
    }
}

impl KvAttention for HostKv {
    fn capacity(&self) -> usize {
        self.capacity
    }
    fn reset(&mut self) {
        for (k, v) in self.keys.iter_mut().zip(self.values.iter_mut()) {
            k.clear();
            v.clear();
        }
    }
    fn truncate(&mut self, len: usize) {
        for (k, v) in self.keys.iter_mut().zip(self.values.iter_mut()) {
            k.truncate(len);
            v.truncate(len);
        }
    }
    fn append(&mut self, layer: usize, pos: usize, k: &[f32], v: &[f32]) -> Result<(), String> {
        if pos != self.keys[layer].len() || pos >= self.capacity {
            return Err(format!("host kv: append at {pos}, have {} (capacity {})", self.keys[layer].len(), self.capacity));
        }
        self.keys[layer].push(k.to_vec());
        self.values[layer].push(v.to_vec());
        Ok(())
    }
    fn attend(&mut self, layer: usize, q: &[f32], context_len: usize, out: &mut [f32]) -> Result<u64, String> {
        scaled_dot_product_attention_single(
            q,
            &self.keys[layer][..context_len],
            &self.values[layer][..context_len],
            self.q_heads,
            self.kv_heads,
            self.head_dim,
            out,
        );
        Ok(0)
    }
    fn resident(&self) -> bool {
        false
    }
}

/// Paged KV block size (tokens): the size the 8B daemon runs (`gb10_serving`), a power of two.
pub const KV_BLOCK_SIZE: usize = 16;

/// Omega's paged bf16 KV: one sequence, blocks `0..n` in order, the pool in the engine's resident
/// allocation when the driver grants it (attention reads in place), a host-mapped pool otherwise.
pub struct OmegaPagedKv {
    pool: aien_kv_cache::UnifiedKvTensorPool,
    layout: aien_omega_gpu::OmegaGpuKvLayout,
    block_ids: Vec<u32>,
    capacity: usize,
    q_heads: usize,
    kv_heads: usize,
    head_dim: usize,
    resident: bool,
    filled: Vec<usize>,
}

impl OmegaPagedKv {
    pub fn new(config: &Qwen3MoeConfig, max_context: usize) -> Result<Self, String> {
        let num_blocks = max_context.div_ceil(KV_BLOCK_SIZE).max(1);
        let cfg = aien_kv_cache::KvPoolConfig::for_model(
            num_blocks,
            KV_BLOCK_SIZE,
            config.layers,
            config.kv_heads,
            config.head_dim,
            aien_kv_cache::KvDType::Bf16,
        );
        let bytes = cfg.layout()?.total_bytes;
        let (pool, resident) = match aien_omega_gpu::resident_kv_pool_alloc(bytes) {
            Ok((ptr, len)) => {
                // SAFETY: omega owns `len` bytes at `ptr` until `resident_kv_pool_free`, which the
                // release callback runs exactly once; chip address == host address for this memory.
                let buf = unsafe {
                    aien_kv_cache::external_default_buffer(ptr, len, ptr as u64, Box::new(aien_omega_gpu::resident_kv_pool_free))
                };
                match buf {
                    Ok(b) => (aien_kv_cache::UnifiedKvTensorPool::new(cfg.clone(), b)?, true),
                    Err(e) => {
                        aien_omega_gpu::resident_kv_pool_free();
                        return Err(e);
                    }
                }
            }
            Err(_) => (aien_kv_cache::UnifiedKvTensorPool::allocate(cfg.clone())?, false),
        };
        let d = pool.layout_desc();
        let layout = aien_omega_gpu::OmegaGpuKvLayout {
            block_stride_bytes: d.block_stride_bytes,
            layer_stride_bytes: d.layer_stride_bytes,
            kv_plane_stride_bytes: d.kv_plane_stride_bytes,
            token_stride_bytes: d.token_stride_bytes,
            head_stride_bytes: d.head_stride_bytes,
            pool_bytes: d.pool_bytes,
            num_blocks: d.num_blocks,
            num_layers: d.num_layers,
            block_size: d.block_size,
        };
        Ok(Self {
            pool,
            layout,
            block_ids: (0..num_blocks as u32).collect(),
            capacity: num_blocks * KV_BLOCK_SIZE,
            q_heads: config.q_heads,
            kv_heads: config.kv_heads,
            head_dim: config.head_dim,
            resident,
            filled: vec![0; config.layers],
        })
    }

    pub fn pool_bytes(&self) -> usize {
        self.pool.total_bytes()
    }
}

impl KvAttention for OmegaPagedKv {
    fn capacity(&self) -> usize {
        self.capacity
    }
    fn reset(&mut self) {
        self.filled.iter_mut().for_each(|f| *f = 0);
    }
    fn truncate(&mut self, len: usize) {
        // The pool keeps its bytes; positions past `len` are rewritten by later appends.
        for f in self.filled.iter_mut() {
            *f = (*f).min(len);
        }
    }
    fn append(&mut self, layer: usize, pos: usize, k: &[f32], v: &[f32]) -> Result<(), String> {
        if pos != self.filled[layer] || pos >= self.capacity {
            return Err(format!("paged kv: append at {pos}, have {} (capacity {})", self.filled[layer], self.capacity));
        }
        self.pool.write_token_kv(pos / KV_BLOCK_SIZE, layer, pos % KV_BLOCK_SIZE, k, v);
        self.filled[layer] = pos + 1;
        Ok(())
    }
    fn attend(&mut self, layer: usize, q: &[f32], context_len: usize, out: &mut [f32]) -> Result<u64, String> {
        if context_len > self.filled[layer] {
            return Err(format!("paged kv: attend over {context_len}, only {} stored", self.filled[layer]));
        }
        let nblocks = context_len.div_ceil(KV_BLOCK_SIZE);
        // SAFETY: the pool owns total_bytes() readable bytes at base_ptr() for its lifetime.
        let bytes = unsafe { std::slice::from_raw_parts(self.pool.base_ptr(), self.pool.total_bytes()) };
        let info = aien_omega_gpu::paged_attention_bf16(
            q,
            bytes,
            &self.layout,
            &self.block_ids[..nblocks],
            context_len,
            layer,
            self.q_heads,
            self.kv_heads,
            self.head_dim,
            out,
        )
        .map_err(|e| format!("paged attention layer {layer} ctx {context_len}: {e}; {}", aien_omega_gpu::last_error()))?;
        Ok(info.elapsed_ns)
    }
    fn resident(&self) -> bool {
        self.resident
    }
}

/// Generation request (field for field what the HTTP door's `GenerateParams` carries).
#[derive(Debug, Clone, PartialEq)]
pub struct GenerateParams {
    /// Hard cap on emitted tokens; stop tokens do not count.
    pub max_new_tokens: usize,
    /// `<= 0.001` means greedy.
    pub temperature: f32,
    /// `(0, 1]`; 1.0 disables nucleus filtering.
    pub top_p: f32,
    /// 0 disables.
    pub top_k: usize,
    /// Request seed; per-step seeds are `decode_sampling_seed(seed, step)`.
    pub seed: u64,
    pub stop_token_ids: Vec<u32>,
}

impl GenerateParams {
    pub fn greedy(max_new_tokens: usize, stop_token_ids: Vec<u32>) -> Self {
        Self {
            max_new_tokens,
            temperature: 0.0,
            top_p: 1.0,
            top_k: 0,
            seed: 0,
            stop_token_ids,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishReason {
    Stop,
    Length,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenerateOutcome {
    pub emitted_tokens: usize,
    pub finish: FinishReason,
    pub prefill_ms: u64,
    pub decode_ms: u64,
    pub prompt_tokens: usize,
    /// Prompt tokens whose KV was still valid from the previous generation (not re-prefilled).
    pub reused_prompt_tokens: usize,
    /// Sum of the per-step timings over the decode steps.
    pub decode_timing: StepTiming,
}

/// Where one step's time went. `*_ns` are host wall-clock sections; `chip_ns` is the sum of the
/// chip-reported kernel times; `launches` counts engine calls.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StepTiming {
    pub total_ns: u64,
    pub attention_ns: u64,
    pub experts_ns: u64,
    pub lm_head_ns: u64,
    pub host_ns: u64,
    pub chip_ns: u64,
    pub launches: u64,
}

impl StepTiming {
    pub fn add(&mut self, o: &StepTiming) {
        self.total_ns += o.total_ns;
        self.attention_ns += o.attention_ns;
        self.experts_ns += o.experts_ns;
        self.lm_head_ns += o.lm_head_ns;
        self.host_ns += o.host_ns;
        self.chip_ns += o.chip_ns;
        self.launches += o.launches;
    }
}

/// What one traced layer did for one token (the parity harness replays it on the host).
#[derive(Debug, Clone, PartialEq)]
pub struct LayerTrace {
    pub layer: usize,
    pub pos: usize,
    pub x_in: Vec<f32>,
    pub attn_proj: Vec<f32>,
    pub post: Vec<f32>,
    pub experts: Vec<u32>,
    pub weights: Vec<f32>,
    pub moe_out: Vec<f32>,
    pub x_out: Vec<f32>,
}

/// Samples like the HTTP door's `sample_next`: top-k mask, then the workspace sampler with a
/// per-step seed, so greedy and seeded replies match across engines.
pub fn sample_next(logits: &[f32], params: &GenerateParams, step: usize) -> u32 {
    let masked;
    let logits = if params.top_k > 0 && params.top_k < logits.len() && params.temperature > 0.001 {
        let mut order: Vec<usize> = (0..logits.len()).collect();
        order.sort_by(|&a, &b| logits[b].partial_cmp(&logits[a]).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b)));
        let mut out = vec![f32::NEG_INFINITY; logits.len()];
        for &i in &order[..params.top_k] {
            out[i] = logits[i];
        }
        masked = out;
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
    sample_with_params(logits, &sp, decode_sampling_seed(params.seed, step)).0
}

/// The Stage A engine: one sequence, token-at-a-time prefill and decode.
pub struct Qwen3MoeOmegaEngine {
    weights: Qwen3MoeOmegaWeights,
    kv: Box<dyn KvAttention>,
    /// Layers whose activations are recorded into `traces` (parity harness).
    pub trace_layers: Vec<usize>,
    pub traces: Vec<LayerTrace>,
    /// Negative control: every chosen expert id is replaced by its neighbour (weights stay), so
    /// every expert window is a wrong expert. The layer oracle must then disagree loudly.
    pub routing_mutant: bool,
    pub last_timing: StepTiming,
    pos: usize,
    /// Tokens whose KV is stored at positions `0..pos` (prefix reuse across generations).
    history: Vec<u32>,
}

impl Qwen3MoeOmegaEngine {
    pub fn new(weights: Qwen3MoeOmegaWeights, kv: Box<dyn KvAttention>) -> Self {
        Self {
            weights,
            kv,
            trace_layers: Vec::new(),
            traces: Vec::new(),
            routing_mutant: false,
            last_timing: StepTiming::default(),
            pos: 0,
            history: Vec::new(),
        }
    }

    /// The production constructor: loads the checkpoint onto Omega and allocates the paged KV.
    pub fn load_omega(dir: &Path, max_context: usize, progress: &mut dyn FnMut(&str)) -> Result<(Self, LoadReport), String> {
        Self::load_omega_with(dir, max_context, WeightFormat::Bf16, progress)
    }

    /// As [`Self::load_omega`] with a chosen weight format (FP8 needs omega's FP8 GEMV linked).
    pub fn load_omega_with(dir: &Path, max_context: usize, format: WeightFormat, progress: &mut dyn FnMut(&str)) -> Result<(Self, LoadReport), String> {
        if !aien_omega_gpu::is_native() {
            return Err("this binary carries the CPU stub of the Omega engine (build with AIEN_OMEGA_DIR or AIEN_OMEGA_GPU_LIB)".into());
        }
        let (weights, report) = load_qwen3_moe_omega_with(dir, &OmegaModelUploader, format, progress)?;
        let kv = OmegaPagedKv::new(&weights.config, max_context)?;
        progress(&format!("kv pool {} MB resident={}", kv.pool_bytes() >> 20, kv.resident()));
        Ok((Self::new(weights, Box::new(kv)), report))
    }

    pub fn config(&self) -> &Qwen3MoeConfig {
        &self.weights.config
    }

    pub fn weights(&self) -> &Qwen3MoeOmegaWeights {
        &self.weights
    }

    pub fn max_context_tokens(&self) -> usize {
        self.kv.capacity()
    }

    pub fn kv_resident(&self) -> bool {
        self.kv.resident()
    }

    /// Current sequence length (tokens already in the KV).
    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn reset(&mut self) {
        self.kv.reset();
        self.pos = 0;
        self.history.clear();
        self.traces.clear();
    }

    /// Tokens whose KV is stored (positions `0..position()`).
    pub fn history(&self) -> &[u32] {
        &self.history
    }

    /// Keeps the first `len` stored tokens and forgets the rest; the next step is at `len`.
    pub fn rewind(&mut self, len: usize) {
        let len = len.min(self.pos);
        self.kv.truncate(len);
        self.history.truncate(len);
        self.pos = len;
    }

    /// Runs `token` at the next position through every layer. Returns the final-normed hidden
    /// state (feed it to `logits`). Records `last_timing`.
    pub fn step(&mut self, token: u32) -> Result<Vec<f32>, String> {
        let t_step = Instant::now();
        let mut timing = StepTiming::default();
        let c = &self.weights.config;
        let (h, qd, kvd, hd, eps) = (c.hidden, c.q_dim(), c.kv_dim(), c.head_dim, c.rms_eps);
        let (e, top_k, i) = (c.experts, c.top_k, c.moe_intermediate);
        let pos = self.pos;
        if pos >= self.kv.capacity() {
            return Err(format!("context full: {pos} of {}", self.kv.capacity()));
        }
        let mut x = self.weights.embed_row(token);
        let mut x_norm = vec![0.0f32; h];
        let mut qkv_out = vec![0.0f32; c.qkv_n()];
        let mut qn = vec![0.0f32; qd];
        let mut kn = vec![0.0f32; kvd];
        let mut attn_out = vec![0.0f32; qd];
        let mut attn_proj = vec![0.0f32; h];
        let mut post = vec![0.0f32; h];
        let mut logits = vec![0.0f32; e];
        let mut gu_out = vec![0.0f32; top_k * 2 * i];
        let mut act = vec![0.0f32; i];
        let mut down_out = vec![0.0f32; h];
        let mut moe_out = vec![0.0f32; h];
        let mut col0s: Vec<u32> = vec![0; top_k];
        // FP8 grouped path: all top_k activations and down outputs in one buffer each.
        let mut act_all = vec![0.0f32; top_k * i];
        let mut down_all = vec![0.0f32; top_k * h];

        for (layer_idx, layer) in self.weights.layers.iter().enumerate() {
            let traced = self.trace_layers.contains(&layer_idx);
            let x_in = if traced { x.clone() } else { Vec::new() };

            // --- attention ---
            let t0 = Instant::now();
            rmsnorm(&x, &layer.input_norm, eps, &mut x_norm);
            timing.host_ns += t0.elapsed().as_nanos() as u64;
            let t1 = Instant::now();
            timing.chip_ns += match &layer.attn {
                AttnWeights::Bf16 { qkv, .. } => qkv.matmul_f32(1, &x_norm, &mut qkv_out),
                AttnWeights::Fp8 { qkv, .. } => qkv.grouped_f32(&[0], &x_norm, &mut qkv_out),
            }
            .map_err(|e| format!("layer {layer_idx} qkv: {e}"))?;
            timing.launches += 1;
            let t2 = Instant::now();
            rmsnorm_per_head(&qkv_out[..qd], &layer.q_norm, eps, c.q_heads, hd, &mut qn);
            rmsnorm_per_head(&qkv_out[qd..qd + kvd], &layer.k_norm, eps, c.kv_heads, hd, &mut kn);
            apply_rope(&mut qn, &mut kn, pos, c.q_heads, c.kv_heads, hd, c.rope_theta);
            self.kv.append(layer_idx, pos, &kn, &qkv_out[qd + kvd..])?;
            timing.host_ns += t2.elapsed().as_nanos() as u64;
            timing.chip_ns += self.kv.attend(layer_idx, &qn, pos + 1, &mut attn_out)?;
            timing.launches += 1;
            timing.chip_ns += match &layer.attn {
                AttnWeights::Bf16 { o, .. } => o.matmul_f32(1, &attn_out, &mut attn_proj),
                AttnWeights::Fp8 { o, .. } => o.grouped_f32(&[0], &attn_out, &mut attn_proj),
            }
            .map_err(|e| format!("layer {layer_idx} o: {e}"))?;
            timing.launches += 1;
            timing.attention_ns += t1.elapsed().as_nanos() as u64;

            // --- residual, post norm, routing (host) ---
            let t3 = Instant::now();
            for (xi, p) in x.iter_mut().zip(&attn_proj) {
                *xi += p;
            }
            rmsnorm(&x, &layer.post_norm, eps, &mut post);
            for (ex, l) in logits.iter_mut().enumerate() {
                *l = layer.router[ex * h..(ex + 1) * h].iter().zip(&post).map(|(a, b)| a * b).sum();
            }
            let plan = MoeBatchPlan::from_logits(&logits, 1, e, top_k).map_err(|e| format!("layer {layer_idx} routing: {e}"))?;
            let mut experts: Vec<u32> = plan.topk_experts.clone();
            if self.routing_mutant {
                experts.iter_mut().for_each(|ex| *ex = (*ex + 1) % e as u32);
            }
            for (slot, &ex) in experts.iter().enumerate() {
                col0s[slot] = c.gate_up_col0(ex as usize) as u32;
            }
            timing.host_ns += t3.elapsed().as_nanos() as u64;

            let t4 = Instant::now();
            moe_out.iter_mut().for_each(|v| *v = 0.0);
            match &layer.experts {
                // --- bf16 windows: 1 gate|up call over top_k windows, top_k down calls ---
                ExpertWeights::Bf16 { gate_up, down } => {
                    timing.chip_ns += gate_up
                        .windows_f32(1, &post, &col0s, 2 * i, &mut gu_out)
                        .map_err(|e| format!("layer {layer_idx} gate_up: {e}"))?;
                    timing.launches += top_k as u64;
                    for (slot, &ex) in experts.iter().enumerate() {
                        let th = Instant::now();
                        swiglu(&gu_out[slot * 2 * i..(slot + 1) * 2 * i], i, &mut act);
                        timing.host_ns += th.elapsed().as_nanos() as u64;
                        let col0 = c.down_col0(ex as usize) as u32;
                        timing.chip_ns += down
                            .windows_f32(1, &act, std::slice::from_ref(&col0), h, &mut down_out)
                            .map_err(|e| format!("layer {layer_idx} down expert {ex}: {e}"))?;
                        timing.launches += 1;
                        let w = plan.topk_weights[slot];
                        for (m, d) in moe_out.iter_mut().zip(&down_out) {
                            *m += w * d;
                        }
                    }
                }
                // --- FP8 grouped: one gate|up launch (shared row), one down launch (a row per expert) ---
                ExpertWeights::Fp8 { gate_up, down } => {
                    timing.chip_ns += gate_up
                        .grouped_f32(&experts, &post, &mut gu_out)
                        .map_err(|e| format!("layer {layer_idx} gate_up fp8: {e}"))?;
                    timing.launches += 1;
                    let th = Instant::now();
                    for slot in 0..top_k {
                        swiglu(&gu_out[slot * 2 * i..(slot + 1) * 2 * i], i, &mut act_all[slot * i..(slot + 1) * i]);
                    }
                    timing.host_ns += th.elapsed().as_nanos() as u64;
                    timing.chip_ns += down
                        .grouped_f32(&experts, &act_all, &mut down_all)
                        .map_err(|e| format!("layer {layer_idx} down fp8: {e}"))?;
                    timing.launches += 1;
                    let th = Instant::now();
                    for (slot, &w) in plan.topk_weights.iter().enumerate() {
                        for (m, d) in moe_out.iter_mut().zip(&down_all[slot * h..(slot + 1) * h]) {
                            *m += w * d;
                        }
                    }
                    timing.host_ns += th.elapsed().as_nanos() as u64;
                }
            }
            for (xi, m) in x.iter_mut().zip(&moe_out) {
                *xi += m;
            }
            timing.experts_ns += t4.elapsed().as_nanos() as u64;

            if traced {
                self.traces.push(LayerTrace {
                    layer: layer_idx,
                    pos,
                    x_in,
                    attn_proj: attn_proj.clone(),
                    post: post.clone(),
                    experts: experts.clone(),
                    weights: plan.topk_weights.clone(),
                    moe_out: moe_out.clone(),
                    x_out: x.clone(),
                });
            }
        }
        let t5 = Instant::now();
        let mut hidden = vec![0.0f32; h];
        rmsnorm(&x, &self.weights.final_norm, eps, &mut hidden);
        timing.host_ns += t5.elapsed().as_nanos() as u64;
        self.pos = pos + 1;
        self.history.push(token);
        timing.total_ns = t_step.elapsed().as_nanos() as u64;
        self.last_timing = timing;
        Ok(hidden)
    }

    /// Vocabulary logits of a final hidden state (one lm_head launch), added to `last_timing`.
    pub fn logits(&mut self, hidden: &[f32]) -> Result<Vec<f32>, String> {
        let t0 = Instant::now();
        let mut logits = vec![0.0f32; self.weights.config.vocab];
        let ns = self.weights.lm_head.matmul_f32(1, hidden, &mut logits).map_err(|e| format!("lm_head: {e}"))?;
        let dt = t0.elapsed().as_nanos() as u64;
        self.last_timing.lm_head_ns += dt;
        self.last_timing.total_ns += dt;
        self.last_timing.chip_ns += ns;
        self.last_timing.launches += 1;
        Ok(logits)
    }

    /// Prefill `prompt` then decode with `sampler(logits, step)`; `on_token` gets every emitted
    /// token and may return `false` to cancel. Stop ids are consumed, never emitted. The engine
    /// is reset first (one sequence at a time).
    pub fn generate_with(
        &mut self,
        prompt: &[u32],
        params: &GenerateParams,
        sampler: &mut dyn FnMut(&[f32], usize) -> u32,
        on_token: &mut dyn FnMut(u32) -> bool,
    ) -> Result<GenerateOutcome, String> {
        if prompt.is_empty() {
            return Err("empty prompt".into());
        }
        if prompt.len() >= self.kv.capacity() {
            return Err(format!("prompt of {} tokens, context is {}", prompt.len(), self.kv.capacity()));
        }
        // Prefix reuse: the KV of the longest common prefix with the stored history stays; at
        // least the last prompt token is always stepped so the logits come from this prompt.
        let common = self.history.iter().zip(prompt).take_while(|(a, b)| a == b).count();
        let reused = common.min(prompt.len() - 1);
        if reused > 0 {
            self.rewind(reused);
        } else {
            self.reset();
        }
        let t0 = Instant::now();
        let mut hidden = Vec::new();
        for &t in &prompt[reused..] {
            hidden = self.step(t)?;
        }
        let prefill_ms = t0.elapsed().as_millis() as u64;
        let t1 = Instant::now();
        let mut decode_timing = StepTiming::default();
        let mut emitted = 0usize;
        let mut finish = FinishReason::Length;
        let mut step = 0usize;
        loop {
            if emitted >= params.max_new_tokens {
                break;
            }
            let logits = self.logits(&hidden)?;
            decode_timing.add(&self.last_timing);
            let next = sampler(&logits, step);
            step += 1;
            if params.stop_token_ids.contains(&next) {
                finish = FinishReason::Stop;
                break;
            }
            emitted += 1;
            if !on_token(next) {
                finish = FinishReason::Cancelled;
                break;
            }
            if emitted >= params.max_new_tokens || self.pos >= self.kv.capacity() {
                break;
            }
            hidden = self.step(next)?;
        }
        Ok(GenerateOutcome {
            emitted_tokens: emitted,
            finish,
            prefill_ms,
            decode_ms: t1.elapsed().as_millis() as u64,
            prompt_tokens: prompt.len(),
            reused_prompt_tokens: reused,
            decode_timing,
        })
    }

    /// `generate_with` using [`sample_next`] (the door's sampling rule).
    pub fn generate(
        &mut self,
        prompt: &[u32],
        params: &GenerateParams,
        on_token: &mut dyn FnMut(u32) -> bool,
    ) -> Result<GenerateOutcome, String> {
        let p = params.clone();
        self.generate_with(prompt, params, &mut |logits, step| sample_next(logits, &p, step), on_token)
    }
}

/// The library entry for the HTTP door: `&self` generation behind one lock (one sequence at a
/// time, as the door's contract says). `Send + Sync`.
pub struct OmegaEngine {
    inner: Mutex<Qwen3MoeOmegaEngine>,
    max_context: usize,
    pub load_report: LoadReport,
}

impl OmegaEngine {
    /// Loads the checkpoint at `dir` onto Omega with a KV for `max_context` tokens.
    pub fn load(dir: &Path, max_context: usize, progress: &mut dyn FnMut(&str)) -> Result<Self, String> {
        Self::load_with(dir, max_context, WeightFormat::Bf16, progress)
    }

    /// As [`Self::load`] with a chosen [`WeightFormat`].
    pub fn load_with(dir: &Path, max_context: usize, format: WeightFormat, progress: &mut dyn FnMut(&str)) -> Result<Self, String> {
        let (engine, load_report) = Qwen3MoeOmegaEngine::load_omega_with(dir, max_context, format, progress)?;
        let max_context = engine.max_context_tokens();
        Ok(Self {
            inner: Mutex::new(engine),
            max_context,
            load_report,
        })
    }

    pub fn from_engine(engine: Qwen3MoeOmegaEngine, load_report: LoadReport) -> Self {
        let max_context = engine.max_context_tokens();
        Self {
            inner: Mutex::new(engine),
            max_context,
            load_report,
        }
    }

    pub fn kind(&self) -> &str {
        "omega"
    }

    pub fn max_context_tokens(&self) -> usize {
        self.max_context
    }

    pub fn generate(
        &self,
        prompt: &[u32],
        params: &GenerateParams,
        on_token: &mut dyn FnMut(u32) -> bool,
    ) -> Result<GenerateOutcome, String> {
        let mut g = self.inner.lock().map_err(|_| "engine lock poisoned".to_string())?;
        g.generate(prompt, params, on_token)
    }

    /// Same with the caller's sampler (the door passes its `sample_next`).
    pub fn generate_with(
        &self,
        prompt: &[u32],
        params: &GenerateParams,
        sampler: &mut dyn FnMut(&[f32], usize) -> u32,
        on_token: &mut dyn FnMut(u32) -> bool,
    ) -> Result<GenerateOutcome, String> {
        let mut g = self.inner.lock().map_err(|_| "engine lock poisoned".to_string())?;
        g.generate_with(prompt, params, sampler, on_token)
    }

    pub fn with_engine<R>(&self, f: impl FnOnce(&mut Qwen3MoeOmegaEngine) -> R) -> Result<R, String> {
        let mut g = self.inner.lock().map_err(|_| "engine lock poisoned".to_string())?;
        Ok(f(&mut g))
    }
}

#[cfg(test)]
mod forward_tests {
    use super::test_support::*;
    use super::*;
    use crate::qwen3_moe::parity_stats;

    /// Plain f32 forward over the dequantised weights (rounded to bf16 as uploaded), no engine
    /// code shared: embedding, GQA attention with q/k norms and rope, top-k routing, swiglu
    /// experts, lm_head. Returns the logits at every position.
    fn naive_forward(src: &MemorySource, c: &Qwen3MoeConfig, tokens: &[u32]) -> Vec<Vec<f32>> {
        let r = |v: f32| bf16_to_f32(f32_to_bf16(v));
        let bf = |name: &str| -> Vec<f32> {
            let (_, _, b) = &src.tensors[name];
            b.chunks_exact(2).map(|x| bf16_to_f32(u16::from_le_bytes([x[0], x[1]]))).collect()
        };
        let fp8 = |name: &str| -> Vec<f32> { src.fp8_f32(name).0.into_iter().map(r).collect() };
        let (h, qd, kvd, hd, i) = (c.hidden, c.q_dim(), c.kv_dim(), c.head_dim, c.moe_intermediate);
        let matvec = |x: &[f32], w: &[f32], out_dim: usize| -> Vec<f32> {
            let x: Vec<f32> = x.iter().map(|&v| r(v)).collect();
            (0..out_dim).map(|o| x.iter().zip(&w[o * x.len()..(o + 1) * x.len()]).map(|(a, b)| a * b).sum()).collect()
        };
        let embed = bf("model.embed_tokens.weight");
        let lm = bf("lm_head.weight");
        let fnorm = bf("model.norm.weight");
        let mut keys = vec![Vec::<Vec<f32>>::new(); c.layers];
        let mut vals = vec![Vec::<Vec<f32>>::new(); c.layers];
        let mut out = Vec::new();
        for (pos, &t) in tokens.iter().enumerate() {
            let mut x = embed[t as usize * h..(t as usize + 1) * h].to_vec();
            for l in 0..c.layers {
                let p = format!("model.layers.{l}");
                let mut xn = vec![0.0; h];
                rmsnorm(&x, &bf(&format!("{p}.input_layernorm.weight")), c.rms_eps, &mut xn);
                let q = matvec(&xn, &fp8(&format!("{p}.self_attn.q_proj.weight")), qd);
                let k = matvec(&xn, &fp8(&format!("{p}.self_attn.k_proj.weight")), kvd);
                let v = matvec(&xn, &fp8(&format!("{p}.self_attn.v_proj.weight")), kvd);
                let mut qn = vec![0.0; qd];
                let mut kn = vec![0.0; kvd];
                rmsnorm_per_head(&q, &bf(&format!("{p}.self_attn.q_norm.weight")), c.rms_eps, c.q_heads, hd, &mut qn);
                rmsnorm_per_head(&k, &bf(&format!("{p}.self_attn.k_norm.weight")), c.rms_eps, c.kv_heads, hd, &mut kn);
                apply_rope(&mut qn, &mut kn, pos, c.q_heads, c.kv_heads, hd, c.rope_theta);
                keys[l].push(kn.clone());
                vals[l].push(v);
                let mut att = vec![0.0; qd];
                scaled_dot_product_attention_single(&qn, &keys[l], &vals[l], c.q_heads, c.kv_heads, hd, &mut att);
                let o = matvec(&att, &fp8(&format!("{p}.self_attn.o_proj.weight")), h);
                for j in 0..h {
                    x[j] += o[j];
                }
                let mut post = vec![0.0; h];
                rmsnorm(&x, &bf(&format!("{p}.post_attention_layernorm.weight")), c.rms_eps, &mut post);
                let router = bf(&format!("{p}.mlp.gate.weight"));
                let logits: Vec<f32> = (0..c.experts).map(|e| post.iter().zip(&router[e * h..(e + 1) * h]).map(|(a, b)| a * b).sum()).collect();
                let plan = MoeBatchPlan::from_logits(&logits, 1, c.experts, c.top_k).unwrap();
                for slot in 0..c.top_k {
                    let e = plan.topk_experts[slot] as usize;
                    let b = format!("{p}.mlp.experts.{e}");
                    let g = matvec(&post, &fp8(&format!("{b}.gate_proj.weight")), i);
                    let u = matvec(&post, &fp8(&format!("{b}.up_proj.weight")), i);
                    let a: Vec<f32> = (0..i).map(|j| g[j] / (1.0 + (-g[j]).exp()) * u[j]).collect();
                    let d = matvec(&a, &fp8(&format!("{b}.down_proj.weight")), h);
                    for j in 0..h {
                        x[j] += plan.topk_weights[slot] * d[j];
                    }
                }
            }
            let mut hid = vec![0.0; h];
            rmsnorm(&x, &fnorm, c.rms_eps, &mut hid);
            out.push(matvec(&hid, &lm, c.vocab));
        }
        out
    }

    fn engine() -> (Qwen3MoeOmegaEngine, MemorySource, Qwen3MoeConfig) {
        let c = tiny_config();
        let mut src = MemorySource::for_config(&c);
        let up = RecordingUploader::default();
        let (w, _) = load_qwen3_moe_omega_from(&c, &mut src, &up, &mut |_| {}).unwrap();
        let kv = HostKv::new(&c, 32);
        (Qwen3MoeOmegaEngine::new(w, Box::new(kv)), src, c)
    }

    fn engine_with(format: WeightFormat) -> (Qwen3MoeOmegaEngine, MemorySource, Qwen3MoeConfig, LoadReport) {
        let c = tiny_config();
        let mut src = MemorySource::for_config(&c);
        let up = RecordingUploader::default();
        let (w, report) = load_qwen3_moe_omega_from_with(&c, &mut src, &up, format, &mut |_| {}).unwrap();
        let kv = HostKv::new(&c, 32);
        (Qwen3MoeOmegaEngine::new(w, Box::new(kv)), src, c, report)
    }

    #[test]
    fn fp8_paths_match_the_naive_reference_with_fewer_launches() {
        let tokens = [5u32, 17, 42, 3, 9, 60];
        for format in [WeightFormat::Fp8Experts, WeightFormat::Fp8All] {
            let (mut eng, src, c, report) = engine_with(format);
            assert_eq!(report.format, format);
            let want = naive_forward(&src, &c, &tokens);
            for (pos, &t) in tokens.iter().enumerate() {
                let hid = eng.step(t).unwrap();
                let got = eng.logits(&hid).unwrap();
                let s = parity_stats(&got, &want[pos]);
                assert!(s.cosine > 0.9999 && s.non_finite == 0, "{format:?} pos {pos}: {s:?}");
                assert_eq!(crate::tensor::sample_argmax(&got).0, crate::tensor::sample_argmax(&want[pos]).0, "{format:?} pos {pos}");
            }
            // Experts: 2 launches per layer instead of 2 * top_k; attention still 3.
            assert_eq!(eng.last_timing.launches as usize, c.layers * (3 + 2) + 1, "{format:?}");
            // Resident bytes: FP8 experts are 1 byte + f32 block scales, attention per format.
            let (h, i, e) = (c.hidden, c.moe_intermediate, c.experts);
            let experts = e * (2 * i * h + h * i) + e * 4 * (2 * (i / 128) * (h / 128) + (h / 128) * (i / 128));
            let attn = if format == WeightFormat::Fp8All {
                c.qkv_n() * h + 4 * (c.qkv_n() / 128) * (h / 128) + h * c.q_dim() + 4 * (h / 128) * (c.q_dim() / 128)
            } else {
                2 * (h * c.qkv_n() + c.q_dim() * h)
            };
            assert_eq!(report.resident_bytes, c.layers * (experts + attn) + 2 * h * c.vocab, "{format:?}");
        }
    }

    #[test]
    fn prefix_reuse_prefills_only_the_new_tokens_and_matches_a_cold_run() {
        let (mut cold, _, c, _) = engine_with(WeightFormat::Fp8All);
        let (mut warm, _, _, _) = engine_with(WeightFormat::Fp8All);
        let p = GenerateParams::greedy(4, vec![]);
        let turn1 = [5u32, 17, 42, 3, 9, 60];
        let mut reply1 = Vec::new();
        let o1 = warm.generate(&turn1, &p, &mut |t| { reply1.push(t); true }).unwrap();
        assert_eq!(o1.reused_prompt_tokens, 0);
        assert_eq!(warm.history().len(), turn1.len() + 3, "prompt plus every stepped reply token");
        // Turn 2 resends turn 1, the reply, and new user tokens (a chat app resending the thread).
        let mut turn2 = turn1.to_vec();
        turn2.extend_from_slice(&reply1);
        turn2.extend_from_slice(&[11, 23, 8]);
        let mut warm_out = Vec::new();
        let o2 = warm.generate(&turn2, &p, &mut |t| { warm_out.push(t); true }).unwrap();
        assert_eq!(o2.reused_prompt_tokens, turn1.len() + 3, "prompt + stepped reply tokens reused");
        let mut cold_out = Vec::new();
        let o3 = cold.generate(&turn2, &p, &mut |t| { cold_out.push(t); true }).unwrap();
        assert_eq!(o3.reused_prompt_tokens, 0);
        assert_eq!(warm_out, cold_out, "warm (prefix reused) and cold runs must agree token for token");
        assert_eq!(warm.position(), cold.position());
        // A prompt equal to the whole history still steps its last token (fresh logits).
        let hist = warm.history().to_vec();
        let o4 = warm.generate(&hist, &p, &mut |_| true).unwrap();
        assert_eq!(o4.reused_prompt_tokens, hist.len() - 1);
        // An unrelated prompt starts cold.
        let o5 = warm.generate(&[1, 2, 3], &p, &mut |_| true).unwrap();
        assert_eq!(o5.reused_prompt_tokens, 0);
        assert!(c.layers > 0);
    }

    #[test]
    fn fp8_experts_path_agrees_with_the_bf16_path_and_catches_the_routing_mutant() {
        let tokens = [7u32, 1, 33, 12];
        let (mut a, _, _, _) = engine_with(WeightFormat::Bf16);
        let (mut b, _, c, _) = engine_with(WeightFormat::Fp8Experts);
        a.trace_layers = vec![0];
        b.trace_layers = vec![0];
        for &t in &tokens {
            let ha = a.step(t).unwrap();
            let la = a.logits(&ha).unwrap();
            let hb = b.step(t).unwrap();
            let lb = b.logits(&hb).unwrap();
            let s = parity_stats(&lb, &la);
            assert!(s.cosine > 0.9999, "{s:?}");
        }
        for (ta, tb) in a.traces.iter().zip(&b.traces) {
            assert_eq!(ta.experts, tb.experts, "same routing at pos {}", ta.pos);
            let s = parity_stats(&tb.moe_out, &ta.moe_out);
            assert!(s.cosine > 0.9999, "moe_out pos {}: {s:?}", ta.pos);
        }
        let mut m = Qwen3MoeOmegaEngine::new(engine_with(WeightFormat::Fp8Experts).0.weights, Box::new(HostKv::new(&c, 32)));
        m.trace_layers = vec![0];
        m.routing_mutant = true;
        for &t in &tokens {
            m.step(t).unwrap();
        }
        let worst = m.traces.iter().zip(&b.traces).map(|(x, y)| parity_stats(&x.moe_out, &y.moe_out).cosine).fold(1.0, f64::min);
        assert!(worst < 0.99, "routing mutant must break the FP8 path too: {worst}");
    }

    #[test]
    fn engine_forward_matches_a_naive_reference_at_every_position() {
        let (mut eng, src, c) = engine();
        let tokens = [5u32, 17, 42, 3, 9, 60];
        let want = naive_forward(&src, &c, &tokens);
        for (pos, &t) in tokens.iter().enumerate() {
            let hid = eng.step(t).unwrap();
            let got = eng.logits(&hid).unwrap();
            let s = parity_stats(&got, &want[pos]);
            assert!(s.cosine > 0.9999 && s.non_finite == 0, "pos {pos}: {s:?}");
            assert_eq!(crate::tensor::sample_argmax(&got).0, crate::tensor::sample_argmax(&want[pos]).0, "pos {pos}");
        }
        assert_eq!(eng.position(), tokens.len());
        assert_eq!(eng.last_timing.launches as usize, c.layers * (3 + c.top_k + c.top_k) + 1);
    }

    #[test]
    fn routing_mutant_breaks_the_layer_and_the_trace_records_it() {
        let (mut eng, _, _) = engine();
        eng.trace_layers = vec![0];
        eng.step(5).unwrap();
        let good = eng.traces[0].clone();
        eng.reset();
        eng.routing_mutant = true;
        eng.step(5).unwrap();
        let bad = eng.traces[0].clone();
        assert_eq!(good.layer, 0);
        assert_eq!(good.post, bad.post, "attention is untouched by the routing mutant");
        assert_ne!(good.experts, bad.experts);
        let s = parity_stats(&bad.moe_out, &good.moe_out);
        assert!(s.cosine < 0.99, "mutant must be caught: {s:?}");
    }

    #[test]
    fn generate_stops_on_stop_ids_caps_length_and_cancels() {
        let (mut eng, src, c) = engine();
        let prompt = [5u32, 17, 42];
        let want = naive_forward(&src, &c, &prompt);
        let first = crate::tensor::sample_argmax(&want[2]).0;
        // Length cap.
        let mut got = Vec::new();
        let o = eng.generate(&prompt, &GenerateParams::greedy(3, vec![]), &mut |t| {
            got.push(t);
            true
        })
        .unwrap();
        assert_eq!(o.finish, FinishReason::Length);
        assert_eq!(o.emitted_tokens, 3);
        assert_eq!(got[0], first);
        assert_eq!(o.prompt_tokens, 3);
        assert_eq!(eng.position(), 3 + 2, "the last emitted token is not stepped");
        // Stop id: the first greedy token is a stop id, nothing emitted.
        let o = eng.generate(&prompt, &GenerateParams::greedy(3, vec![first]), &mut |_| true).unwrap();
        assert_eq!((o.finish, o.emitted_tokens), (FinishReason::Stop, 0));
        // Cancel after the first token.
        let o = eng.generate(&prompt, &GenerateParams::greedy(3, vec![]), &mut |_| false).unwrap();
        assert_eq!((o.finish, o.emitted_tokens), (FinishReason::Cancelled, 1));
        // The locked wrapper behaves the same and is Send + Sync.
        fn assert_sync<T: Send + Sync>(_: &T) {}
        let report = LoadReport {
            resident_bytes: 0,
            format: WeightFormat::Bf16,
            host_bytes: 0,
            peak_rss_bytes: None,
            peak_rss_before_bytes: None,
            seconds: 0.0,
            window_api: false,
        };
        let shared = OmegaEngine::from_engine(eng, report);
        assert_sync(&shared);
        let o = shared.generate(&prompt, &GenerateParams::greedy(2, vec![]), &mut |_| true).unwrap();
        assert_eq!(o.emitted_tokens, 2);
        assert_eq!(shared.kind(), "omega");
    }

    #[test]
    fn seeded_sampling_replays_and_greedy_is_argmax() {
        let logits: Vec<f32> = (0..50).map(|i| ((i * 7919) % 50) as f32 / 10.0).collect();
        let p = GenerateParams { max_new_tokens: 1, temperature: 0.7, top_p: 0.8, top_k: 20, seed: 42, stop_token_ids: vec![] };
        assert_eq!(sample_next(&logits, &p, 3), sample_next(&logits, &p, 3));
        let g = GenerateParams::greedy(1, vec![]);
        assert_eq!(sample_next(&logits, &g, 0), crate::tensor::sample_argmax(&logits).0);
        let mut seen = std::collections::HashSet::new();
        for step in 0..64 {
            seen.insert(sample_next(&logits, &p, step));
        }
        assert!(seen.len() > 1 && seen.len() <= 20, "{}", seen.len());
    }
}

// ===========================================================================
// Parity harness support: host replays of a traced layer.
// ===========================================================================

use crate::qwen3_coder::{bf16_words, CoderShardCache, Qwen3CoderAttention};
use crate::qwen3_moe::{parity_stats, reference_moe_forward, reference_router_logits, ParityStats, Qwen3MoeLayer};

/// One layer's attention weights decoded exactly (FP8 x bf16 scale) to f32 row-major, for the
/// host replay. About 75 MB per layer.
pub fn load_attention_layer_f32(dir: &Path, config: &Qwen3MoeConfig, layer: usize) -> Result<Qwen3CoderAttention, String> {
    let mut cache = CoderShardCache::open(dir).map_err(|e| e.to_string())?;
    let (h, qd, kvd, hd) = (config.hidden, config.q_dim(), config.kv_dim(), config.head_dim);
    let p = format!("model.layers.{layer}");
    let lut = fp8_table();
    let mut fp8_f32 = |name: &str, rows: usize, cols: usize| -> Result<Vec<f32>, String> {
        let w = cache.read_raw(name, "F8_E4M3", &[rows, cols], 1).map_err(|e| e.to_string())?;
        let s = cache
            .read_raw(&format!("{name}_scale_inv"), "BF16", &[rows / FP8_BLOCK, cols / FP8_BLOCK], 2)
            .map_err(|e| e.to_string())?;
        let s = bf16_words(&s);
        let kb = cols / FP8_BLOCK;
        Ok((0..rows * cols)
            .map(|idx| lut[w[idx] as usize] * bf16_to_f32(s[(idx / cols / FP8_BLOCK) * kb + (idx % cols) / FP8_BLOCK]))
            .collect())
    };
    let q_proj = fp8_f32(&format!("{p}.self_attn.q_proj.weight"), qd, h)?;
    let k_proj = fp8_f32(&format!("{p}.self_attn.k_proj.weight"), kvd, h)?;
    let v_proj = fp8_f32(&format!("{p}.self_attn.v_proj.weight"), kvd, h)?;
    let o_proj = fp8_f32(&format!("{p}.self_attn.o_proj.weight"), h, qd)?;
    let mut bf16_f32 = |name: &str, n: usize| -> Result<Vec<f32>, String> {
        let b = cache.read_raw(name, "BF16", &[n], 2).map_err(|e| e.to_string())?;
        Ok(bf16_words(&b).iter().map(|&x| bf16_to_f32(x)).collect())
    };
    Ok(Qwen3CoderAttention {
        q_proj,
        k_proj,
        v_proj,
        o_proj,
        q_norm: bf16_f32(&format!("{p}.self_attn.q_norm.weight"), hd)?,
        k_norm: bf16_f32(&format!("{p}.self_attn.k_norm.weight"), hd)?,
        input_norm: bf16_f32(&format!("{p}.input_layernorm.weight"), h)?,
        post_norm: bf16_f32(&format!("{p}.post_attention_layernorm.weight"), h)?,
    })
}

fn matvec_rows(x: &[f32], w: &[f32], out_dim: usize) -> Vec<f32> {
    let k = x.len();
    (0..out_dim)
        .into_par_iter()
        .map(|o| w[o * k..(o + 1) * k].iter().zip(x).map(|(a, b)| a * b).sum())
        .collect()
}

/// Host replay of one layer's attention over consecutive traces (positions 0..n of one
/// sequence) with exact f32 weights and an f32 KV: returns the `attn_proj` the host computes
/// for each trace, to compare with the chip's `trace.attn_proj`.
pub fn replay_attention(config: &Qwen3MoeConfig, attn: &Qwen3CoderAttention, traces: &[LayerTrace]) -> Vec<Vec<f32>> {
    let (h, qd, kvd, hd, eps) = (config.hidden, config.q_dim(), config.kv_dim(), config.head_dim, config.rms_eps);
    let mut keys: Vec<Vec<f32>> = Vec::new();
    let mut vals: Vec<Vec<f32>> = Vec::new();
    let mut out = Vec::new();
    for t in traces {
        let mut xn = vec![0.0; h];
        rmsnorm(&t.x_in, &attn.input_norm, eps, &mut xn);
        let q = matvec_rows(&xn, &attn.q_proj, qd);
        let k = matvec_rows(&xn, &attn.k_proj, kvd);
        let v = matvec_rows(&xn, &attn.v_proj, kvd);
        let mut qn = vec![0.0; qd];
        let mut kn = vec![0.0; kvd];
        rmsnorm_per_head(&q, &attn.q_norm, eps, config.q_heads, hd, &mut qn);
        rmsnorm_per_head(&k, &attn.k_norm, eps, config.kv_heads, hd, &mut kn);
        apply_rope(&mut qn, &mut kn, t.pos, config.q_heads, config.kv_heads, hd, config.rope_theta);
        keys.push(kn);
        vals.push(v);
        let mut att = vec![0.0; qd];
        scaled_dot_product_attention_single(&qn, &keys, &vals, config.q_heads, config.kv_heads, hd, &mut att);
        out.push(matvec_rows(&att, &attn.o_proj, h));
    }
    out
}

/// Host oracle of one traced MoE: routes `trace.post` with the capsule's router, runs the exact
/// FP8 oracle, and reports the parity of the chip's `moe_out` plus whether the chip's expert
/// choice equals the oracle's.
pub struct MoeParity {
    pub stats: ParityStats,
    pub oracle_experts: Vec<u32>,
    pub routing_agrees: bool,
}

pub fn replay_moe(layer: &Qwen3MoeLayer, trace: &LayerTrace, top_k: usize) -> MoeParity {
    let logits = reference_router_logits(layer, &trace.post, 1);
    let plan = MoeBatchPlan::from_logits(&logits, 1, layer.router_bf16().len() / layer.hidden, top_k).expect("plan");
    let want = reference_moe_forward(layer, &trace.post, &plan);
    let mut chip_sorted = trace.experts.clone();
    chip_sorted.sort_unstable();
    let mut oracle_sorted = plan.topk_experts.clone();
    oracle_sorted.sort_unstable();
    MoeParity {
        stats: parity_stats(&trace.moe_out, &want),
        routing_agrees: chip_sorted == oracle_sorted,
        oracle_experts: plan.topk_experts,
    }
}

/// SwiGLU of one expert's gate|up row: `act[j] = silu(gu[j]) * gu[i + j]`.
fn swiglu(gu: &[f32], i: usize, act: &mut [f32]) {
    for j in 0..i {
        let g = gu[j];
        act[j] = g / (1.0 + (-g).exp()) * gu[i + j];
    }
}

//! Rust side of `libqwencoder_serve.so`: resident-weight full-model serve.
//!
//! The library keeps every FP8/BF16 weight on device after one upload pass.
//! Per decode step only activation rows cross the boundary. CPU fallback is
//! always available; every device call is parity-checked against it.

use std::path::{Path, PathBuf};

use libloading::Library;

use crate::qwen3_coder::{
    bf16_words, rmsnorm_per_head, CoderShardCache, Qwen3CoderError, Qwen3CoderState,
    Qwen3CoderWeights, QWEN3_CODER_HEAD_DIM, QWEN3_CODER_HIDDEN, QWEN3_CODER_KV_DIM,
    QWEN3_CODER_KV_HEADS, QWEN3_CODER_LAYERS, QWEN3_CODER_Q_DIM, QWEN3_CODER_Q_HEADS,
    QWEN3_CODER_RMS_EPS, QWEN3_CODER_ROPE_THETA, QWEN3_CODER_VOCAB,
};
use crate::qwen3_moe::{bf16_to_f32, f32_to_bf16};
use crate::tensor::{apply_rope, rmsnorm, scaled_dot_product_attention_single};

type CreateFn = unsafe extern "C" fn(*mut u64) -> i32;
type FreeFn = unsafe extern "C" fn(u64) -> i32;
type MoeUploadFn =
    unsafe extern "C" fn(u64, i32, *const u16, *const u8, *const u16, *const u8, *const u16) -> i32;
type ProjUploadFn = unsafe extern "C" fn(u64, i32, i32, *const u8, *const u16) -> i32;
type LmUploadFn = unsafe extern "C" fn(u64, *const u16) -> i32;
type MoeForwardFn = unsafe extern "C" fn(u64, i32, *const u16, *mut u16) -> i32;
type Fp8GemvFn = unsafe extern "C" fn(u64, i32, i32, *const f32, *mut f32) -> i32;
type FusedQkvFn = unsafe extern "C" fn(u64, i32, *const f32, *mut f32, *mut f32, *mut f32) -> i32;
type Bf16GemvFn = unsafe extern "C" fn(u64, *const f32, *mut f32) -> i32;
type KvAppendFn = unsafe extern "C" fn(u64, i32, *const f32, *const f32, i32) -> i32;
type AttnFn = unsafe extern "C" fn(u64, i32, *const f32, *mut f32, i32) -> i32;

fn status(code: i32, what: &str) -> Result<(), Qwen3CoderError> {
    if code == 0 {
        Ok(())
    } else {
        Err(Qwen3CoderError::Contract(format!(
            "{what}: device status {code}"
        )))
    }
}

pub struct QwenServeLib {
    _lib: Library,
    create: CreateFn,
    free: FreeFn,
    moe_upload: MoeUploadFn,
    proj_upload: ProjUploadFn,
    lm_upload: LmUploadFn,
    moe_forward: MoeForwardFn,
    fp8_gemv: Fp8GemvFn,
    fused_qkv: FusedQkvFn,
    bf16_gemv: Bf16GemvFn,
    kv_append: KvAppendFn,
    attn: AttnFn,
    pub path: PathBuf,
}

impl QwenServeLib {
    /// `AIEN_QWENSERVE_LIB`, else the crate's `mojo/qwencoder_serve/libqwencoder_serve.so`.
    pub fn default_path() -> PathBuf {
        std::env::var_os("AIEN_QWENSERVE_LIB")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("mojo/qwencoder_serve/libqwencoder_serve.so")
            })
    }

    pub fn load(path: &Path) -> Result<Self, Qwen3CoderError> {
        let err =
            |e: libloading::Error| Qwen3CoderError::Contract(format!("{}: {e}", path.display()));
        // SAFETY: the library is built from the foundry seat and exports these C signatures.
        unsafe {
            let lib = Library::new(path).map_err(err)?;
            Ok(Self {
                create: *lib
                    .get::<CreateFn>(b"qwencoder_model_create\0")
                    .map_err(err)?,
                free: *lib.get::<FreeFn>(b"qwencoder_model_free\0").map_err(err)?,
                moe_upload: *lib
                    .get::<MoeUploadFn>(b"qwencoder_moe_upload\0")
                    .map_err(err)?,
                proj_upload: *lib
                    .get::<ProjUploadFn>(b"qwencoder_proj_upload\0")
                    .map_err(err)?,
                lm_upload: *lib
                    .get::<LmUploadFn>(b"qwencoder_lm_upload\0")
                    .map_err(err)?,
                moe_forward: *lib
                    .get::<MoeForwardFn>(b"qwencoder_moe_forward\0")
                    .map_err(err)?,
                fp8_gemv: *lib.get::<Fp8GemvFn>(b"qwencoder_fp8_gemv\0").map_err(err)?,
                fused_qkv: *lib
                    .get::<FusedQkvFn>(b"qwencoder_fused_qkv\0")
                    .map_err(err)?,
                bf16_gemv: *lib
                    .get::<Bf16GemvFn>(b"qwencoder_bf16_gemv\0")
                    .map_err(err)?,
                kv_append: *lib
                    .get::<KvAppendFn>(b"qwencoder_kv_append\0")
                    .map_err(err)?,
                attn: *lib.get::<AttnFn>(b"qwencoder_attn\0").map_err(err)?,
                _lib: lib,
                path: path.to_path_buf(),
            })
        }
    }
}

pub struct QwenServeModel {
    lib: QwenServeLib,
    handle: u64,
}

/// (rows, cols) per projection slot: 0 = q, 1 = k, 2 = v, 3 = o.
const PROJ_SHAPES: [(usize, usize); 4] = [
    (QWEN3_CODER_Q_DIM, QWEN3_CODER_HIDDEN),
    (QWEN3_CODER_KV_DIM, QWEN3_CODER_HIDDEN),
    (QWEN3_CODER_KV_DIM, QWEN3_CODER_HIDDEN),
    (QWEN3_CODER_HIDDEN, QWEN3_CODER_Q_DIM),
];
const PROJ_NAMES: [&str; 4] = ["q_proj", "k_proj", "v_proj", "o_proj"];

impl QwenServeModel {
    /// Creates the device model and uploads every weight. One-time cost
    /// (tens of GB); afterwards only activations cross the boundary.
    pub fn upload_all(
        lib: QwenServeLib,
        weights: &Qwen3CoderWeights,
        checkpoint_dir: &Path,
    ) -> Result<Self, Qwen3CoderError> {
        let this = Self::create(lib)?;
        this.upload_weights(weights, checkpoint_dir)?;
        Ok(this)
    }

    /// Allocates the device model without weights. Kernel-level checks (KV
    /// append, attention) run against this without the multi-minute upload.
    pub fn create(lib: QwenServeLib) -> Result<Self, Qwen3CoderError> {
        let mut handle = 0u64;
        // SAFETY: handle is written by the call.
        status(unsafe { (lib.create)(&mut handle) }, "model_create")?;
        Ok(Self { lib, handle })
    }

    fn upload_weights(
        &self,
        weights: &Qwen3CoderWeights,
        checkpoint_dir: &Path,
    ) -> Result<(), Qwen3CoderError> {
        let h = QWEN3_CODER_HIDDEN;
        // MoE layers straight from the already-fused capsules: no re-read.
        for (layer, moe) in weights.moes.iter().enumerate() {
            let l = layer as i32;
            // SAFETY: slices outlive the call; device copies synchronously.
            status(
                unsafe {
                    (self.lib.moe_upload)(
                        self.handle,
                        l,
                        moe.router_bf16().as_ptr(),
                        moe.gate_up_fp8().as_ptr(),
                        moe.gate_up_scale_inv_bf16().as_ptr(),
                        moe.down_fp8().as_ptr(),
                        moe.down_scale_inv_bf16().as_ptr(),
                    )
                },
                "moe_upload",
            )?;
        }
        // Attention projections + LM head re-read raw (loader decoded to f32).
        let mut cache = CoderShardCache::open(checkpoint_dir)?;
        for layer in 0..QWEN3_CODER_LAYERS {
            let p = format!("model.layers.{layer}.self_attn");
            for (slot, (name, (rows, cols))) in
                PROJ_NAMES.iter().zip(PROJ_SHAPES.iter()).enumerate()
            {
                let base = format!("{p}.{name}.weight");
                let w = cache.read_raw(&base, "F8_E4M3", &[*rows, *cols], 1)?;
                let s = cache.read_raw(
                    &format!("{base}_scale_inv"),
                    "BF16",
                    &[rows / 128, cols / 128],
                    2,
                )?;
                // SAFETY: buffers outlive the synchronous call.
                status(
                    unsafe {
                        (self.lib.proj_upload)(
                            self.handle,
                            layer as i32,
                            slot as i32,
                            w.as_ptr(),
                            bf16_words(&s).as_ptr(),
                        )
                    },
                    "proj_upload",
                )?;
            }
        }
        let lm = cache.read_raw("lm_head.weight", "BF16", &[QWEN3_CODER_VOCAB, h], 2)?;
        // SAFETY: buffer outlives the synchronous call.
        status(
            unsafe { (self.lib.lm_upload)(self.handle, bf16_words(&lm).as_ptr()) },
            "lm_upload",
        )?;
        Ok(())
    }

    /// One token's MoE sublayer on device. `post_bf16`/`out_bf16` are [2048].
    pub fn moe_layer(
        &self,
        layer: usize,
        post_bf16: &[u16],
        out_bf16: &mut [u16],
    ) -> Result<(), Qwen3CoderError> {
        assert_eq!(post_bf16.len(), QWEN3_CODER_HIDDEN);
        assert_eq!(out_bf16.len(), QWEN3_CODER_HIDDEN);
        // SAFETY: slices outlive the synchronous call.
        status(
            unsafe {
                (self.lib.moe_forward)(
                    self.handle,
                    layer as i32,
                    post_bf16.as_ptr(),
                    out_bf16.as_mut_ptr(),
                )
            },
            "moe_forward",
        )
    }

    /// Q/K/V projections in one device call (slots 0/1/2).
    pub fn fused_qkv(
        &self,
        layer: usize,
        vec: &[f32],
        q: &mut [f32],
        k: &mut [f32],
        v: &mut [f32],
    ) -> Result<(), Qwen3CoderError> {
        assert_eq!(vec.len(), QWEN3_CODER_HIDDEN);
        assert_eq!(q.len(), QWEN3_CODER_Q_DIM);
        assert_eq!(k.len(), QWEN3_CODER_KV_DIM);
        assert_eq!(v.len(), QWEN3_CODER_KV_DIM);
        // SAFETY: slices outlive the synchronous call.
        status(
            unsafe {
                (self.lib.fused_qkv)(
                    self.handle,
                    layer as i32,
                    vec.as_ptr(),
                    q.as_mut_ptr(),
                    k.as_mut_ptr(),
                    v.as_mut_ptr(),
                )
            },
            "fused_qkv",
        )
    }

    /// Attention output projection on device: [2048, 4096] x [4096] -> [2048].
    /// The device entry point implements only this shape (q/k/v go through
    /// `fused_qkv`), so slot 3 is the only one exposed.
    pub fn o_proj(
        &self,
        layer: usize,
        vec: &[f32],
        out: &mut [f32],
    ) -> Result<(), Qwen3CoderError> {
        const O_SLOT: usize = 3;
        let (rows, cols) = PROJ_SHAPES[O_SLOT];
        assert_eq!(vec.len(), cols);
        assert_eq!(out.len(), rows);
        let slot = O_SLOT;
        // SAFETY: slices outlive the synchronous call.
        status(
            unsafe {
                (self.lib.fp8_gemv)(
                    self.handle,
                    layer as i32,
                    slot as i32,
                    vec.as_ptr(),
                    out.as_mut_ptr(),
                )
            },
            "fp8_gemv",
        )
    }

    /// Appends one token's K/V rows to the layer's resident cache.
    pub fn kv_append(
        &self,
        layer: usize,
        k_row: &[f32],
        v_row: &[f32],
        pos: usize,
    ) -> Result<(), Qwen3CoderError> {
        assert_eq!(k_row.len(), QWEN3_CODER_KV_DIM);
        assert_eq!(v_row.len(), QWEN3_CODER_KV_DIM);
        // SAFETY: slices outlive the synchronous call.
        status(
            unsafe {
                (self.lib.kv_append)(
                    self.handle,
                    layer as i32,
                    k_row.as_ptr(),
                    v_row.as_ptr(),
                    pos as i32,
                )
            },
            "kv_append",
        )
    }

    /// One query token against the layer's resident KV.
    pub fn attn(
        &self,
        layer: usize,
        q_row: &[f32],
        out: &mut [f32],
        seq_len: usize,
    ) -> Result<(), Qwen3CoderError> {
        assert_eq!(q_row.len(), QWEN3_CODER_Q_DIM);
        assert_eq!(out.len(), QWEN3_CODER_Q_DIM);
        // SAFETY: slices outlive the synchronous call.
        status(
            unsafe {
                (self.lib.attn)(
                    self.handle,
                    layer as i32,
                    q_row.as_ptr(),
                    out.as_mut_ptr(),
                    seq_len as i32,
                )
            },
            "attn",
        )
    }

    /// Full-vocabulary logits on device.
    pub fn logits(&self, hidden: &[f32], out: &mut [f32]) -> Result<(), Qwen3CoderError> {
        assert_eq!(hidden.len(), QWEN3_CODER_HIDDEN);
        assert_eq!(out.len(), QWEN3_CODER_VOCAB);
        // SAFETY: slices outlive the synchronous call.
        status(
            unsafe { (self.lib.bf16_gemv)(self.handle, hidden.as_ptr(), out.as_mut_ptr()) },
            "bf16_gemv",
        )
    }
}

impl Drop for QwenServeModel {
    fn drop(&mut self) {
        // SAFETY: handle came from model_create.
        unsafe {
            (self.lib.free)(self.handle);
        }
    }
}

/// Full 48-layer forward with every GEMM on device. Norms, RoPE, attention
/// reduction, and sampling stay on CPU in Phase B1; q/k/v/o, MoE, and the LM
/// head run through the resident serve library.
///
/// When `profile` is set, per-section milliseconds accumulate into it instead
/// of being discarded: [proj_qkv, attn_cpu, proj_o, moe, norm_misc].
pub fn forward_token_serve(
    weights: &Qwen3CoderWeights,
    serve: &QwenServeModel,
    token_id: u32,
    pos: usize,
    state: &mut Qwen3CoderState,
    profile: Option<&mut [f64; 5]>,
    device_attn: bool,
) -> Result<Vec<f32>, Qwen3CoderError> {
    let h = QWEN3_CODER_HIDDEN;
    let qd = QWEN3_CODER_Q_DIM;
    let kvd = QWEN3_CODER_KV_DIM;
    let hd = QWEN3_CODER_HEAD_DIM;
    let eps = QWEN3_CODER_RMS_EPS;

    let token_idx = (token_id as usize) % QWEN3_CODER_VOCAB;
    let mut x = weights.embed[token_idx * h..(token_idx + 1) * h].to_vec();
    let mut prof = [0.0f64; 5];
    let want_prof = profile.is_some();

    for (layer_idx, attn) in weights.attns.iter().enumerate() {
        let kv = &mut state.layers[layer_idx];

        let t = std::time::Instant::now();
        let mut x_norm = vec![0.0f32; h];
        rmsnorm(&x, &attn.input_norm, eps, &mut x_norm);
        if want_prof {
            prof[4] += t.elapsed().as_secs_f64() * 1000.0;
        }

        let t = std::time::Instant::now();
        let mut q = vec![0.0f32; qd];
        let mut k = vec![0.0f32; kvd];
        let mut v = vec![0.0f32; kvd];
        serve.fused_qkv(layer_idx, &x_norm, &mut q, &mut k, &mut v)?;
        if want_prof {
            prof[0] += t.elapsed().as_secs_f64() * 1000.0;
        }

        let t = std::time::Instant::now();
        let mut qn = vec![0.0f32; qd];
        let mut kn = vec![0.0f32; kvd];
        rmsnorm_per_head(&q, &attn.q_norm, eps, QWEN3_CODER_Q_HEADS, hd, &mut qn);
        rmsnorm_per_head(&k, &attn.k_norm, eps, QWEN3_CODER_KV_HEADS, hd, &mut kn);
        apply_rope(
            &mut qn,
            &mut kn,
            pos,
            QWEN3_CODER_Q_HEADS,
            QWEN3_CODER_KV_HEADS,
            hd,
            QWEN3_CODER_ROPE_THETA,
        );

        let mut attn_out = vec![0.0f32; qd];
        // The device and CPU paths keep separate KV caches; mixing them within
        // one sequence would attend over a partial history without any error.
        let mixed = if device_attn {
            !kv.keys.is_empty()
        } else {
            kv.keys.len() != pos
        };
        if mixed {
            return Err(Qwen3CoderError::Contract(format!(
                "layer {layer_idx}: KV cache mode mismatch at pos {pos} \
                 (device_attn={device_attn}, cpu_kv_len={}); use one attention path per sequence",
                kv.keys.len()
            )));
        }
        if device_attn {
            serve.kv_append(layer_idx, &kn, &v, pos)?;
            serve.attn(layer_idx, &qn, &mut attn_out, pos + 1)?;
        } else {
            kv.keys.push(kn);
            kv.values.push(v);
            scaled_dot_product_attention_single(
                &qn,
                &kv.keys,
                &kv.values,
                QWEN3_CODER_Q_HEADS,
                QWEN3_CODER_KV_HEADS,
                hd,
                &mut attn_out,
            );
        }

        if want_prof {
            prof[1] += t.elapsed().as_secs_f64() * 1000.0;
        }

        let t = std::time::Instant::now();
        let mut attn_proj = vec![0.0f32; h];
        serve.o_proj(layer_idx, &attn_out, &mut attn_proj)?;
        if want_prof {
            prof[2] += t.elapsed().as_secs_f64() * 1000.0;
        }

        let t = std::time::Instant::now();
        for i in 0..h {
            x[i] += attn_proj[i];
        }

        let mut post = vec![0.0f32; h];
        rmsnorm(&x, &attn.post_norm, eps, &mut post);
        if want_prof {
            prof[4] += t.elapsed().as_secs_f64() * 1000.0;
        }

        let t = std::time::Instant::now();
        let post_bf16: Vec<u16> = post.iter().map(|&v| f32_to_bf16(v)).collect();
        let mut moe_bf16 = vec![0u16; h];
        serve.moe_layer(layer_idx, &post_bf16, &mut moe_bf16)?;
        if want_prof {
            prof[3] += t.elapsed().as_secs_f64() * 1000.0;
        }
        for i in 0..h {
            x[i] += bf16_to_f32(moe_bf16[i]);
        }
    }

    let t = std::time::Instant::now();
    let mut x_final = vec![0.0f32; h];
    rmsnorm(&x, &weights.final_norm, eps, &mut x_final);
    if want_prof {
        prof[4] += t.elapsed().as_secs_f64() * 1000.0;
    }
    if let Some(p) = profile {
        p.copy_from_slice(&prof);
    }
    Ok(x_final)
}

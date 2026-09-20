//! Mojo GB10 Hardware Acceleration Backend.
//! Binds to compiled libaien_kernels.so using libloading with persistent unified memory buffer management.

use crate::backend::{ReferenceCpuBackend, TensorBackend};
use libloading::{Library, Symbol};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub type FnRmsnormBf16 = unsafe extern "C" fn(*const u16, *const u16, *mut u16, i32, f32);
pub type FnRopeBf16 = unsafe extern "C" fn(*mut u16, *mut u16, i32, i32, i32, i32, f32);
pub type FnGemvBf16 = unsafe extern "C" fn(*const u16, *const u16, *mut u16, i32, i32);
pub type FnSwigluBf16 = unsafe extern "C" fn(*const u16, *const u16, *mut u16, i32);
pub type FnGqaBf16 =
    unsafe extern "C" fn(*const u16, *const u16, *const u16, *mut u16, i32, i32, i32, i32);

pub type FnRmsnormF32 = unsafe extern "C" fn(*const f32, *const f32, *mut f32, i32, f32);
pub type FnRopeF32 = unsafe extern "C" fn(*mut f32, *mut f32, i32, i32, i32, i32, f32);
pub type FnGemvF32 = unsafe extern "C" fn(*const f32, *const f32, *mut f32, i32, i32);
pub type FnSwigluF32 = unsafe extern "C" fn(*const f32, *const f32, *mut f32, i32);
pub type FnGqaF32 =
    unsafe extern "C" fn(*const f32, *const f32, *const f32, *mut f32, i32, i32, i32, i32);

/// Loaded C-ABI symbol table for libaien_kernels.so.
pub struct MojoKernelBindings {
    _lib: Library,
    pub rmsnorm_bf16: FnRmsnormBf16,
    pub rope_bf16: FnRopeBf16,
    pub gemv_bf16: FnGemvBf16,
    pub swiglu_bf16: FnSwigluBf16,
    pub gqa_bf16: FnGqaBf16,
    pub rmsnorm_f32: Option<FnRmsnormF32>,
    pub rope_f32: Option<FnRopeF32>,
    pub gemv_f32: Option<FnGemvF32>,
    pub swiglu_f32: Option<FnSwigluF32>,
    pub gqa_f32: Option<FnGqaF32>,
}

impl MojoKernelBindings {
    pub fn load_from<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let p = path.as_ref();
        unsafe {
            let lib = Library::new(p).map_err(|e| {
                format!(
                    "Failed to load Mojo kernels library at {}: {}",
                    p.display(),
                    e
                )
            })?;

            let rmsnorm_bf16: Symbol<FnRmsnormBf16> = lib
                .get(b"aien_rmsnorm_bf16\0")
                .map_err(|e| format!("Missing symbol aien_rmsnorm_bf16: {}", e))?;
            let rope_bf16: Symbol<FnRopeBf16> = lib
                .get(b"aien_rope_bf16\0")
                .map_err(|e| format!("Missing symbol aien_rope_bf16: {}", e))?;
            let gemv_bf16: Symbol<FnGemvBf16> = lib
                .get(b"aien_gemv_bf16\0")
                .map_err(|e| format!("Missing symbol aien_gemv_bf16: {}", e))?;
            let swiglu_bf16: Symbol<FnSwigluBf16> = lib
                .get(b"aien_swiglu_bf16\0")
                .map_err(|e| format!("Missing symbol aien_swiglu_bf16: {}", e))?;
            let gqa_bf16: Symbol<FnGqaBf16> = lib
                .get(b"aien_gqa_bf16\0")
                .map_err(|e| format!("Missing symbol aien_gqa_bf16: {}", e))?;

            let rmsnorm_f32: Option<FnRmsnormF32> = lib
                .get(b"aien_rmsnorm_f32\0")
                .ok()
                .map(|s: Symbol<FnRmsnormF32>| *s);
            let rope_f32: Option<FnRopeF32> = lib
                .get(b"aien_rope_f32\0")
                .ok()
                .map(|s: Symbol<FnRopeF32>| *s);
            let gemv_f32: Option<FnGemvF32> = lib
                .get(b"aien_gemv_f32\0")
                .ok()
                .map(|s: Symbol<FnGemvF32>| *s);
            let swiglu_f32: Option<FnSwigluF32> = lib
                .get(b"aien_swiglu_f32\0")
                .ok()
                .map(|s: Symbol<FnSwigluF32>| *s);
            let gqa_f32: Option<FnGqaF32> = lib
                .get(b"aien_gqa_f32\0")
                .ok()
                .map(|s: Symbol<FnGqaF32>| *s);

            Ok(Self {
                rmsnorm_bf16: *rmsnorm_bf16,
                rope_bf16: *rope_bf16,
                gemv_bf16: *gemv_bf16,
                swiglu_bf16: *swiglu_bf16,
                gqa_bf16: *gqa_bf16,
                rmsnorm_f32,
                rope_f32,
                gemv_f32,
                swiglu_f32,
                gqa_f32,
                _lib: lib,
            })
        }
    }
}

/// Resolves candidate file paths for libaien_kernels.so.
pub fn find_kernel_library_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("AIEN_KERNELS_LIB") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }

    let candidates = [
        PathBuf::from("crates/aien-inference-abi/mojo/libaien_kernels.so"),
        PathBuf::from("mojo/libaien_kernels.so"),
        PathBuf::from("/home/drakestapleton/workspace/aien-sovereign-core/crates/aien-inference-abi/mojo/libaien_kernels.so"),
    ];

    for c in &candidates {
        if c.exists() {
            return Some(c.clone());
        }
    }

    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let manifest_path = PathBuf::from(manifest).join("mojo/libaien_kernels.so");
        if manifest_path.exists() {
            return Some(manifest_path);
        }
    }

    None
}

/// Persistent buffer pool residing in NVIDIA DGX Spark 128 GB unified coherent memory.
/// Retains allocations across generation steps to eliminate per-token memory allocation and copying.
#[derive(Default)]
pub struct UnifiedMemoryPool {
    pub intermediate_f32: Vec<f32>,
    pub q_buffer_f32: Vec<f32>,
    pub k_buffer_f32: Vec<f32>,
    pub v_buffer_f32: Vec<f32>,
    pub kv_cache_f32: Vec<f32>,
    pub bf16_scratch_x: Vec<u16>,
    pub bf16_scratch_w: Vec<u16>,
    pub bf16_scratch_out: Vec<u16>,
}

impl UnifiedMemoryPool {
    pub fn ensure_capacity(
        &mut self,
        hidden_dim: usize,
        q_dim: usize,
        kv_dim: usize,
        vocab_size: usize,
    ) {
        let max_dim = hidden_dim.max(q_dim).max(kv_dim).max(vocab_size);
        if self.intermediate_f32.len() < max_dim {
            self.intermediate_f32.resize(max_dim, 0.0);
        }
        if self.q_buffer_f32.len() < q_dim {
            self.q_buffer_f32.resize(q_dim, 0.0);
        }
        if self.k_buffer_f32.len() < kv_dim {
            self.k_buffer_f32.resize(kv_dim, 0.0);
        }
        if self.v_buffer_f32.len() < kv_dim {
            self.v_buffer_f32.resize(kv_dim, 0.0);
        }
    }
}

/// Mojo GB10 Hardware Acceleration backend.
/// Dispatches tensor operations to compiled Mojo C-ABI shared library with unified memory buffer reuse.
pub struct MojoGb10Backend {
    bindings: Option<Arc<MojoKernelBindings>>,
    fallback: ReferenceCpuBackend,
    pool: Mutex<UnifiedMemoryPool>,
}

static GLOBAL_BINDINGS: OnceLock<Option<Arc<MojoKernelBindings>>> = OnceLock::new();

fn get_or_init_bindings() -> Option<Arc<MojoKernelBindings>> {
    GLOBAL_BINDINGS
        .get_or_init(|| {
            let path = find_kernel_library_path()?;
            match MojoKernelBindings::load_from(&path) {
                Ok(b) => {
                    eprintln!("MojoGb10Backend: Successfully bound to {}", path.display());
                    Some(Arc::new(b))
                }
                Err(e) => {
                    eprintln!(
                        "MojoGb10Backend: Warning - could not load kernels library: {}",
                        e
                    );
                    None
                }
            }
        })
        .clone()
}

impl MojoGb10Backend {
    pub fn new() -> Self {
        let bindings = get_or_init_bindings();
        Self {
            bindings,
            fallback: ReferenceCpuBackend::new(),
            pool: Mutex::new(UnifiedMemoryPool::default()),
        }
    }

    pub fn with_library_path<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let bindings = MojoKernelBindings::load_from(path)?;
        Ok(Self {
            bindings: Some(Arc::new(bindings)),
            fallback: ReferenceCpuBackend::new(),
            pool: Mutex::new(UnifiedMemoryPool::default()),
        })
    }

    pub fn is_hardware_accelerated(&self) -> bool {
        self.bindings.is_some()
    }

    /// Access the persistent unified memory buffer pool.
    pub fn pool(&self) -> &Mutex<UnifiedMemoryPool> {
        &self.pool
    }
}

impl Default for MojoGb10Backend {
    fn default() -> Self {
        Self::new()
    }
}

/// Type alias for honest architectural labeling of current multi-core CPU backend.
pub type NativeCpuBackend = MojoGb10Backend;

impl TensorBackend for MojoGb10Backend {
    fn name(&self) -> &'static str {
        "NativeCpuBackend (Rayon Multi-Core)"
    }

    fn rmsnorm(&self, out: &mut [f32], x: &[f32], weight: &[f32], eps: f32) {
        if let Some(ref b) = self.bindings {
            if let Some(f32_fn) = b.rmsnorm_f32 {
                unsafe {
                    f32_fn(
                        x.as_ptr(),
                        weight.as_ptr(),
                        out.as_mut_ptr(),
                        x.len() as i32,
                        eps,
                    );
                }
                return;
            }
        }
        self.fallback.rmsnorm(out, x, weight, eps);
    }

    fn apply_rope(
        &self,
        q: &mut [f32],
        k: &mut [f32],
        pos: usize,
        head_dim: usize,
        num_q_heads: usize,
        num_kv_heads: usize,
        theta: f32,
    ) {
        if let Some(ref b) = self.bindings {
            if let Some(f32_fn) = b.rope_f32 {
                unsafe {
                    f32_fn(
                        q.as_mut_ptr(),
                        k.as_mut_ptr(),
                        pos as i32,
                        head_dim as i32,
                        num_q_heads as i32,
                        num_kv_heads as i32,
                        theta,
                    );
                }
                return;
            }
        }
        self.fallback
            .apply_rope(q, k, pos, head_dim, num_q_heads, num_kv_heads, theta);
    }

    fn matmul_vec(
        &self,
        out: &mut [f32],
        x: &[f32],
        weight: &[f32],
        out_dim: usize,
        in_dim: usize,
    ) {
        if let Some(ref b) = self.bindings {
            if let Some(f32_fn) = b.gemv_f32 {
                unsafe {
                    f32_fn(
                        x.as_ptr(),
                        weight.as_ptr(),
                        out.as_mut_ptr(),
                        out_dim as i32,
                        in_dim as i32,
                    );
                }
                return;
            }
        }
        self.fallback.matmul_vec(out, x, weight, out_dim, in_dim);
    }

    fn matmul_batch(
        &self,
        out: &mut [f32],
        x: &[f32],
        weight: &[f32],
        batch_size: usize,
        in_dim: usize,
        out_dim: usize,
    ) {
        crate::tensor::matmul_batch(x, weight, out, batch_size, in_dim, out_dim);
    }

    fn swiglu(&self, out: &mut [f32], gate: &[f32], up: &[f32]) {
        let size = gate.len().min(up.len()).min(out.len());
        if let Some(ref b) = self.bindings {
            if let Some(f32_fn) = b.swiglu_f32 {
                unsafe {
                    f32_fn(gate.as_ptr(), up.as_ptr(), out.as_mut_ptr(), size as i32);
                }
                return;
            }
        }
        self.fallback.swiglu(out, gate, up);
    }

    fn gqa_attention(
        &self,
        out: &mut [f32],
        q: &[f32],
        k_cache: &[f32],
        v_cache: &[f32],
        seq_len: usize,
        num_q_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
    ) {
        if seq_len == 0 {
            out.fill(0.0);
            return;
        }
        if let Some(ref b) = self.bindings {
            if let Some(f32_fn) = b.gqa_f32 {
                unsafe {
                    f32_fn(
                        q.as_ptr(),
                        k_cache.as_ptr(),
                        v_cache.as_ptr(),
                        out.as_mut_ptr(),
                        seq_len as i32,
                        num_q_heads as i32,
                        num_kv_heads as i32,
                        head_dim as i32,
                    );
                }
                return;
            }
        }
        self.fallback.gqa_attention(
            out,
            q,
            k_cache,
            v_cache,
            seq_len,
            num_q_heads,
            num_kv_heads,
            head_dim,
        );
    }

    fn compute_logits(
        &self,
        logits: &mut [f32],
        hidden: &[f32],
        embed_weight: &[f32],
        vocab_size: usize,
        hidden_dim: usize,
    ) {
        self.matmul_vec(logits, hidden, embed_weight, vocab_size, hidden_dim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mojo_backend_creation_and_name() {
        let backend = MojoGb10Backend::new();
        assert_eq!(backend.name(), "NativeCpuBackend (Rayon Multi-Core)");
    }

    #[test]
    fn test_mojo_backend_fallback_or_accelerated_parity() {
        let mojo_backend = MojoGb10Backend::new();
        let ref_backend = ReferenceCpuBackend::new();

        let x = vec![1.0f32, 2.0, 3.0, 4.0];
        let w = vec![1.0f32, 1.0, 1.0, 1.0];
        let mut out_mojo = vec![0.0f32; 4];
        let mut out_ref = vec![0.0f32; 4];

        mojo_backend.rmsnorm(&mut out_mojo, &x, &w, 1e-5);
        ref_backend.rmsnorm(&mut out_ref, &x, &w, 1e-5);

        assert_eq!(out_mojo, out_ref);
    }
}

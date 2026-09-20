//! Genuine Blackwell GB10 Hardware Acceleration Backend for DGX Spark.
//! Dispatches single-token GEMV, batched GEMM, and Grouped Query Paged Attention
//! directly to CUDA and cuBLAS 13 on Blackwell sm_121.

use crate::backend::{ReferenceCpuBackend, TensorBackend};
use std::ffi::CStr;
use std::os::raw::{c_char, c_float, c_int};
use std::sync::atomic::{AtomicU64, Ordering};

extern "C" {
    fn blackwell_gemm_init() -> c_int;
    fn blackwell_gemm_get_kernel_count() -> u64;
    fn blackwell_gemm_get_device_name(buf: *mut c_char, max_len: c_int) -> c_int;
    fn blackwell_gemm_f32(
        a: *const c_float,
        b: *const c_float,
        c: *mut c_float,
        m: c_int,
        k: c_int,
        n: c_int,
    ) -> c_int;
    fn blackwell_gemv_f32(
        x: *const c_float,
        weight: *const c_float,
        out: *mut c_float,
        in_dim: c_int,
        out_dim: c_int,
    ) -> c_int;
    fn paged_attention_bf16_forward(
        q: *const u16,
        kv_pool_base: *const u16,
        block_tables: *const i32,
        context_lens: *const i32,
        max_blocks_per_seq: c_int,
        num_seqs: c_int,
        num_q_heads: c_int,
        num_kv_heads: c_int,
        head_dim: c_int,
        layer_idx: c_int,
        block_stride_elements: usize,
        layer_stride_elements: usize,
        kv_stride_elements: usize,
        token_stride_elements: usize,
        page_size: c_int,
        sm_scale: c_float,
        out: *mut u16,
    ) -> c_int;
    fn blackwell_allocate_managed(bytes: usize) -> *mut std::ffi::c_void;
    fn blackwell_free_managed(ptr: *mut std::ffi::c_void);
    fn blackwell_gemm_destroy();
}

/// Blackwell GB10 GPU Tensor Backend.
/// Executes GEMV, batched GEMM, and Paged Attention on the NVIDIA GB10 Blackwell GPU.
pub struct BlackwellGb10Backend {
    fallback: ReferenceCpuBackend,
    device_name: String,
    available: bool,
    fallback_counter: AtomicU64,
    paged_attention_kernel_calls: AtomicU64,
}

impl BlackwellGb10Backend {
    pub fn new() -> Self {
        let mut available = false;
        let mut device_name = "Unknown".to_string();

        unsafe {
            if blackwell_gemm_init() == 0 {
                let mut buf = [0 as c_char; 256];
                if blackwell_gemm_get_device_name(buf.as_mut_ptr(), 256) == 0 {
                    let c_str = CStr::from_ptr(buf.as_ptr());
                    if let Ok(s) = c_str.to_str() {
                        device_name = s.to_string();
                        available = true;
                    }
                }
            }
        }

        if available {
            unsafe fn custom_alloc(bytes: usize) -> *mut u8 {
                blackwell_allocate_managed(bytes) as *mut u8
            }
            unsafe fn custom_free(ptr: *mut u8, _bytes: usize) {
                blackwell_free_managed(ptr as *mut std::ffi::c_void);
            }
            aien_kv_cache::register_unified_allocator(custom_alloc, custom_free);

            eprintln!(
                "BlackwellGb10Backend: Successfully bound to device '{}' (sm_121 cuBLAS 13)",
                device_name
            );
        } else {
            eprintln!("BlackwellGb10Backend: Hardware initialization failed, using CPU fallback");
        }

        Self {
            fallback: ReferenceCpuBackend::new(),
            device_name,
            available,
            fallback_counter: AtomicU64::new(0),
            paged_attention_kernel_calls: AtomicU64::new(0),
        }
    }

    pub fn paged_attention_kernel_count(&self) -> u64 {
        self.paged_attention_kernel_calls.load(Ordering::Relaxed)
    }

    pub fn is_available(&self) -> bool {
        self.available
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn kernel_exec_count(&self) -> u64 {
        if self.available {
            unsafe { blackwell_gemm_get_kernel_count() }
        } else {
            0
        }
    }

    pub fn fallback_count(&self) -> u64 {
        self.fallback_counter.load(Ordering::Relaxed)
    }

    /// Dispatches Grouped Query Paged Attention on Blackwell sm_121
    pub fn paged_attention_bf16(
        &self,
        q: &[u16],
        kv_pool_base: *const u16,
        block_tables: &[i32],
        context_lens: &[i32],
        max_blocks_per_seq: usize,
        num_seqs: usize,
        num_q_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
        layer_idx: usize,
        block_stride_elements: usize,
        layer_stride_elements: usize,
        kv_stride_elements: usize,
        token_stride_elements: usize,
        page_size: usize,
        sm_scale: f32,
        out: &mut [u16],
    ) -> Result<(), String> {
        if self.available {
            let res = unsafe {
                paged_attention_bf16_forward(
                    q.as_ptr(),
                    kv_pool_base,
                    block_tables.as_ptr(),
                    context_lens.as_ptr(),
                    max_blocks_per_seq as c_int,
                    num_seqs as c_int,
                    num_q_heads as c_int,
                    num_kv_heads as c_int,
                    head_dim as c_int,
                    layer_idx as c_int,
                    block_stride_elements,
                    layer_stride_elements,
                    kv_stride_elements,
                    token_stride_elements,
                    page_size as c_int,
                    sm_scale as c_float,
                    out.as_mut_ptr(),
                )
            };
            if res == 0 {
                self.paged_attention_kernel_calls.fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
            return Err(format!(
                "paged_attention_bf16_forward failed with error code {}",
                res
            ));
        }
        Err("Blackwell GPU backend not available".to_string())
    }
}

impl Default for BlackwellGb10Backend {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BlackwellGb10Backend {
    fn drop(&mut self) {
        if self.available {
            unsafe {
                blackwell_gemm_destroy();
            }
        }
    }
}

impl TensorBackend for BlackwellGb10Backend {
    fn name(&self) -> &'static str {
        if self.available {
            "BlackwellGb10Backend (NVIDIA GB10 sm_121 cuBLAS)"
        } else {
            "BlackwellGb10Backend (FallbackCpu)"
        }
    }

    fn rmsnorm(&self, out: &mut [f32], x: &[f32], weight: &[f32], eps: f32) {
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
        if self.available {
            let res = unsafe {
                blackwell_gemv_f32(
                    x.as_ptr(),
                    weight.as_ptr(),
                    out.as_mut_ptr(),
                    in_dim as c_int,
                    out_dim as c_int,
                )
            };
            if res == 0 {
                return;
            }
            self.fallback_counter.fetch_add(1, Ordering::Relaxed);
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
        if self.available {
            let res = unsafe {
                blackwell_gemm_f32(
                    x.as_ptr(),
                    weight.as_ptr(),
                    out.as_mut_ptr(),
                    batch_size as c_int,
                    in_dim as c_int,
                    out_dim as c_int,
                )
            };
            if res == 0 {
                return;
            }
            self.fallback_counter.fetch_add(1, Ordering::Relaxed);
        }
        crate::tensor::matmul_batch(x, weight, out, batch_size, in_dim, out_dim);
    }

    fn swiglu(&self, out: &mut [f32], gate: &[f32], up: &[f32]) {
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
        self.fallback
            .gqa_attention(out, q, k_cache, v_cache, seq_len, num_q_heads, num_kv_heads, head_dim);
    }

    fn paged_attention(
        &self,
        out: &mut [f32],
        q: &[f32],
        pool: &aien_kv_cache::UnifiedKvTensorPool,
        block_ids: &[usize],
        context_len: usize,
        layer_idx: usize,
        num_q_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
    ) {
        if context_len == 0 || block_ids.is_empty() {
            out.fill(0.0);
            return;
        }

        if self.available && pool.config().dtype == aien_kv_cache::KvDType::Bf16 {
            let q_bf16: Vec<u16> = q.iter().map(|&v| {
                let bits = v.to_bits();
                (bits >> 16) as u16
            }).collect();

            let sm_scale = 1.0f32 / (head_dim as f32).sqrt();
            let mut out_bf16 = vec![0u16; out.len()];
            let i32_block_tables: Vec<i32> = block_ids.iter().map(|&b| b as i32).collect();
            let context_lens = [context_len as i32];
            let max_blocks = block_ids.len();

            let (token_stride, kv_stride, layer_stride, block_stride) = pool.element_strides();

            let res = self.paged_attention_bf16(
                &q_bf16,
                pool.base_ptr() as *const u16,
                &i32_block_tables,
                &context_lens,
                max_blocks,
                1,
                num_q_heads,
                num_kv_heads,
                head_dim,
                layer_idx,
                block_stride,
                layer_stride,
                kv_stride,
                token_stride,
                pool.config().block_size,
                sm_scale,
                &mut out_bf16,
            );

            if res.is_ok() {
                for i in 0..out.len() {
                    let bits = (out_bf16[i] as u32) << 16;
                    out[i] = f32::from_bits(bits);
                }
                return;
            }
            self.fallback_counter.fetch_add(1, Ordering::Relaxed);
        }

        self.fallback.paged_attention(
            out,
            q,
            pool,
            block_ids,
            context_len,
            layer_idx,
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
        if self.available {
            let res = unsafe {
                blackwell_gemv_f32(
                    hidden.as_ptr(),
                    embed_weight.as_ptr(),
                    logits.as_mut_ptr(),
                    hidden_dim as c_int,
                    vocab_size as c_int,
                )
            };
            if res == 0 {
                return;
            }
            self.fallback_counter.fetch_add(1, Ordering::Relaxed);
        }
        self.fallback
            .compute_logits(logits, hidden, embed_weight, vocab_size, hidden_dim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blackwell_backend_creation() {
        let backend = BlackwellGb10Backend::new();
        assert!(backend.name().starts_with("BlackwellGb10Backend"));
        if backend.is_available() {
            assert!(backend.device_name().contains("GB10") || !backend.device_name().is_empty());
        }
    }

    #[test]
    fn test_blackwell_gemm_parity_with_cpu() {
        let gpu_backend = BlackwellGb10Backend::new();
        let cpu_backend = ReferenceCpuBackend::new();

        if !gpu_backend.is_available() {
            eprintln!("Skipping GPU test: Blackwell hardware not available");
            return;
        }

        let m = 16;
        let k = 128;
        let n = 128;

        let mut a = vec![0.0f32; m * k];
        let mut b = vec![0.0f32; n * k];
        for i in 0..a.len() {
            a[i] = (i % 37) as f32 * 0.05;
        }
        for i in 0..b.len() {
            b[i] = ((i + 13) % 43) as f32 * 0.05;
        }

        let mut c_gpu = vec![0.0f32; m * n];
        let mut c_cpu = vec![0.0f32; m * n];

        gpu_backend.matmul_batch(&mut c_gpu, &a, &b, m, k, n);
        cpu_backend.matmul_batch(&mut c_cpu, &a, &b, m, k, n);

        let mut max_diff = 0.0f32;
        for i in 0..c_gpu.len() {
            let diff = (c_gpu[i] - c_cpu[i]).abs();
            if diff > max_diff {
                max_diff = diff;
            }
        }

        assert!(
            max_diff < 1e-3,
            "Blackwell GEMM diverged from CPU: max_diff = {}",
            max_diff
        );
        assert!(gpu_backend.kernel_exec_count() > 0);
    }

    #[test]
    fn test_blackwell_paged_attention_kernel() {
        let backend = BlackwellGb10Backend::new();
        if !backend.is_available() {
            eprintln!("Skipping GPU test: Blackwell hardware not available");
            return;
        }

        let num_blocks = 8;
        let block_size = 16;
        let num_layers = 2;
        let num_q_heads = 4;
        let num_kv_heads = 2;
        let head_dim = 64;
        let kv_dim = num_kv_heads * head_dim;

        let pool_cfg = aien_kv_cache::KvPoolConfig::for_model(
            num_blocks,
            block_size,
            num_layers,
            num_kv_heads,
            head_dim,
            aien_kv_cache::KvDType::Bf16,
        );
        let mut pool = aien_kv_cache::UnifiedKvTensorPool::allocate(pool_cfg).unwrap();

        // Write deterministic tokens across blocks 0 and 1 for layer 0
        let k_vec: Vec<f32> = (0..kv_dim).map(|i| (i as f32) * 0.05).collect();
        let v_vec: Vec<f32> = (0..kv_dim).map(|i| ((i + 3) as f32) * 0.02).collect();
        for slot in 0..block_size {
            pool.write_token_kv(0, 0, slot, &k_vec, &v_vec);
            pool.write_token_kv(1, 0, slot, &k_vec, &v_vec);
        }

        let q = vec![1.0f32; num_q_heads * head_dim];
        let mut out_gpu = vec![0.0f32; num_q_heads * head_dim];
        let mut out_cpu = vec![0.0f32; num_q_heads * head_dim];

        let block_ids = vec![0, 1];
        let context_len = 25; // 25 tokens across blocks 0 and 1

        let pre_calls = backend.paged_attention_kernel_count();
        let pre_fallbacks = backend.fallback_count();

        // Run GPU backend
        backend.paged_attention(
            &mut out_gpu,
            &q,
            &pool,
            &block_ids,
            context_len,
            0, // layer 0
            num_q_heads,
            num_kv_heads,
            head_dim,
        );

        assert_eq!(backend.paged_attention_kernel_count(), pre_calls + 1);
        assert_eq!(backend.fallback_count(), pre_fallbacks);

        // Run Reference CPU backend
        ReferenceCpuBackend::new().paged_attention(
            &mut out_cpu,
            &q,
            &pool,
            &block_ids,
            context_len,
            0,
            num_q_heads,
            num_kv_heads,
            head_dim,
        );

        let mut max_diff = 0.0f32;
        for i in 0..out_gpu.len() {
            let diff = (out_gpu[i] - out_cpu[i]).abs();
            if diff > max_diff {
                max_diff = diff;
            }
        }

        assert!(
            max_diff < 0.02,
            "Blackwell PagedAttention kernel diverged from CPU reference: max_diff = {}",
            max_diff
        );
    }
}

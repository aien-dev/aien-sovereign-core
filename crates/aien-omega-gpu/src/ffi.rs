//! Raw declarations, written by hand from omega `src/omega_gpu_matmul_api.h`
//! at the commit pinned in `omega.lock` (2636409). Nothing generated.
//! Includes the resident-weights handles added by omega FB-1 cut 1b.
use std::os::raw::{c_char, c_int};

pub const OMEGA_GPU_MATMUL_OK: c_int = 0;
pub const OMEGA_GPU_MATMUL_BAD_ARGS: c_int = -1;
pub const OMEGA_GPU_MATMUL_TOO_LARGE: c_int = -2;
pub const OMEGA_GPU_MATMUL_CODEGEN_FAIL: c_int = -3;
pub const OMEGA_GPU_MATMUL_CHIP_FAIL: c_int = -4;
pub const OMEGA_GPU_MATMUL_PARITY_FAIL: c_int = -5;

pub const OMEGA_GPU_MATMUL_TILE_M: u32 = 16;
pub const OMEGA_GPU_MATMUL_TILE_N: u32 = 8;
pub const OMEGA_GPU_MATMUL_TILE_K: u32 = 16;
pub const OMEGA_GPU_MATMUL_MAX_CTAS: u32 = 64;

/// Mirrors `OmegaGpuMatmulInfo` field for field, same order.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OmegaGpuMatmulInfo {
    pub m: u32,
    pub k: u32,
    pub n: u32,
    pub padded_m: u32,
    pub padded_n: u32,
    pub k_slices: u32,
    pub chip_calls: u32,
    pub rows_per_call: u32,
    pub elapsed_ns: u64,
    pub max_abs_err: f32,
    pub max_rel_err: f32,
    pub mismatch_count: usize,
    pub parity_verified: bool,
    pub completion_marker: u32,
    pub kernel_cache_hit: bool,
    pub target_chip: [c_char; 64],
    pub sm_architecture: u32,
    // cut 1b additions (appended in the C header, same order)
    pub grid_x: u32,
    pub grid_y: u32,
    pub padded_k: u32,
    pub resident: bool,
    pub oracle_ran: bool,
    pub call_ns: u64,
}

impl OmegaGpuMatmulInfo {
    pub fn zeroed() -> Self {
        Self {
            m: 0,
            k: 0,
            n: 0,
            padded_m: 0,
            padded_n: 0,
            k_slices: 0,
            chip_calls: 0,
            rows_per_call: 0,
            elapsed_ns: 0,
            max_abs_err: 0.0,
            max_rel_err: 0.0,
            mismatch_count: 0,
            parity_verified: false,
            completion_marker: 0,
            kernel_cache_hit: false,
            target_chip: [0; 64],
            sm_architecture: 0,
            grid_x: 0,
            grid_y: 0,
            padded_k: 0,
            resident: false,
            oracle_ran: false,
            call_ns: 0,
        }
    }
}

/// Opaque resident weight tensor (omega `OmegaGpuTensor`).
#[repr(C)]
pub struct OmegaGpuTensor {
    _private: [u8; 0],
}

#[cfg(has_omega_gpu)]
extern "C" {
    pub fn omega_gpu_matmul_bf16(
        m: u32,
        k: u32,
        n: u32,
        a: *const u16,
        b: *const u16,
        c: *mut f32,
        info: *mut OmegaGpuMatmulInfo,
    ) -> c_int;
    pub fn omega_gpu_matmul_f32(
        m: u32,
        k: u32,
        n: u32,
        a: *const f32,
        b: *const f32,
        c: *mut f32,
        info: *mut OmegaGpuMatmulInfo,
    ) -> c_int;
    pub fn omega_gpu_matmul_rc_name(rc: c_int) -> *const c_char;
    pub fn omega_gpu_matmul_cache_clear();
    pub fn omega_gpu_tensor_upload_bf16(
        k: u32,
        n: u32,
        b: *const u16,
        out: *mut *mut OmegaGpuTensor,
    ) -> c_int;
    pub fn omega_gpu_tensor_upload_f32(
        k: u32,
        n: u32,
        b: *const f32,
        out: *mut *mut OmegaGpuTensor,
    ) -> c_int;
    pub fn omega_gpu_tensor_free(t: *mut OmegaGpuTensor);
    pub fn omega_gpu_tensor_shape(t: *const OmegaGpuTensor, k: *mut u32, n: *mut u32);
    pub fn omega_gpu_matmul_resident_bf16(
        m: u32,
        a: *const u16,
        b: *const OmegaGpuTensor,
        c: *mut f32,
        info: *mut OmegaGpuMatmulInfo,
    ) -> c_int;
    pub fn omega_gpu_matmul_resident_f32(
        m: u32,
        a: *const f32,
        b: *const OmegaGpuTensor,
        c: *mut f32,
        info: *mut OmegaGpuMatmulInfo,
    ) -> c_int;
    pub fn omega_gpu_matmul_last_error() -> *const c_char;
    pub fn omega_gpu_matmul_is_blocked() -> c_int;
    pub fn omega_gpu_device_close();
}

// ---- omega `src/omega_gpu_elementwise_api.h` (FB-1 cut 4, pinned 2636409) ----
pub const OMEGA_GPU_EW_OK: c_int = 0;
pub const OMEGA_GPU_EW_BAD_ARGS: c_int = -1;
pub const OMEGA_GPU_EW_TOO_LARGE: c_int = -2;
pub const OMEGA_GPU_EW_CODEGEN_FAIL: c_int = -3;
pub const OMEGA_GPU_EW_CHIP_FAIL: c_int = -4;
pub const OMEGA_GPU_EW_UNWRITTEN: c_int = -5;

/// Mirrors `OmegaGpuEwInfo` field for field, same order.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OmegaGpuEwInfo {
    pub chip_calls: u32,
    pub elapsed_ns: u64,
    pub completion_marker: u32,
    pub unwritten_words: u32,
    pub kernel_cache_hit: bool,
    pub gpr_count: u32,
    pub insn_count: u32,
    pub threads_per_cta: u32,
    pub ctas_last_launch: u32,
    pub target_chip: [c_char; 64],
    pub sm_architecture: u32,
}

impl OmegaGpuEwInfo {
    pub fn zeroed() -> Self {
        Self {
            chip_calls: 0,
            elapsed_ns: 0,
            completion_marker: 0,
            unwritten_words: 0,
            kernel_cache_hit: false,
            gpr_count: 0,
            insn_count: 0,
            threads_per_cta: 0,
            ctas_last_launch: 0,
            target_chip: [0; 64],
            sm_architecture: 0,
        }
    }
}

#[cfg(has_omega_gpu)]
extern "C" {
    pub fn omega_gpu_rmsnorm_f32(
        rows: u32,
        dim: u32,
        x: *const f32,
        weight: *const f32,
        eps: f32,
        out: *mut f32,
        info: *mut OmegaGpuEwInfo,
    ) -> c_int;
    pub fn omega_gpu_rope_f32(
        heads: u32,
        head_dim: u32,
        v: *const f32,
        cos_half: *const f32,
        sin_half: *const f32,
        out: *mut f32,
        info: *mut OmegaGpuEwInfo,
    ) -> c_int;
    pub fn omega_gpu_swiglu_f32(
        n: u32,
        gate: *const f32,
        up: *const f32,
        out: *mut f32,
        info: *mut OmegaGpuEwInfo,
    ) -> c_int;
    pub fn omega_gpu_elementwise_rc_name(rc: c_int) -> *const c_char;
}

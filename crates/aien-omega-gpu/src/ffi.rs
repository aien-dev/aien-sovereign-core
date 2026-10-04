//! Raw declarations, written by hand from omega `src/omega_gpu_matmul_api.h`
//! at the commit pinned in `omega.lock` (d0ca8ce). Nothing generated.
//! Functions added by later omega cuts (resident-weights handles, FB-1 cut 1b)
//! get their own declarations here once they are on omega main and the lock moves.
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
        }
    }
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
}

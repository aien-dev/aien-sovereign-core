//! Safe Rust wrapper over omega's native GPU matmul (no CUDA).
//!
//! Built against the omega commit in `omega.lock`. With `cfg(has_omega_gpu)` the
//! real library is linked; otherwise (`AIEN_FORCE_CPU_STUB=1`, or no omega checkout
//! given) every chip call returns [`OmegaGpuError::Unavailable`], so CI passes
//! without the chip. Only the API on omega main at the pinned commit is wrapped.
//!
//! Conventions: row-major, `C[m x n] = A[m x k] * B[k x n]`, f32 output.
//! Inputs are bf16 (`matmul_bf16`) or f32 rounded to bf16 on the host (`matmul_f32`).
//! Limits (omega `omega_blackwell_matmul.h`: M, K, N <= 1024) are enforced by omega
//! and surface as `TooLarge`.
pub mod ffi;

use std::fmt;

pub use ffi::OmegaGpuMatmulInfo;

/// Omega commit this crate was built against (from `omega.lock`).
pub const PINNED_OMEGA_SHA: &str = env!("AIEN_OMEGA_PINNED_SHA");

/// True when the real native library is linked (not the stub).
pub const fn is_native() -> bool {
    cfg!(has_omega_gpu)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OmegaGpuError {
    /// Stub build: no native library linked.
    Unavailable,
    /// A slice length does not match the stated shape (caught before the C call).
    ShapeMismatch {
        what: &'static str,
        expected: usize,
        got: usize,
    },
    /// Omega returned a non-zero code; `name` is `omega_gpu_matmul_rc_name(rc)`.
    Rc { rc: i32, name: String },
}

impl fmt::Display for OmegaGpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => write!(f, "omega gpu unavailable (stub build)"),
            Self::ShapeMismatch {
                what,
                expected,
                got,
            } => write!(f, "{what}: expected {expected} elements, got {got}"),
            Self::Rc { rc, name } => write!(f, "omega_gpu_matmul rc={rc} ({name})"),
        }
    }
}

impl std::error::Error for OmegaGpuError {}

/// Name of a return code; the C function when native, the same table otherwise.
pub fn rc_name(rc: i32) -> String {
    #[cfg(has_omega_gpu)]
    {
        // SAFETY: returns a pointer to a static NUL-terminated string.
        let p = unsafe { ffi::omega_gpu_matmul_rc_name(rc) };
        if !p.is_null() {
            // SAFETY: non-null, static, NUL-terminated.
            return unsafe { std::ffi::CStr::from_ptr(p) }
                .to_string_lossy()
                .into_owned();
        }
    }
    match rc {
        ffi::OMEGA_GPU_MATMUL_OK => "OK",
        ffi::OMEGA_GPU_MATMUL_BAD_ARGS => "BAD_ARGS",
        ffi::OMEGA_GPU_MATMUL_TOO_LARGE => "TOO_LARGE",
        ffi::OMEGA_GPU_MATMUL_CODEGEN_FAIL => "CODEGEN_FAIL",
        ffi::OMEGA_GPU_MATMUL_CHIP_FAIL => "CHIP_FAIL",
        ffi::OMEGA_GPU_MATMUL_PARITY_FAIL => "PARITY_FAIL",
        _ => "UNKNOWN",
    }
    .to_string()
}

/// Per-call receipt from omega (copy of `OmegaGpuMatmulInfo`).
#[derive(Debug, Clone)]
pub struct MatmulInfo {
    pub raw: OmegaGpuMatmulInfo,
}

impl MatmulInfo {
    pub fn target_chip(&self) -> String {
        let bytes: Vec<u8> = self
            .raw
            .target_chip
            .iter()
            .take_while(|&&c| c != 0)
            .map(|c| c.to_ne_bytes()[0])
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

fn check_len(what: &'static str, expected: usize, got: usize) -> Result<(), OmegaGpuError> {
    if expected == got {
        Ok(())
    } else {
        Err(OmegaGpuError::ShapeMismatch {
            what,
            expected,
            got,
        })
    }
}

fn dims(m: usize, k: usize, n: usize) -> Result<(u32, u32, u32), OmegaGpuError> {
    let conv = |v: usize| {
        u32::try_from(v).map_err(|_| OmegaGpuError::Rc {
            rc: ffi::OMEGA_GPU_MATMUL_TOO_LARGE,
            name: rc_name(ffi::OMEGA_GPU_MATMUL_TOO_LARGE),
        })
    };
    Ok((conv(m)?, conv(k)?, conv(n)?))
}

#[cfg(has_omega_gpu)]
fn finish(rc: i32, info: OmegaGpuMatmulInfo) -> Result<MatmulInfo, OmegaGpuError> {
    if rc == ffi::OMEGA_GPU_MATMUL_OK {
        Ok(MatmulInfo { raw: info })
    } else {
        Err(OmegaGpuError::Rc {
            rc,
            name: rc_name(rc),
        })
    }
}

/// `c = a * b`, bf16 inputs (raw bits), f32 output.
pub fn matmul_bf16(
    m: usize,
    k: usize,
    n: usize,
    a: &[u16],
    b: &[u16],
    c: &mut [f32],
) -> Result<MatmulInfo, OmegaGpuError> {
    check_len("a", m * k, a.len())?;
    check_len("b", k * n, b.len())?;
    check_len("c", m * n, c.len())?;
    let (mu, ku, nu) = dims(m, k, n)?;
    #[cfg(has_omega_gpu)]
    {
        let mut info = OmegaGpuMatmulInfo::zeroed();
        // SAFETY: lengths checked above match m*k, k*n, m*n; pointers are valid
        // for the call; info is a valid out-pointer.
        let rc = unsafe {
            ffi::omega_gpu_matmul_bf16(
                mu,
                ku,
                nu,
                a.as_ptr(),
                b.as_ptr(),
                c.as_mut_ptr(),
                &mut info,
            )
        };
        finish(rc, info)
    }
    #[cfg(not(has_omega_gpu))]
    {
        let _ = (mu, ku, nu);
        Err(OmegaGpuError::Unavailable)
    }
}

/// `c = a * b`, f32 inputs rounded to bf16 on the host, f32 output.
pub fn matmul_f32(
    m: usize,
    k: usize,
    n: usize,
    a: &[f32],
    b: &[f32],
    c: &mut [f32],
) -> Result<MatmulInfo, OmegaGpuError> {
    check_len("a", m * k, a.len())?;
    check_len("b", k * n, b.len())?;
    check_len("c", m * n, c.len())?;
    let (mu, ku, nu) = dims(m, k, n)?;
    #[cfg(has_omega_gpu)]
    {
        let mut info = OmegaGpuMatmulInfo::zeroed();
        // SAFETY: as in matmul_bf16.
        let rc = unsafe {
            ffi::omega_gpu_matmul_f32(
                mu,
                ku,
                nu,
                a.as_ptr(),
                b.as_ptr(),
                c.as_mut_ptr(),
                &mut info,
            )
        };
        finish(rc, info)
    }
    #[cfg(not(has_omega_gpu))]
    {
        let _ = (mu, ku, nu);
        Err(OmegaGpuError::Unavailable)
    }
}

/// Drop every cached kernel (tests; otherwise never needed). No-op in the stub.
pub fn cache_clear() {
    #[cfg(has_omega_gpu)]
    // SAFETY: takes no arguments; omega guards its cache with a mutex.
    unsafe {
        ffi::omega_gpu_matmul_cache_clear()
    }
}

//! Safe binding to omega COMPOSITION-2 through the rxc_host ABI
//! (`src/runtime/rxc_host_abi.h`, linked from `librx_compose.a`).
//!
//! One goal = one `Compose::run`: the registered Skills compete on staged
//! J-Space branches, the AEGIS verifier checks each result against the
//! contract (the `set_verify` callback), the winner is committed to the World
//! and the whole composition is recorded in the Cortex journal
//! `<dir>/cortex.cx`. Records and their digests survive a restart; the
//! directory is bound to one AienMachineId.
//!
//! Without the library (stub build, see build.rs) every constructor returns
//! [`ComposeError::Unavailable`] and [`LINKED`] is false.
pub mod ffi;

use std::fmt;
use std::path::Path;

/// True when `librx_compose.a` is linked (not the stub).
pub const LINKED: bool = cfg!(has_omega_compose);
/// The omega commit this build expected (omega.lock or `AIEN_OMEGA_COMPOSE_SHA`).
pub const EXPECTED_OMEGA_SHA: &str = env!("AIEN_OMEGA_COMPOSE_EXPECTED_SHA");
pub use ffi::{RXC_HOST_SUBJECT_GOAL as SUBJECT_GOAL, RXC_HOST_SUBJECT_STATE as SUBJECT_STATE};
/// No winner / no Skill.
pub const NONE: u32 = ffi::RXC_HOST_NONE;

/// Composition outcomes (`RXC_HOST_OUT_*`).
pub const OUT_COMMITTED: i32 = 1;
pub const OUT_NO_WINNER: i32 = 2;
pub const OUT_NOT_COMMITTED: i32 = 3;
pub const OUT_NOT_DURABLE: i32 = 4;
pub const OUT_RECORD_FAILED: i32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    /// 1..4096 opaque bytes generated once at provisioning.
    Provisioned = 1,
    /// The 32-byte digest of the owner-enrolled key.
    Hardware = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeError {
    /// The library is not linked (stub build).
    Unavailable,
    /// Rust and C struct layouts differ (wrong omega commit).
    Layout { c: [u32; 3], rust: [u32; 3] },
    /// An argument Rust refuses before calling C (interior NUL, empty name).
    Arg(String),
    /// `RXC_HOST_E_*` from the C side, with the underlying rx code when known.
    Code { code: i32, detail: i32 },
}

impl ComposeError {
    pub fn name(code: i32) -> &'static str {
        match code {
            ffi::RXC_HOST_E_ARG => "E_ARG",
            ffi::RXC_HOST_E_IDENTITY => "E_IDENTITY",
            ffi::RXC_HOST_E_TORN => "E_TORN",
            ffi::RXC_HOST_E_OPEN => "E_OPEN",
            ffi::RXC_HOST_E_FULL => "E_FULL",
            ffi::RXC_HOST_E_STATE => "E_STATE",
            ffi::RXC_HOST_E_RUN => "E_RUN",
            ffi::RXC_HOST_E_NOMEM => "E_NOMEM",
            ffi::RXC_HOST_E_NOT_FOUND => "E_NOT_FOUND",
            ffi::RXC_HOST_E_DIGEST => "E_DIGEST",
            _ => "E_UNKNOWN",
        }
    }
}

impl fmt::Display for ComposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComposeError::Unavailable => write!(f, "librx_compose.a is not linked (stub build)"),
            ComposeError::Layout { c, rust } => {
                write!(f, "rxc_host ABI layout mismatch: C {c:?} vs Rust {rust:?}")
            }
            ComposeError::Arg(s) => write!(f, "invalid argument: {s}"),
            ComposeError::Code { code, detail } => {
                write!(
                    f,
                    "rxc_host {} ({code}), detail {detail}",
                    Self::name(*code)
                )
            }
        }
    }
}

impl std::error::Error for ComposeError {}

pub type Info = ffi::RxcHostInfo;
pub type RunResult = ffi::RxcHostResult;
pub type Record = ffi::RxcHostRecord;

/// Lower-case hex of a byte slice.
pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

type SkillFn = dyn Fn(u64) -> Option<u64> + Send + Sync;
type VerifyFn = dyn Fn(u64, u64) -> bool + Send + Sync;

/// An open composition home. Calls take `&mut self`: one caller at a time.
/// Skill and verify callbacks run on omega World worker threads (two Skills
/// may run at once), hence `Send + Sync`.
pub struct Compose {
    #[cfg_attr(not(has_omega_compose), allow(dead_code))]
    h: *mut ffi::RxcHost,
    // Boxed so the ctx pointers handed to C stay put; dropped after close.
    #[allow(clippy::vec_box, dead_code)]
    skills: Vec<Box<Box<SkillFn>>>,
    #[allow(dead_code)]
    verify: Option<Box<Box<VerifyFn>>>,
}

// SAFETY: the handle is only used through &mut self (one thread at a time);
// the C side's own worker threads only call the Send + Sync callbacks.
unsafe impl Send for Compose {}

#[cfg(has_omega_compose)]
mod linked {
    use super::*;
    use std::ffi::CString;
    use std::os::raw::{c_int, c_void};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    fn check(rc: c_int, detail: i32) -> Result<(), ComposeError> {
        if rc == ffi::RXC_HOST_OK {
            Ok(())
        } else {
            Err(ComposeError::Code { code: rc, detail })
        }
    }

    unsafe extern "C" fn skill_tramp(ctx: *mut c_void, task: u64, result: *mut u64) -> c_int {
        let f = &*(ctx as *const Box<SkillFn>);
        match catch_unwind(AssertUnwindSafe(|| f(task))) {
            Ok(Some(r)) if r != 0 => {
                *result = r;
                0
            }
            _ => 1,
        }
    }

    unsafe extern "C" fn verify_tramp(ctx: *mut c_void, task: u64, result: u64) -> c_int {
        let f = &*(ctx as *const Box<VerifyFn>);
        matches!(catch_unwind(AssertUnwindSafe(|| f(task, result))), Ok(true)) as c_int
    }

    fn layout() -> Result<(), ComposeError> {
        let mut c = [0u32; 3];
        // SAFETY: writes exactly three u32.
        let v = unsafe { ffi::rxc_host_abi_layout(c.as_mut_ptr()) };
        let rust = [
            std::mem::size_of::<ffi::RxcHostInfo>() as u32,
            std::mem::size_of::<ffi::RxcHostResult>() as u32,
            std::mem::size_of::<ffi::RxcHostRecord>() as u32,
        ];
        if v != ffi::RXC_HOST_ABI_VERSION || c != rust {
            return Err(ComposeError::Layout { c, rust });
        }
        Ok(())
    }

    impl Compose {
        pub fn open(
            dir: &Path,
            root_kind: RootKind,
            root: &[u8],
            session: u64,
            refuse_torn: bool,
        ) -> Result<(Self, Info), ComposeError> {
            layout()?;
            use std::os::unix::ffi::OsStrExt;
            let d = CString::new(dir.as_os_str().as_bytes())
                .map_err(|_| ComposeError::Arg("dir holds a NUL byte".into()))?;
            let mut h = std::ptr::null_mut();
            let mut info = Info::default();
            let flags = if refuse_torn {
                ffi::RXC_HOST_OPEN_REFUSE_TORN
            } else {
                0
            };
            // SAFETY: valid C string, root slice, out pointers.
            let rc = unsafe {
                ffi::rxc_host_open(
                    d.as_ptr(),
                    root_kind as u32,
                    root.as_ptr(),
                    root.len(),
                    session,
                    flags,
                    &mut h,
                    &mut info,
                )
            };
            check(rc, info.open_rc)?;
            Ok((
                Compose {
                    h,
                    skills: Vec::new(),
                    verify: None,
                },
                info,
            ))
        }

        pub fn register_skill<F>(
            &mut self,
            name: &str,
            digest: Option<[u8; 32]>,
            cost: u64,
            f: F,
        ) -> Result<u32, ComposeError>
        where
            F: Fn(u64) -> Option<u64> + Send + Sync + 'static,
        {
            let n = CString::new(name)
                .map_err(|_| ComposeError::Arg("name holds a NUL byte".into()))?;
            let b: Box<Box<SkillFn>> = Box::new(Box::new(f));
            let ctx = &*b as *const Box<SkillFn> as *mut c_void;
            let dp = digest.as_ref().map_or(std::ptr::null(), |d| d.as_ptr());
            // SAFETY: ctx outlives the handle (dropped after close in Drop).
            let rc = unsafe {
                ffi::rxc_host_register_skill(self.h, n.as_ptr(), dp, cost, skill_tramp, ctx)
            };
            if rc < 0 {
                return Err(ComposeError::Code {
                    code: rc,
                    detail: 0,
                });
            }
            self.skills.push(b);
            Ok(rc as u32)
        }

        pub fn set_verify<F>(&mut self, f: F) -> Result<(), ComposeError>
        where
            F: Fn(u64, u64) -> bool + Send + Sync + 'static,
        {
            let b: Box<Box<VerifyFn>> = Box::new(Box::new(f));
            let ctx = &*b as *const Box<VerifyFn> as *mut c_void;
            // SAFETY: as register_skill.
            let rc = unsafe { ffi::rxc_host_set_verify(self.h, Some(verify_tramp), ctx) };
            check(rc, 0)?;
            self.verify = Some(b);
            Ok(())
        }

        pub fn run(&mut self, task: u64, now_us: u64) -> Result<RunResult, ComposeError> {
            let mut r = RunResult::default();
            // SAFETY: valid handle and out pointer.
            let rc = unsafe { ffi::rxc_host_run(self.h, task, now_us, &mut r) };
            check(rc, r.run_rc)?;
            Ok(r)
        }

        /// Records of `subject`, oldest first (at most `max`), and how many exist.
        pub fn recall(
            &mut self,
            subject: u64,
            max: u32,
        ) -> Result<(Vec<Record>, u64), ComposeError> {
            let mut v = vec![Record::default(); max as usize];
            let (mut n, mut total) = (0u32, 0u64);
            // SAFETY: v holds max records.
            let rc = unsafe {
                ffi::rxc_host_recall(self.h, subject, v.as_mut_ptr(), max, &mut n, &mut total)
            };
            check(rc, 0)?;
            v.truncate(n as usize);
            Ok((v, total))
        }

        pub fn record(&mut self, id: u64) -> Result<Record, ComposeError> {
            let mut r = Record::default();
            // SAFETY: valid handle and out pointer.
            let rc = unsafe { ffi::rxc_host_record(self.h, id, &mut r) };
            check(rc, 0)?;
            Ok(r)
        }

        pub fn payload(&mut self, id: u64) -> Result<Vec<u64>, ComposeError> {
            // SAFETY: max 0 only asks for the count.
            let n = unsafe { ffi::rxc_host_payload(self.h, id, std::ptr::null_mut(), 0) };
            if n < 0 {
                return Err(ComposeError::Code { code: n, detail: 0 });
            }
            let mut v = vec![0u64; n as usize];
            // SAFETY: v holds n words.
            let m = unsafe { ffi::rxc_host_payload(self.h, id, v.as_mut_ptr(), n as u32) };
            if m < 0 {
                return Err(ComposeError::Code { code: m, detail: 0 });
            }
            Ok(v)
        }

        pub fn info(&mut self) -> Result<Info, ComposeError> {
            let mut i = Info::default();
            // SAFETY: valid handle and out pointer.
            let rc = unsafe { ffi::rxc_host_info(self.h, &mut i) };
            check(rc, i.open_rc)?;
            Ok(i)
        }
    }

    impl Drop for Compose {
        fn drop(&mut self) {
            // SAFETY: close stops every World worker before returning, so no
            // callback runs after this; the boxes drop afterwards.
            unsafe { ffi::rxc_host_close(self.h) };
            self.h = std::ptr::null_mut();
        }
    }
}

#[cfg(not(has_omega_compose))]
impl Compose {
    pub fn open(
        _: &Path,
        _: RootKind,
        _: &[u8],
        _: u64,
        _: bool,
    ) -> Result<(Self, Info), ComposeError> {
        Err(ComposeError::Unavailable)
    }
    pub fn register_skill<F>(
        &mut self,
        _: &str,
        _: Option<[u8; 32]>,
        _: u64,
        _: F,
    ) -> Result<u32, ComposeError>
    where
        F: Fn(u64) -> Option<u64> + Send + Sync + 'static,
    {
        Err(ComposeError::Unavailable)
    }
    pub fn set_verify<F>(&mut self, _: F) -> Result<(), ComposeError>
    where
        F: Fn(u64, u64) -> bool + Send + Sync + 'static,
    {
        Err(ComposeError::Unavailable)
    }
    pub fn run(&mut self, _: u64, _: u64) -> Result<RunResult, ComposeError> {
        Err(ComposeError::Unavailable)
    }
    pub fn recall(&mut self, _: u64, _: u32) -> Result<(Vec<Record>, u64), ComposeError> {
        Err(ComposeError::Unavailable)
    }
    pub fn record(&mut self, _: u64) -> Result<Record, ComposeError> {
        Err(ComposeError::Unavailable)
    }
    pub fn payload(&mut self, _: u64) -> Result<Vec<u64>, ComposeError> {
        Err(ComposeError::Unavailable)
    }
    pub fn info(&mut self) -> Result<Info, ComposeError> {
        Err(ComposeError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_or_linked_is_reported() {
        let dir =
            std::env::temp_dir().join(format!("aien-omega-compose-unit-{}", std::process::id()));
        let r = Compose::open(&dir, RootKind::Provisioned, b"unit", 1, true);
        match &r {
            Ok(_) => {
                if !LINKED {
                    panic!("stub build opened a home");
                }
            }
            Err(e) => {
                if LINKED {
                    panic!("linked build failed to open: {e}");
                }
                assert_eq!(*e, ComposeError::Unavailable);
            }
        }
        drop(r);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

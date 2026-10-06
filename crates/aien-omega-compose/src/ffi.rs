//! Raw declarations of omega `src/runtime/rxc_host_abi.h` (ABI version 1).
//! Layouts are `#[repr(C)]` mirrors; `Compose::open` compares their sizes with
//! `rxc_host_abi_layout` before any other call.
#![allow(non_camel_case_types)]

#[cfg(has_omega_compose)]
use std::os::raw::c_char;
use std::os::raw::{c_int, c_void};

pub const RXC_HOST_ABI_VERSION: u32 = 1;
pub const RXC_HOST_OPEN_REFUSE_TORN: u32 = 1;
pub const RXC_HOST_NONE: u32 = 0xFFFF_FFFF;
pub const RXC_HOST_SUBJECT_GOAL: u64 = 1;
pub const RXC_HOST_SUBJECT_STATE: u64 = 5;

pub const RXC_HOST_OK: c_int = 0;
pub const RXC_HOST_E_ARG: c_int = -1;
pub const RXC_HOST_E_IDENTITY: c_int = -2;
pub const RXC_HOST_E_TORN: c_int = -3;
pub const RXC_HOST_E_OPEN: c_int = -4;
pub const RXC_HOST_E_FULL: c_int = -5;
pub const RXC_HOST_E_STATE: c_int = -6;
pub const RXC_HOST_E_RUN: c_int = -7;
pub const RXC_HOST_E_NOMEM: c_int = -8;
pub const RXC_HOST_E_NOT_FOUND: c_int = -9;
pub const RXC_HOST_E_DIGEST: c_int = -10;

#[repr(C)]
pub struct RxcHost {
    _private: [u8; 0],
}

pub type RxcHostSkillFn =
    unsafe extern "C" fn(ctx: *mut c_void, task: u64, result: *mut u64) -> c_int;
pub type RxcHostVerifyFn = unsafe extern "C" fn(ctx: *mut c_void, task: u64, result: u64) -> c_int;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RxcHostInfo {
    pub abi_version: u32,
    pub machine_root: u8,
    pub machine_id: [u8; 32],
    pub machine_id_was_stored: u32,
    pub tail_torn: u32,
    pub opened: u32,
    pub open_rc: i32,
    pub records: u64,
    pub recovered_record: u64,
    pub rolled_back: u32,
    pub recovered_completed: u32,
    pub anchor_unknown: u32,
    pub n_skills: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RxcHostResult {
    pub run_rc: i32,
    pub outcome: i32,
    pub committed: u32,
    pub n_branches: u32,
    pub branches_reclaimed: u32,
    pub winner: u32,
    pub winner_skill: u32,
    pub aegis_pass_mask: u32,
    pub task: u64,
    pub result: u64,
    pub cx_goal: u64,
    pub cx_candidate: [u64; 2],
    pub cx_evidence: u64,
    pub cx_promotion: u64,
    pub cx_admission: [u64; 2],
    pub winner_digest: [u8; 32],
    pub record_digest: [u8; 32],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RxcHostRecord {
    pub id: u64,
    pub cls: u32,
    pub kind: u32,
    pub subject: u64,
    pub t: u64,
    pub generation: u64,
    pub tag: u64,
    pub links: [u64; 4],
    pub n_payload: u32,
    pub verified: u32,
    pub digest: [u8; 32],
}

#[cfg(has_omega_compose)]
extern "C" {
    pub fn rxc_host_abi_layout(out: *mut u32) -> u32;
    pub fn rxc_host_open(
        dir: *const c_char,
        root_kind: u32,
        root: *const u8,
        root_len: usize,
        session: u64,
        flags: u32,
        out: *mut *mut RxcHost,
        info: *mut RxcHostInfo,
    ) -> c_int;
    pub fn rxc_host_register_skill(
        h: *mut RxcHost,
        name: *const c_char,
        digest32: *const u8,
        cost: u64,
        f: RxcHostSkillFn,
        ctx: *mut c_void,
    ) -> c_int;
    pub fn rxc_host_set_verify(
        h: *mut RxcHost,
        f: Option<RxcHostVerifyFn>,
        ctx: *mut c_void,
    ) -> c_int;
    pub fn rxc_host_run(h: *mut RxcHost, task: u64, now_us: u64, out: *mut RxcHostResult) -> c_int;
    pub fn rxc_host_recall(
        h: *mut RxcHost,
        subject: u64,
        out: *mut RxcHostRecord,
        max: u32,
        n_filled: *mut u32,
        n_total: *mut u64,
    ) -> c_int;
    pub fn rxc_host_record(h: *mut RxcHost, id: u64, out: *mut RxcHostRecord) -> c_int;
    pub fn rxc_host_payload(h: *mut RxcHost, id: u64, out: *mut u64, max: u32) -> c_int;
    pub fn rxc_host_info(h: *mut RxcHost, info: *mut RxcHostInfo) -> c_int;
    pub fn rxc_host_close(h: *mut RxcHost);
}

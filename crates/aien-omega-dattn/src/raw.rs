//! Raw cache handle for sibling FFI crates (OWN-30B whole-prompt prefill, W-L1G-R, 2026-10-14). New file.
//!
//! omega's prefill attention (`omega_gpu_prefill_attn`, bound in `aien-omega-pf`) appends to and
//! reads the SAME KV cache the decode attention owns, so it needs the C handle of a
//! [`DecodeAttnCache`]. This child module can read the private field; nothing else changes.
use super::DecodeAttnCache;

impl DecodeAttnCache {
    /// The `OmegaGpuDecodeAttnCache *` behind this cache, or null in the stub build.
    ///
    /// Taking `&mut self` ties the handle's use to an exclusive borrow: the caller must not keep
    /// the pointer past that borrow, and must not free it (the cache's `Drop` does).
    pub fn raw_handle(&mut self) -> *mut std::ffi::c_void {
        #[cfg(has_omega_dattn)]
        {
            self.ptr.cast()
        }
        #[cfg(not(has_omega_dattn))]
        {
            std::ptr::null_mut()
        }
    }
}

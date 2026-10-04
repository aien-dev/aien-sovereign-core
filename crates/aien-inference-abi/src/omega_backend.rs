//! `OmegaGb10Backend` (FB-1 cuts 3b and 3c): the three matrix-multiply ops, rmsnorm,
//! rope and swiglu of `TensorBackend` run on our own native GPU engine
//! (`libomega_gpu.a`, no CUDA) through the `aien-omega-gpu` crate; the attention ops
//! run on the reference CPU path by design and are counted by `OpAccounting`.
//!
//! Native mask: `matmul_vec`, `matmul_batch`, `compute_logits`, `rmsnorm`,
//! `apply_rope`, `swiglu`. A chip error in
//! one of those is a fallback of a claimed-native op: it is counted, goes through
//! `OpAccounting::reference_path` (fatal in a production build, see `strict.rs`)
//! and, in a dev build only, the reference result is computed so the run goes on.
//!
//! Weights: the trait hands weights as `&[f32]` slices per call (row-major
//! `[out_dim, in_dim]`, computing `x * W^T`). The first call for a slice uploads
//! `W^T` (k = in_dim, n = out_dim, rounded to bf16 by omega) once and keeps it
//! resident; later calls move only activations. The cache key is
//! (pointer, length, in_dim, out_dim) plus a sampled content fingerprint.
//!
//! INVARIANT: weight slices are immutable and outlive the backend's use of them.
//! Checked for the model path: `TransformerWeights` (weights.rs:35) owns plain
//! `Vec<f32>` fields; the forward passes borrow it as `&TransformerWeights`
//! (transformer_backend.rs:287, :399) and nothing in the crate mutates a loaded
//! checkpoint after construction (UNVERIFIED for external callers: `weights` is a
//! `pub` field, so a caller that mutates it in place gets stale device weights;
//! the fingerprint catches a freed-and-reused address or a changed sample point,
//! not every in-place edit). Use one backend per weight set, or call
//! [`OmegaGb10Backend::clear_resident`] after changing weights.
use crate::backend::{ReferenceCpuBackend, TensorBackend};
use crate::native_ops::{NativeOpMask, OpAccounting, OpReport, TensorOp};
use aien_omega_gpu::ResidentTensor;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Opt-in selection: `AIEN_GPU_BACKEND=omega` binds this backend instead of the
/// default CUDA `BlackwellGb10Backend`. Unset (or any other value) keeps the default.
pub const GPU_BACKEND_ENV: &str = "AIEN_GPU_BACKEND";

/// True when the environment asks for the Omega backend.
pub fn omega_backend_selected() -> bool {
    std::env::var(GPU_BACKEND_ENV)
        .map(|v| v.trim().eq_ignore_ascii_case("omega"))
        .unwrap_or(false)
}

const BACKEND_NAME: &str = "OmegaGb10Backend";

type Key = (usize, usize, usize, usize);

/// Rope cos/sin table for one position: `head_dim / 2` values each.
type RopeTable = Arc<(Vec<f32>, Vec<f32>)>;

/// Cache key: (head_dim, theta bits, position). Drop everything past this many entries.
type RopeKey = (usize, u32, usize);
const ROPE_CACHE_MAX: usize = 16384;

struct Resident {
    fingerprint: u64,
    tensor: ResidentTensor,
}

/// Native backend on the Omega GPU engine (matmuls and elementwise ops).
pub struct OmegaGb10Backend {
    reference: ReferenceCpuBackend,
    acct: OpAccounting,
    /// One lock serializes every chip call and guards the resident cache
    /// (omega's resident-call thread safety is not documented).
    chip: Mutex<HashMap<Key, Resident>>,
    rope_tables: Mutex<HashMap<RopeKey, RopeTable>>,
    chip_errors: AtomicU64,
    chip_calls: AtomicU64,
    chip_ns: AtomicU64,
    last_error: Mutex<String>,
}

impl Default for OmegaGb10Backend {
    fn default() -> Self {
        Self::new()
    }
}

/// Cheap content check: FNV-1a over up to 64 evenly spaced elements plus the last one.
fn fingerprint(w: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let step = (w.len() / 64).max(1);
    let mut i = 0;
    while i < w.len() {
        h = (h ^ w[i].to_bits() as u64).wrapping_mul(0x100000001b3);
        i += step;
    }
    if let Some(last) = w.last() {
        h = (h ^ last.to_bits() as u64).wrapping_mul(0x100000001b3);
    }
    h
}

impl OmegaGb10Backend {
    pub fn new() -> Self {
        Self {
            reference: ReferenceCpuBackend::new(),
            acct: OpAccounting::new(NativeOpMask::from_ops(&[
                TensorOp::MatmulVec,
                TensorOp::MatmulBatch,
                TensorOp::ComputeLogits,
                TensorOp::Rmsnorm,
                TensorOp::ApplyRope,
                TensorOp::Swiglu,
            ])),
            chip: Mutex::new(HashMap::new()),
            rope_tables: Mutex::new(HashMap::new()),
            chip_errors: AtomicU64::new(0),
            chip_calls: AtomicU64::new(0),
            chip_ns: AtomicU64::new(0),
            last_error: Mutex::new(String::new()),
        }
    }

    /// True when the real native library is linked (not the CI stub).
    pub fn is_available(&self) -> bool {
        aien_omega_gpu::is_native()
    }

    pub fn chip_calls(&self) -> u64 {
        self.chip_calls.load(Ordering::Relaxed)
    }

    pub fn chip_errors(&self) -> u64 {
        self.chip_errors.load(Ordering::Relaxed)
    }

    /// Sum of chip launch-to-completion time reported by omega, in nanoseconds.
    pub fn chip_ns(&self) -> u64 {
        self.chip_ns.load(Ordering::Relaxed)
    }

    pub fn last_error(&self) -> String {
        self.last_error
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Drop every resident weight (call after changing weights in place).
    pub fn clear_resident(&self) {
        if let Ok(mut c) = self.chip.lock() {
            c.clear();
        }
    }

    /// Number of resident weight matrices (for tests and receipts).
    pub fn resident_count(&self) -> usize {
        self.chip.lock().map(|c| c.len()).unwrap_or(0)
    }

    /// `out[m x out_dim] = x[m x in_dim] * W^T`, W row-major `[out_dim, in_dim]`.
    /// Returns false on any chip error (already recorded).
    fn chip_matmul(
        &self,
        out: &mut [f32],
        x: &[f32],
        weight: &[f32],
        m: usize,
        in_dim: usize,
        out_dim: usize,
    ) -> bool {
        if x.len() != m * in_dim || out.len() != m * out_dim || weight.len() != out_dim * in_dim {
            return self.fail(format!(
                "shape mismatch: x={} out={} w={} for m={m} in={in_dim} out_dim={out_dim}",
                x.len(),
                out.len(),
                weight.len()
            ));
        }
        let key: Key = (weight.as_ptr() as usize, weight.len(), in_dim, out_dim);
        let fp = fingerprint(weight);
        let mut cache = match self.chip.lock() {
            Ok(g) => g,
            Err(_) => return self.fail("chip lock poisoned".into()),
        };
        if cache.get(&key).is_some_and(|r| r.fingerprint != fp) {
            cache.remove(&key);
        }
        if let std::collections::hash_map::Entry::Vacant(slot) = cache.entry(key) {
            // W is [out, in]; the engine wants B = W^T as [k = in, n = out].
            let mut wt = vec![0.0f32; weight.len()];
            for o in 0..out_dim {
                let row = &weight[o * in_dim..(o + 1) * in_dim];
                for (i, v) in row.iter().enumerate() {
                    wt[i * out_dim + o] = *v;
                }
            }
            match ResidentTensor::upload_f32(in_dim, out_dim, &wt) {
                Ok(tensor) => {
                    slot.insert(Resident {
                        fingerprint: fp,
                        tensor,
                    });
                }
                Err(e) => {
                    let msg = format!("upload {in_dim}x{out_dim}: {e}");
                    return self.fail(msg);
                }
            }
        }
        let res = &cache[&key];
        match res.tensor.matmul_f32(m, x, out) {
            Ok(info) => {
                self.chip_calls.fetch_add(1, Ordering::Relaxed);
                self.chip_ns
                    .fetch_add(info.raw.elapsed_ns, Ordering::Relaxed);
                true
            }
            Err(e) => {
                drop(cache);
                self.fail(format!("matmul m={m} k={in_dim} n={out_dim}: {e}"))
            }
        }
    }

    /// Rope cos/sin table for `pos`, built exactly as the reference does
    /// (`tensor::apply_rope`, tensor.rs:101): f64 frequency and angle, then cast to f32.
    /// Cached per (head_dim, theta, pos).
    fn rope_table(&self, head_dim: usize, theta: f32, pos: usize) -> RopeTable {
        let key: RopeKey = (head_dim, theta.to_bits(), pos);
        let mut cache = self
            .rope_tables
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(t) = cache.get(&key) {
            return Arc::clone(t);
        }
        let half = head_dim / 2;
        let mut cos = Vec::with_capacity(half);
        let mut sin = Vec::with_capacity(half);
        for i in 0..half {
            let exponent = (2 * i) as f64 / (head_dim as f64);
            let freq = 1.0 / (theta as f64).powf(exponent);
            let rot = (pos as f64) * freq;
            sin.push(rot.sin() as f32);
            cos.push(rot.cos() as f32);
        }
        if cache.len() >= ROPE_CACHE_MAX {
            cache.clear();
        }
        let t: RopeTable = Arc::new((cos, sin));
        cache.insert(key, Arc::clone(&t));
        t
    }

    /// Run one elementwise chip call under the chip lock. False on any error (recorded).
    fn chip_elementwise(
        &self,
        what: &str,
        call: impl FnOnce() -> Result<aien_omega_gpu::OmegaGpuEwInfo, aien_omega_gpu::OmegaGpuError>,
    ) -> bool {
        let guard = match self.chip.lock() {
            Ok(g) => g,
            Err(_) => return self.fail("chip lock poisoned".into()),
        };
        let res = call();
        drop(guard);
        match res {
            Ok(info) => {
                self.chip_calls.fetch_add(1, Ordering::Relaxed);
                self.chip_ns.fetch_add(info.elapsed_ns, Ordering::Relaxed);
                true
            }
            Err(e) => self.fail(format!("{what}: {e}")),
        }
    }

    fn chip_rmsnorm(&self, out: &mut [f32], x: &[f32], weight: &[f32], eps: f32) -> bool {
        let dim = x.len();
        self.chip_elementwise(&format!("rmsnorm dim={dim}"), || {
            aien_omega_gpu::rmsnorm_f32(1, dim, x, weight, eps, out)
        })
    }

    /// q then k go to the chip as one `heads` run (the table is the same for every head).
    #[allow(clippy::too_many_arguments)]
    fn chip_rope(
        &self,
        q: &mut [f32],
        k: &mut [f32],
        pos: usize,
        head_dim: usize,
        num_q_heads: usize,
        num_kv_heads: usize,
        theta: f32,
    ) -> bool {
        if head_dim == 0
            || !head_dim.is_multiple_of(2)
            || q.len() != num_q_heads * head_dim
            || k.len() != num_kv_heads * head_dim
        {
            return self.fail(format!(
                "rope shape mismatch: q={} k={} head_dim={head_dim} q_heads={num_q_heads} kv_heads={num_kv_heads}",
                q.len(),
                k.len()
            ));
        }
        let table = self.rope_table(head_dim, theta, pos);
        let mut both = Vec::with_capacity(q.len() + k.len());
        both.extend_from_slice(q);
        both.extend_from_slice(k);
        let mut rotated = vec![0.0f32; both.len()];
        let heads = num_q_heads + num_kv_heads;
        let ok = self.chip_elementwise(&format!("rope heads={heads} head_dim={head_dim}"), || {
            aien_omega_gpu::rope_f32(heads, head_dim, &both, &table.0, &table.1, &mut rotated)
        });
        if ok {
            let (rq, rk) = rotated.split_at(q.len());
            q.copy_from_slice(rq);
            k.copy_from_slice(rk);
        }
        ok
    }

    fn chip_swiglu(&self, out: &mut [f32], gate: &[f32], up: &[f32]) -> bool {
        // The reference uses min(len) of the three; the chip path demands equal lengths.
        if gate.len() != up.len() || gate.len() != out.len() {
            return self.fail(format!(
                "swiglu shape mismatch: gate={} up={} out={}",
                gate.len(),
                up.len(),
                out.len()
            ));
        }
        self.chip_elementwise(&format!("swiglu n={}", gate.len()), || {
            aien_omega_gpu::swiglu_f32(gate, up, out)
        })
    }

    fn fail(&self, msg: String) -> bool {
        self.chip_errors.fetch_add(1, Ordering::Relaxed);
        let stage = aien_omega_gpu::last_error();
        let full = if stage.is_empty() {
            msg
        } else {
            format!("{msg} (stage: {stage})")
        };
        eprintln!("OMEGA_BACKEND chip error: {full}");
        if let Ok(mut l) = self.last_error.lock() {
            *l = full;
        }
        false
    }

    fn reference_for(&self, op: TensorOp) {
        self.acct.reference_path(BACKEND_NAME, op);
    }
}

impl TensorBackend for OmegaGb10Backend {
    fn name(&self) -> &'static str {
        if self.is_available() {
            "OmegaGb10Backend (native Omega engine, no CUDA, NVIDIA GB10 sm_121)"
        } else {
            "OmegaGb10Backend (stub, no chip)"
        }
    }

    fn fallback_count(&self) -> u64 {
        self.acct.fallback_count()
    }

    fn native_ops(&self) -> NativeOpMask {
        self.acct.mask()
    }

    fn op_report(&self) -> OpReport {
        self.acct.report()
    }

    fn rmsnorm(&self, out: &mut [f32], x: &[f32], weight: &[f32], eps: f32) {
        if !self.chip_rmsnorm(out, x, weight, eps) {
            self.reference_for(TensorOp::Rmsnorm);
            self.reference.rmsnorm(out, x, weight, eps);
        }
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
        if !self.chip_rope(q, k, pos, head_dim, num_q_heads, num_kv_heads, theta) {
            self.reference_for(TensorOp::ApplyRope);
            self.reference
                .apply_rope(q, k, pos, head_dim, num_q_heads, num_kv_heads, theta);
        }
    }

    fn matmul_vec(
        &self,
        out: &mut [f32],
        x: &[f32],
        weight: &[f32],
        out_dim: usize,
        in_dim: usize,
    ) {
        if !self.chip_matmul(out, x, weight, 1, in_dim, out_dim) {
            self.reference_for(TensorOp::MatmulVec);
            self.reference.matmul_vec(out, x, weight, out_dim, in_dim);
        }
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
        if !self.chip_matmul(out, x, weight, batch_size, in_dim, out_dim) {
            self.reference_for(TensorOp::MatmulBatch);
            self.reference
                .matmul_batch(out, x, weight, batch_size, in_dim, out_dim);
        }
    }

    fn swiglu(&self, out: &mut [f32], gate: &[f32], up: &[f32]) {
        if !self.chip_swiglu(out, gate, up) {
            self.reference_for(TensorOp::Swiglu);
            self.reference.swiglu(out, gate, up);
        }
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
        self.reference_for(TensorOp::GqaAttention);
        self.reference.gqa_attention(
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
        self.reference_for(TensorOp::PagedAttention);
        self.reference.paged_attention(
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

    fn paged_attention_batch(
        &self,
        out: &mut [f32],
        q: &[f32],
        pool: &aien_kv_cache::UnifiedKvTensorPool,
        block_tables: &[i32],
        context_lens: &[i32],
        max_blocks_per_seq: usize,
        num_seqs: usize,
        layer_idx: usize,
        num_q_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
    ) {
        // Counted once as a batch op; the reference batch calls the reference
        // single-sequence path, not ours, so there is no double count.
        self.reference_for(TensorOp::PagedAttentionBatch);
        self.reference.paged_attention_batch(
            out,
            q,
            pool,
            block_tables,
            context_lens,
            max_blocks_per_seq,
            num_seqs,
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
        if !self.chip_matmul(logits, hidden, embed_weight, 1, hidden_dim, vocab_size) {
            self.reference_for(TensorOp::ComputeLogits);
            self.reference
                .compute_logits(logits, hidden, embed_weight, vocab_size, hidden_dim);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(s: &mut u32) -> f32 {
        *s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        ((*s >> 8) as f32 / (1u32 << 24) as f32) - 0.5
    }

    #[test]
    fn mask_is_exactly_the_six_native_ops() {
        let b = OmegaGb10Backend::new();
        let names: Vec<_> = b
            .native_ops()
            .native_ops()
            .iter()
            .map(|o| o.name())
            .collect();
        assert_eq!(
            names,
            [
                "rmsnorm",
                "apply_rope",
                "matmul_vec",
                "matmul_batch",
                "swiglu",
                "compute_logits"
            ]
        );
    }

    #[test]
    fn reference_ops_are_counted_by_design_and_never_trip() {
        let b = OmegaGb10Backend::new();
        let mut out = [0.0f32; 2];
        b.gqa_attention(&mut out, &[1.0; 2], &[1.0; 2], &[1.0; 2], 1, 1, 1, 2);
        assert_eq!(b.fallback_count(), 0);
        assert!(b.op_report().line().contains("gqa_attention:1"));
    }

    /// Stub build only: a chip error on a claimed-native op is a counted fallback
    /// (dev build) with the reference result, never silent.
    #[test]
    fn chip_error_is_a_counted_fallback() {
        if aien_omega_gpu::is_native() || !crate::strict::dev_fallback_active() {
            return; // native: nothing fails here; production: it would (correctly) panic
        }
        let b = OmegaGb10Backend::new();
        let mut s = 7u32;
        let (m, k, n) = (3usize, 32usize, 16usize);
        let x: Vec<f32> = (0..m * k).map(|_| lcg(&mut s)).collect();
        let w: Vec<f32> = (0..n * k).map(|_| lcg(&mut s)).collect();
        let mut got = vec![0.0f32; m * n];
        let mut want = vec![0.0f32; m * n];
        b.matmul_batch(&mut got, &x, &w, m, k, n);
        ReferenceCpuBackend::new().matmul_batch(&mut want, &x, &w, m, k, n);
        assert_eq!(got, want);
        assert_eq!(b.fallback_count(), 1);
        assert_eq!(b.chip_errors(), 1);
        assert!(b
            .op_report()
            .line()
            .contains("native_fallbacks=[matmul_batch:1]"));
    }

    #[test]
    #[should_panic(expected = "STRICT_REAL_MODEL_VIOLATION")]
    fn chip_error_trips_strict_in_production() {
        if aien_omega_gpu::is_native() || crate::strict::dev_fallback_active() {
            panic!("STRICT_REAL_MODEL_VIOLATION (skipped: native or dev build)");
        }
        let b = OmegaGb10Backend::new();
        let mut out = [0.0f32; 2];
        b.matmul_vec(&mut out, &[1.0; 2], &[1.0; 4], 2, 2);
    }

    /// The cached cos/sin table, applied on the host with the same f32 formulas as the
    /// chip kernel (FMUL, FMUL, FADD), must equal the reference rope bit for bit.
    #[test]
    fn rope_table_reproduces_reference_rope() {
        let b = OmegaGb10Backend::new();
        let (head_dim, nq, nkv, pos, theta) = (64usize, 4usize, 2usize, 37usize, 10000.0f32);
        let mut s = 99u32;
        let q0: Vec<f32> = (0..nq * head_dim).map(|_| lcg(&mut s)).collect();
        let k0: Vec<f32> = (0..nkv * head_dim).map(|_| lcg(&mut s)).collect();
        let (mut qr, mut kr) = (q0.clone(), k0.clone());
        ReferenceCpuBackend::new().apply_rope(&mut qr, &mut kr, pos, head_dim, nq, nkv, theta);
        let t = b.rope_table(head_dim, theta, pos);
        let half = head_dim / 2;
        let rot = |v: &[f32]| -> Vec<f32> {
            let mut o = v.to_vec();
            for h in 0..v.len() / head_dim {
                for i in 0..half {
                    let (a, c) = (v[h * head_dim + i], v[h * head_dim + i + half]);
                    o[h * head_dim + i] = a * t.0[i] - c * t.1[i];
                    o[h * head_dim + i + half] = a * t.1[i] + c * t.0[i];
                }
            }
            o
        };
        assert_eq!(rot(&q0), qr);
        assert_eq!(rot(&k0), kr);
        // cached: same allocation on the second ask
        assert!(Arc::ptr_eq(&t, &b.rope_table(head_dim, theta, pos)));
    }

    /// Stub build only: each newly native elementwise op is a counted fallback with the
    /// reference result when the chip is unavailable, never silent.
    #[test]
    fn elementwise_chip_errors_are_counted_fallbacks() {
        if aien_omega_gpu::is_native() || !crate::strict::dev_fallback_active() {
            return;
        }
        let b = OmegaGb10Backend::new();
        let r = ReferenceCpuBackend::new();
        let mut s = 5u32;
        let x: Vec<f32> = (0..128).map(|_| lcg(&mut s)).collect();
        let w: Vec<f32> = (0..128).map(|_| lcg(&mut s)).collect();
        let (mut got, mut want) = (vec![0.0f32; 128], vec![0.0f32; 128]);
        b.rmsnorm(&mut got, &x, &w, 1e-5);
        r.rmsnorm(&mut want, &x, &w, 1e-5);
        assert_eq!(got, want);
        b.swiglu(&mut got, &x, &w);
        r.swiglu(&mut want, &x, &w);
        assert_eq!(got, want);
        let (mut q, mut k) = (x[..64].to_vec(), x[64..].to_vec());
        let (mut qw, mut kw) = (q.clone(), k.clone());
        b.apply_rope(&mut q, &mut k, 3, 32, 2, 2, 10000.0);
        r.apply_rope(&mut qw, &mut kw, 3, 32, 2, 2, 10000.0);
        assert_eq!((q, k), (qw, kw));
        assert_eq!(b.fallback_count(), 3);
        let line = b.op_report().line();
        assert!(
            line.contains("native_fallbacks=[rmsnorm:1,apply_rope:1,swiglu:1]"),
            "{line}"
        );
    }

    #[test]
    fn fingerprint_changes_with_content() {
        let a = vec![1.0f32; 1000];
        let mut b = a.clone();
        b[999] = 2.0;
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }
}

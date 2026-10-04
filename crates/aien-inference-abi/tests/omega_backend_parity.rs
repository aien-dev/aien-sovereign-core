//! FB-1 cut 3b: OmegaGb10Backend against ReferenceCpuBackend on TinyLlama layer shapes.
//!
//! Host test (runs everywhere, stub-aware): in a stub build the backend must
//! report itself unavailable. Chip test (`#[ignore]`, run through the heavy
//! queue with a native build):
//!   AIEN_OMEGA_DIR=... AIEN_PHYSICS_DIR=... cargo test -p aien-inference-abi --release \
//!     --test omega_backend_parity -- --ignored --nocapture
//!
//! Error metric: |chip - ref| divided by sum_i |x_i * w_i| (the accumulation
//! scale). Both sides see bf16-rounded inputs on the chip and f32 on the
//! reference, so each product carries at most ~2 * 2^-8 relative error; the bound
//! asserted is 8e-3 of the scale, and the typical value is far lower (random signs).
use aien_inference_abi::{OmegaGb10Backend, ReferenceCpuBackend, TensorBackend};

fn lcg(s: &mut u32) -> f32 {
    *s = s.wrapping_mul(1664525).wrapping_add(1013904223);
    ((*s >> 8) as f32 / (1u32 << 24) as f32) - 0.5
}

/// Max over outputs of |got - want| / sum|x*w|, for out = X * W^T.
fn worst_ratio(
    got: &[f32],
    want: &[f32],
    x: &[f32],
    w: &[f32],
    m: usize,
    k: usize,
    n: usize,
) -> f64 {
    let mut worst = 0.0f64;
    for r in 0..m {
        for o in 0..n {
            let mut scale = 0.0f64;
            for i in 0..k {
                scale += (x[r * k + i] as f64 * w[o * k + i] as f64).abs();
            }
            let d = (got[r * n + o] as f64 - want[r * n + o] as f64).abs();
            worst = worst.max(d / scale.max(1e-30));
        }
    }
    worst
}

#[test]
fn stub_backend_reports_unavailable() {
    let b = OmegaGb10Backend::new();
    assert_eq!(b.is_available(), aien_omega_gpu::is_native());
    assert_eq!(b.native_ops().native_ops().len(), 3);
}

#[test]
#[ignore = "chip test: needs a native build and the GB10; run through the heavy queue"]
fn chip_parity_on_tinyllama_shapes() {
    let omega = OmegaGb10Backend::new();
    assert!(omega.is_available(), "native build required");
    let reference = ReferenceCpuBackend::new();
    // (name, out_dim, in_dim): TinyLlama 1.1B hidden 2048, kv 256, inter 5632, vocab 32000.
    let shapes = [
        ("q_proj/o_proj", 2048usize, 2048usize),
        ("k_proj/v_proj", 256, 2048),
        ("gate/up_proj", 5632, 2048),
        ("down_proj", 2048, 5632),
        ("lm_head", 32000, 2048),
    ];
    let mut seed = 12345u32;
    let mut overall = 0.0f64;
    for (name, out_dim, in_dim) in shapes {
        let w: Vec<f32> = (0..out_dim * in_dim).map(|_| lcg(&mut seed)).collect();
        // matmul_vec (and compute_logits for lm_head), batch 1
        let x: Vec<f32> = (0..in_dim).map(|_| lcg(&mut seed)).collect();
        let mut got = vec![0.0f32; out_dim];
        let mut want = vec![0.0f32; out_dim];
        if name == "lm_head" {
            omega.compute_logits(&mut got, &x, &w, out_dim, in_dim);
            reference.compute_logits(&mut want, &x, &w, out_dim, in_dim);
        } else {
            omega.matmul_vec(&mut got, &x, &w, out_dim, in_dim);
            reference.matmul_vec(&mut want, &x, &w, out_dim, in_dim);
        }
        let r1 = worst_ratio(&got, &want, &x, &w, 1, in_dim, out_dim);
        // matmul_batch, batch 7 (same resident weights, different m)
        let batch = 7usize;
        let xb: Vec<f32> = (0..batch * in_dim).map(|_| lcg(&mut seed)).collect();
        let mut gotb = vec![0.0f32; batch * out_dim];
        let mut wantb = vec![0.0f32; batch * out_dim];
        omega.matmul_batch(&mut gotb, &xb, &w, batch, in_dim, out_dim);
        reference.matmul_batch(&mut wantb, &xb, &w, batch, in_dim, out_dim);
        let rb = worst_ratio(&gotb, &wantb, &xb, &w, batch, in_dim, out_dim);
        println!(
            "PARITY {name} {out_dim}x{in_dim}: vec max_err/scale={r1:.3e} batch7 max_err/scale={rb:.3e}"
        );
        assert!(r1 < 8e-3 && rb < 8e-3, "{name}: parity bound exceeded");
        overall = overall.max(r1).max(rb);
    }
    assert_eq!(omega.fallback_count(), 0, "{}", omega.last_error());
    assert_eq!(omega.chip_errors(), 0, "{}", omega.last_error());
    println!("PARITY overall worst max_err/scale = {overall:.3e}");
    println!("PARITY resident weights = {}", omega.resident_count());
    println!("PARITY {}", omega.op_report().line());
}

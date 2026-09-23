//! GB10 parity tests for the Mojo Qwen3-Coder-A3B routed expert layer.
//!
//! Build the library first: `crates/aien-inference-abi/mojo/qwen3_moe/build.sh`.
//! Without it these tests skip, unless `AIEN_REQUIRE_GPU_TESTS=1` makes that a failure.
//! `AIEN_QWEN3_FP8_CHECKPOINT=<dir>` enables the official-checkpoint test;
//! `AIEN_QWEN3_LAYERS=0,1,23,24,47` picks its layers.

use aien_inference_abi::{
    bf16_to_f32, f32_to_bf16, fp8_e4m3fn_to_f32, load_qwen3_fp8_moe_layer, parity_stats,
    reference_moe_forward_with, reference_router_logits, seeded_bf16_input, MoeBatchPlan,
    OracleActivations, Qwen3MoeError, Qwen3MoeKernels, Qwen3MoeLayer,
    QWEN3_CODER_A3B_EXPERTS as EXPERTS, QWEN3_CODER_A3B_TOP_K as TOP_K,
};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Tests share one GPU; serialize them so each gets a clean device context.
fn gpu() -> Option<(MutexGuard<'static, ()>, &'static Qwen3MoeKernels)> {
    static LOCK: Mutex<()> = Mutex::new(());
    static KERNELS: OnceLock<Option<Qwen3MoeKernels>> = OnceLock::new();
    let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let kernels = KERNELS.get_or_init(|| {
        let path = Qwen3MoeKernels::default_path();
        match Qwen3MoeKernels::load(&path) {
            Ok(kernels) => Some(kernels),
            Err(error) if std::env::var_os("AIEN_REQUIRE_GPU_TESTS").is_some() => {
                panic!(
                    "AIEN_REQUIRE_GPU_TESTS is set but the kernel library is unavailable: {error}"
                )
            }
            Err(error) => {
                eprintln!("skipping Qwen3 MoE GPU test: {error}");
                None
            }
        }
    });
    kernels.as_ref().map(|k| (guard, k))
}

/// Deterministic generator for test data.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn uniform(&mut self) -> f32 {
        (self.next() % 1_000_000) as f32 / 1_000_000.0
    }
    /// A finite E4M3 byte with magnitude at most 2.
    fn fp8(&mut self) -> u8 {
        let magnitude = (self.next() % 0x41) as u8; // 0x00..=0x40, up to 2.0
        magnitude
            | if self.next().is_multiple_of(2) {
                0x80
            } else {
                0
            }
    }
}

#[test]
fn grouped_fp8_gemm_matches_block_scaled_reference() {
    let Some((_gpu, kernels)) = gpu() else { return };
    // Two experts, ragged groups, two 128-channel blocks on each axis, and
    // distinct values per row, column and expert to expose fragment mapping.
    let (rows, experts, channels) = (26usize, 2usize, 256usize);
    let values = [0.0f32, 0.5, 1.0, 2.0, -0.5, -1.0, -2.0];
    let bits = [0x00u8, 0x30, 0x38, 0x40, 0xb0, 0xb8, 0xc0];
    let a_index = |r: usize, k: usize| (r * 3 + k * 5) % 7;
    let w_index = |e: usize, o: usize, k: usize| (e * 2 + o * 3 + k * 5) % 7;
    let activations: Vec<u8> = (0..rows * channels)
        .map(|i| bits[a_index(i / channels, i % channels)])
        .collect();
    let weights: Vec<u8> = (0..experts * channels * channels)
        .map(|i| {
            bits[w_index(
                i / (channels * channels),
                (i / channels) % channels,
                i % channels,
            )]
        })
        .collect();
    let a_scales: Vec<f32> = (0..rows * 2)
        .map(|i| if (i / 2) % 2 == 0 { 1.0 } else { 0.5 } * [1.0, 2.0][i % 2])
        .collect();
    let w_scales_f32 = [1.0f32, 0.5, 2.0, 1.0, 0.5, 2.0, 1.0, 0.5];
    let w_scales: Vec<u16> = w_scales_f32.iter().map(|&v| f32_to_bf16(v)).collect();
    let offsets = [0u32, 19, rows as u32];
    let expert_ids = [1i32, 0];

    let actual = kernels
        .grouped_fp8_gemm(
            rows,
            experts,
            channels,
            channels,
            &activations,
            &a_scales,
            &weights,
            &w_scales,
            &offsets,
            &expert_ids,
        )
        .unwrap();
    for row in 0..rows {
        let expert = expert_ids[usize::from(row >= 19)] as usize;
        for out in 0..channels {
            let mut total = 0.0f64;
            for block in 0..2 {
                let dot: f64 = (block * 128..(block + 1) * 128)
                    .map(|k| {
                        values[a_index(row, k)] as f64 * values[w_index(expert, out, k)] as f64
                    })
                    .sum();
                total += dot
                    * a_scales[row * 2 + block] as f64
                    * w_scales_f32[expert * 4 + (out / 128) * 2 + block] as f64;
            }
            let got = actual[row * channels + out] as f64;
            assert!(
                (got - total).abs() <= 2e-4,
                "row {row} out {out}: {got} vs {total}"
            );
        }
    }
}

#[test]
fn grouped_fp8_gemm_rejects_out_of_range_groups() {
    let Some((_gpu, kernels)) = gpu() else { return };
    let (rows, c) = (4usize, 128usize);
    let run = |offsets: &[u32], ids: &[i32]| {
        kernels.grouped_fp8_gemm(
            rows,
            1,
            c,
            c,
            &vec![0x38; rows * c],
            &vec![1.0; rows],
            &vec![0x38; c * c],
            &[0x3f80],
            offsets,
            ids,
        )
    };
    assert!(run(&[0, 4], &[0]).is_ok());
    assert_eq!(
        run(&[0, 4], &[1]).unwrap_err(),
        Qwen3MoeError::Kernel(1),
        "expert id beyond weights"
    );
    assert_eq!(
        run(&[0, 4], &[-1]).unwrap_err(),
        Qwen3MoeError::Kernel(1),
        "negative expert id"
    );
    assert_eq!(
        run(&[0, 5], &[0]).unwrap_err(),
        Qwen3MoeError::Kernel(1),
        "offsets past the last row"
    );
    assert_eq!(
        run(&[0, 3, 2, 4].as_slice()[..3], &[0, 0]).unwrap_err(),
        Qwen3MoeError::Kernel(1),
        "non-monotonic offsets"
    );
}

/// Rust router == Mojo router == device grouping permutation == inverse map.
#[test]
fn device_routing_matches_moe_batch_plan_exactly() {
    let Some((_gpu, kernels)) = gpu() else { return };
    let tokens = 1024;
    let mut rng = Rng(7);
    let mut logits: Vec<f32> = (0..tokens * EXPERTS)
        .map(|_| rng.uniform() * 8.0 - 4.0)
        .collect();
    // Token 0: every logit tied, so selection is purely by lowest expert ID.
    logits[..EXPERTS].fill(0.25);
    // Token 1: ties straddling the top-8 boundary and the 64-expert mask split.
    logits[EXPERTS..2 * EXPERTS].fill(-10.0);
    for (expert, value) in [
        (3, 5.0),
        (70, 5.0),
        (64, 4.0),
        (63, 4.0),
        (100, 3.0),
        (2, 3.0),
        (127, 3.0),
        (0, 3.0),
        (1, 3.0),
    ] {
        logits[EXPERTS + expert] = value;
    }
    // Token 2: a skewed router that sends most weight to one expert.
    logits[2 * EXPERTS + 17] = 30.0;

    let device = kernels.route(&logits, tokens).unwrap();
    let plan = MoeBatchPlan::qwen3_coder_a3b(&logits, tokens).unwrap();

    let device_ids: Vec<u32> = device.expert_ids.iter().map(|&id| id as u32).collect();
    assert_eq!(device_ids, plan.topk_experts, "top-8 expert selection");
    assert_eq!(
        &device_ids[..TOP_K],
        &[0, 1, 2, 3, 4, 5, 6, 7],
        "all-tied token uses lowest IDs"
    );
    assert_eq!(
        &device_ids[TOP_K..2 * TOP_K],
        &[3, 70, 63, 64, 0, 1, 2, 100],
        "ties resolve by lower ID"
    );
    for (a, (d, p)) in device.weights.iter().zip(&plan.topk_weights).enumerate() {
        assert!((d - p).abs() <= 1e-6, "assignment {a}: weight {d} vs {p}");
    }
    assert_eq!(device.offsets, plan.expert_offsets, "expert offsets");
    assert_eq!(
        device.order, plan.grouped_to_assignment,
        "grouped permutation"
    );
    assert_eq!(device.restore, plan.assignment_to_grouped, "inverse map");
}

#[test]
fn device_router_fails_closed_on_non_finite_logits() {
    let Some((_gpu, kernels)) = gpu() else { return };
    for poison in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut logits = vec![0.0f32; 3 * EXPERTS];
        logits[EXPERTS + 77] = poison;
        assert_eq!(
            kernels.route(&logits, 3).unwrap_err(),
            Qwen3MoeError::Kernel(2),
            "{poison}"
        );
        assert!(
            MoeBatchPlan::qwen3_coder_a3b(&logits, 3).is_err(),
            "host plan agrees for {poison}"
        );
    }
}

/// A layer small enough for a fast oracle but with every expert distinct, varied
/// routing, multiple 128-channel blocks and a ragged token count.
fn synthetic_layer(seed: u64, hidden: usize, intermediate: usize) -> Qwen3MoeLayer {
    let mut rng = Rng(seed);
    let router: Vec<u16> = (0..EXPERTS * hidden)
        .map(|_| f32_to_bf16((rng.uniform() - 0.5) * 0.5))
        .collect();
    let gate_up: Vec<u8> = (0..EXPERTS * 2 * intermediate * hidden)
        .map(|_| rng.fp8())
        .collect();
    let down: Vec<u8> = (0..EXPERTS * hidden * intermediate)
        .map(|_| rng.fp8())
        .collect();
    let mut scale = |count: usize| -> Vec<u16> {
        (0..count)
            .map(|_| f32_to_bf16(0.01 + rng.uniform() * 0.05))
            .collect()
    };
    let gate_up_scales = scale(EXPERTS * (2 * intermediate / 128) * (hidden / 128));
    let down_scales = scale(EXPERTS * (hidden / 128) * (intermediate / 128));
    Qwen3MoeLayer::from_parts(
        hidden,
        intermediate,
        &router,
        &gate_up,
        &gate_up_scales,
        &down,
        &down_scales,
    )
    .unwrap()
}

/// Checks a device layer call against the oracle. The oracle routes with the
/// device's own logits so near-tie reordering from FP32 summation order cannot
/// change which experts are compared; the logits themselves are checked separately.
fn check_layer(
    kernels: &Qwen3MoeKernels,
    layer: &Qwen3MoeLayer,
    x_bits: &[u16],
    x: &[f32],
    tokens: usize,
    oracle: OracleActivations,
    label: &str,
) -> aien_inference_abi::ParityStats {
    let device = kernels.forward(layer, x_bits, tokens).unwrap();
    let cpu_logits = reference_router_logits(layer, x, tokens);
    let logit_scale = cpu_logits.iter().fold(1.0f32, |m, v| m.max(v.abs()));
    let logit_error = device
        .logits
        .iter()
        .zip(&cpu_logits)
        .fold(0.0f32, |m, (d, c)| m.max((d - c).abs()));
    assert!(
        logit_error <= 1e-4 * logit_scale,
        "{label}: router logits differ by {logit_error}"
    );

    let plan = MoeBatchPlan::qwen3_coder_a3b(&device.logits, tokens).unwrap();
    let ids: Vec<u32> = device.expert_ids.iter().map(|&id| id as u32).collect();
    assert_eq!(
        ids, plan.topk_experts,
        "{label}: device top-8 vs MoeBatchPlan on device logits"
    );
    assert_eq!(device.offsets, plan.expert_offsets, "{label}: offsets");
    assert_eq!(
        device.order, plan.grouped_to_assignment,
        "{label}: grouped order"
    );

    let expected = reference_moe_forward_with(layer, x, &plan, oracle);
    let stats = parity_stats(&device.output, &expected);
    eprintln!(
        "{label} ({oracle:?} oracle): tokens={tokens} cosine={:.8} p95_rel={:.5} max_abs={:.6} max_abs_expected={:.4} router_logit_err={logit_error:.2e}",
        stats.cosine, stats.p95_rel, stats.max_abs, stats.max_abs_expected
    );
    assert_eq!(stats.non_finite, 0, "{label}: non-finite outputs");
    stats
}

// Against an oracle that quantizes activations exactly as the device does, the
// only differences left are FP32 accumulation order and rare one-ULP rounding
// flips at E4M3 ties, so any indexing, scaling or routing bug shows up here.
const EMULATED_COSINE_MIN: f64 = 0.99999;
const EMULATED_MAX_ABS_FRACTION: f32 = 0.01;

#[test]
fn synthetic_layer_with_distinct_experts_matches_fp32_oracle() {
    let Some((_gpu, kernels)) = gpu() else { return };
    let layer = synthetic_layer(11, 256, 256);
    for (tokens, seed) in [(1usize, 1u64), (37, 2), (130, 3)] {
        let (bits, x) = seeded_bf16_input(seed, tokens, layer.hidden, 1.0);
        let stats = check_layer(
            kernels,
            &layer,
            &bits,
            &x,
            tokens,
            OracleActivations::Fp8Block128,
            "synthetic",
        );
        assert!(
            stats.cosine >= EMULATED_COSINE_MIN,
            "cosine {}",
            stats.cosine
        );
        assert!(
            stats.max_abs <= EMULATED_MAX_ABS_FRACTION * stats.max_abs_expected,
            "max_abs {}",
            stats.max_abs
        );
        // Reported, not gated: random-sign synthetic weights amplify E4M3 rounding.
        check_layer(
            kernels,
            &layer,
            &bits,
            &x,
            tokens,
            OracleActivations::Fp32,
            "synthetic",
        );
    }
}

#[test]
fn layer_rejects_non_finite_input_without_output() {
    let Some((_gpu, kernels)) = gpu() else { return };
    let layer = synthetic_layer(5, 128, 128);
    let (mut bits, _) = seeded_bf16_input(9, 4, layer.hidden, 1.0);
    bits[2 * layer.hidden + 5] = f32_to_bf16(f32::NAN);
    assert_eq!(
        kernels.forward(&layer, &bits, 4).unwrap_err(),
        Qwen3MoeError::Kernel(2)
    );
}

#[test]
fn e4m3_test_generator_stays_finite_and_bounded() {
    let mut rng = Rng(3);
    for _ in 0..10_000 {
        let value = fp8_e4m3fn_to_f32(rng.fp8());
        assert!(value.is_finite() && value.abs() <= 2.0);
    }
    assert_eq!(bf16_to_f32(f32_to_bf16(0.5)), 0.5);
}

/// Official checkpoint tolerances against the FP32-activation oracle, frozen from
/// layers 0, 1, 23, 24 and 47 with seeds 1, 2 and 3 (16 tokens each). Worst observed:
/// cosine 0.99910 (layer 23), p95 0.0173 (layer 47), max_abs 3.2% of max|expected|
/// (layer 24). Limits allow about 1.5x to 2x that error. See mojo/qwen3_moe/README.md.
const REAL_COSINE_MIN: f64 = 0.9985;
const REAL_P95_REL_MAX: f32 = 0.025;
const REAL_MAX_ABS_FRACTION: f32 = 0.05;

#[test]
fn official_checkpoint_layers_match_fp32_oracle() {
    let Some(dir) = std::env::var_os("AIEN_QWEN3_FP8_CHECKPOINT").map(PathBuf::from) else {
        eprintln!("skipping official checkpoint parity: AIEN_QWEN3_FP8_CHECKPOINT not set");
        return;
    };
    let Some((_gpu, kernels)) = gpu() else { return };
    let layers: Vec<usize> = std::env::var("AIEN_QWEN3_LAYERS")
        .unwrap_or_else(|_| "0,1,23,24,47".to_string())
        .split(',')
        .map(|l| {
            l.trim()
                .parse()
                .expect("AIEN_QWEN3_LAYERS must be integers")
        })
        .collect();
    let seeds: Vec<u64> = std::env::var("AIEN_QWEN3_SEEDS")
        .unwrap_or_else(|_| "1".to_string())
        .split(',')
        .map(|s| s.trim().parse().expect("AIEN_QWEN3_SEEDS must be integers"))
        .collect();
    let tokens = 16;
    // Measure every case before asserting so one run reports the whole corpus.
    let mut failures = Vec::new();
    for layer_index in layers {
        let layer = load_qwen3_fp8_moe_layer(&dir, layer_index).unwrap();
        for &seed in &seeds {
            let (bits, x) =
                seeded_bf16_input(1000 * seed + layer_index as u64, tokens, layer.hidden, 0.25);
            let label = format!(
                "layer {layer_index} seed {seed} capsule {}",
                &layer.capsule.capsule_digest[..12]
            );
            let emulated = check_layer(
                kernels,
                &layer,
                &bits,
                &x,
                tokens,
                OracleActivations::Fp8Block128,
                &label,
            );
            if emulated.cosine < EMULATED_COSINE_MIN
                || emulated.max_abs > EMULATED_MAX_ABS_FRACTION * emulated.max_abs_expected
            {
                failures.push(format!("{label}: emulated {emulated:?}"));
            }
            let stats = check_layer(
                kernels,
                &layer,
                &bits,
                &x,
                tokens,
                OracleActivations::Fp32,
                &label,
            );
            if stats.cosine < REAL_COSINE_MIN
                || stats.p95_rel > REAL_P95_REL_MAX
                || stats.max_abs > REAL_MAX_ABS_FRACTION * stats.max_abs_expected
            {
                failures.push(format!("{label}: fp32 {stats:?}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "outside tolerance:\n{}",
        failures.join("\n")
    );
}

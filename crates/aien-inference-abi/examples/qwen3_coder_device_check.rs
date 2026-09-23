//! Parity + timing: GPU MoE device forward vs CPU reference through one
//! real layer. Usage: qwen3_coder_device_check [checkpoint_dir] [layer]
//!
//! PASS bar: cosine(device, cpu_fp32) >= 0.999 and per-call device p50 faster
//! than the CPU reference on the same input.

use std::time::Instant;

use aien_inference_abi::moe_plan::MoeBatchPlan;
use aien_inference_abi::qwen3_coder::load_qwen3_coder;
use aien_inference_abi::qwen3_moe::{
    parity_stats, reference_moe_forward, reference_router_logits, Qwen3MoeKernels,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let checkpoint = args.next().map(std::path::PathBuf::from).unwrap_or_else(|| {
        "/home/drakestapleton/.cache/huggingface/hub/models--Qwen--Qwen3-Coder-30B-A3B-Instruct-FP8/snapshots/dcaee4d4dfc5ee71ad501f01f530e5652438fde0".into()
    });
    let layer: usize = args.next().map(|s| s.parse().unwrap()).unwrap_or(23);

    let t0 = Instant::now();
    let weights = load_qwen3_coder(&checkpoint).expect("load weights");
    eprintln!("load: {:.1}s", t0.elapsed().as_secs_f32());
    let kernels = Qwen3MoeKernels::load(&Qwen3MoeKernels::default_path()).expect("load .so");
    let moe = &weights.moes[layer];

    // Deterministic pseudo-activation: sin-based, scaled like a post-norm row.
    let h = 2048usize;
    let post: Vec<f32> = (0..h)
        .map(|i| (i as f32 * 0.37).sin() * 1.7 + (i as f32 * 0.11).cos() * 0.6)
        .collect();

    let t_cpu = Instant::now();
    let router = reference_router_logits(moe, &post, 1);
    let plan = MoeBatchPlan::qwen3_coder_a3b(&router, 1).unwrap();
    let cpu = reference_moe_forward(moe, &post, &plan);
    let cpu_ms = t_cpu.elapsed().as_secs_f64() * 1000.0;

    let post_bf16: Vec<u16> = post
        .iter()
        .map(|&v| aien_inference_abi::qwen3_moe::f32_to_bf16(v))
        .collect();
    // Warmup, then timed repeats.
    kernels.forward(moe, &post_bf16, 1).expect("warmup");
    let t_dev = Instant::now();
    let mut dev = None;
    for _ in 0..10 {
        dev = Some(kernels.forward(moe, &post_bf16, 1).expect("device forward"));
    }
    let dev_ms = t_dev.elapsed().as_secs_f64() * 100.0;
    let dev = dev.unwrap();

    let stats = parity_stats(&dev.output, &cpu);
    println!(
        "layer {layer}: cosine={:.8} max_abs={:.6}",
        stats.cosine, stats.max_abs
    );
    println!("cpu {cpu_ms:.1}ms/call vs device {dev_ms:.2}ms/call");
    let pass = stats.cosine >= 0.999 && dev_ms < cpu_ms;
    println!("{}", if pass { "PASS" } else { "FAIL" });
    if !pass {
        std::process::exit(1);
    }
}

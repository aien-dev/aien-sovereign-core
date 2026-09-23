//! Batch-size curve for one official Qwen3-Coder-A3B FP8 expert layer on GB10.
//!
//! cargo run --release -p aien-inference-abi --example qwen3_moe_bench -- \
//!     <checkpoint-dir> [--layer 0] [--tokens 1,8,16,32,64,128,256,500,1024] \
//!     [--warmup 1] [--repeats 5] [--seed 42] [--out receipt.json]
//!
//! Prints a JSON receipt that identifies exactly what produced each timing:
//! git commit, capsule digest, kernel source and library digests, Mojo version,
//! driver/CUDA version, device name, input seed and digest, and raw samples.

use aien_inference_abi::{
    load_qwen3_fp8_moe_layer, reference_router_logits, seeded_bf16_input, source_digest,
    MoeBatchPlan, Qwen3MoeKernels,
};
use std::path::{Path, PathBuf};
use std::process::Command;

fn command_output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unavailable".to_string())
}

fn file_digest(path: &Path) -> String {
    std::fs::read(path)
        .map(|bytes| source_digest(&bytes))
        .unwrap_or_else(|e| format!("unreadable: {e}"))
}

fn peak_host_rss_bytes() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        })
        .map(|kib| kib * 1024)
        .unwrap_or(0)
}

fn median(samples: &mut [u64]) -> f64 {
    samples.sort_unstable();
    let n = samples.len();
    if n % 2 == 1 {
        samples[n / 2] as f64
    } else {
        (samples[n / 2 - 1] + samples[n / 2]) as f64 / 2.0
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let checkpoint = PathBuf::from(
        args.next()
            .expect("usage: qwen3_moe_bench <checkpoint-dir> [options]"),
    );
    let (mut layer_index, mut warmup, mut repeats, mut seed) = (0usize, 1usize, 5usize, 42u64);
    let mut token_counts = vec![1usize, 8, 16, 32, 64, 128, 256, 500, 1024];
    let mut out: Option<PathBuf> = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--layer" => layer_index = value.parse().expect("--layer"),
            "--warmup" => warmup = value.parse().expect("--warmup"),
            "--repeats" => repeats = value.parse().expect("--repeats"),
            "--seed" => seed = value.parse().expect("--seed"),
            "--tokens" => {
                token_counts = value
                    .split(',')
                    .map(|t| t.parse().expect("--tokens"))
                    .collect()
            }
            "--out" => out = Some(PathBuf::from(value)),
            other => panic!("unknown flag {other}"),
        }
    }

    let kernels = Qwen3MoeKernels::load(&Qwen3MoeKernels::default_path())
        .expect("build mojo/qwen3_moe first");
    let layer = load_qwen3_fp8_moe_layer(&checkpoint, layer_index).expect("load layer");
    let kernel_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("mojo/qwen3_moe");

    let mut points = Vec::new();
    for &tokens in &token_counts {
        let (bits, x) = seeded_bf16_input(seed, tokens, layer.hidden, 0.25);
        let input_bytes: Vec<u8> = bits.iter().flat_map(|b| b.to_le_bytes()).collect();
        let mut samples = kernels
            .benchmark(&layer, &bits, tokens, warmup, repeats)
            .expect("benchmark");
        let raw_samples = samples.clone();
        let median_ns = median(&mut samples);
        let plan =
            MoeBatchPlan::qwen3_coder_a3b(&reference_router_logits(&layer, &x, tokens), tokens)
                .expect("routing");
        let telemetry = plan.telemetry();
        eprintln!(
            "tokens={tokens:5} median={:9.3} ms  {:9.1} tok/s",
            median_ns / 1e6,
            tokens as f64 / (median_ns / 1e9)
        );
        points.push(serde_json::json!({
            "tokens": tokens,
            "assignments": tokens * 8,
            "input_seed": seed,
            "input_digest": source_digest(&input_bytes),
            "layer_latency_ms_median": median_ns / 1e6,
            "layer_tokens_per_sec_median": tokens as f64 / (median_ns / 1e9),
            "assignments_per_sec_median": (tokens * 8) as f64 / (median_ns / 1e9),
            "samples_ns": raw_samples,
            "experts_touched": telemetry.experts_touched,
            "expert_batch_p50": telemetry.expert_batch_p50,
            "expert_batch_p95": telemetry.expert_batch_p95,
            "expert_batch_max": telemetry.max_expert_batch,
            "routing_entropy_bits": telemetry.routing_entropy_bits,
        }));
    }

    let receipt = serde_json::json!({
        "model": "Qwen/Qwen3-Coder-30B-A3B-Instruct-FP8",
        "layer": layer_index,
        "expert_compute": "native grouped FP8 E4M3 mma.sync m16n8k32 (Mojo, SM121a)",
        "weight_dtype": "fp8_e4m3fn with bf16 128x128 inverse scales",
        "resident_dtype": "fp8_e4m3fn",
        "activation_quantization": "fp8_e4m3fn per row and 128 channels, fp32 scales",
        "capsule_digest": layer.capsule.capsule_digest,
        "capsule_source_digest": layer.capsule.source_digest,
        "checkpoint_index_digest": file_digest(&checkpoint.join("model.safetensors.index.json")),
        "resident_weight_bytes": layer.resident_weight_bytes(),
        "git_commit": command_output("git", &["-C", env!("CARGO_MANIFEST_DIR"), "rev-parse", "HEAD"]),
        "git_dirty": !command_output("git", &["-C", env!("CARGO_MANIFEST_DIR"), "status", "--porcelain"]).is_empty(),
        "kernel_source_digests": {
            "kernels.mojo": file_digest(&kernel_dir.join("kernels.mojo")),
            "lib.mojo": file_digest(&kernel_dir.join("lib.mojo")),
        },
        "kernel_library": kernels.path.display().to_string(),
        "kernel_library_digest": file_digest(&kernels.path),
        "mojo_version": command_output("mojo", &["--version"]),
        "gpu": command_output("nvidia-smi", &["--query-gpu=name,driver_version,compute_cap", "--format=csv,noheader"]),
        "cuda_version": command_output("nvidia-smi", &[]).lines().find(|l| l.contains("CUDA Version")).map(|l| l.trim().to_string()).unwrap_or_default(),
        "warmup": warmup,
        "repeats": repeats,
        "timing": "wall time from first enqueue to device synchronize, weights and input resident",
        "peak_host_rss_bytes": peak_host_rss_bytes(),
        "note": "one MoE layer; excludes attention, norms, embedding and sampling",
        "points": points,
    });
    let text = serde_json::to_string_pretty(&receipt).unwrap();
    match out {
        Some(path) => std::fs::write(&path, text + "\n").expect("write receipt"),
        None => println!("{text}"),
    }
}

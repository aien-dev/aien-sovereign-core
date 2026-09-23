//! Serve-library validation: uploads all 30GB once, then compares one full
//! device token against the CPU reference and times both.
//! Usage: qwen3_serve_check [checkpoint_dir]
//!
//! PASS bar: cosine(device_hidden, cpu_hidden) >= 0.999.

use std::time::Instant;

use aien_inference_abi::qwen3_coder::{forward_token, Qwen3CoderState};
use aien_inference_abi::qwen3_moe::parity_stats;
use aien_inference_abi::qwen3_serve::{forward_token_serve, QwenServeLib, QwenServeModel};

fn main() {
    let checkpoint = std::env::args().nth(1).map(std::path::PathBuf::from).unwrap_or_else(|| {
        "/home/drakestapleton/.cache/huggingface/hub/models--Qwen--Qwen3-Coder-30B-A3B-Instruct-FP8/snapshots/dcaee4d4dfc5ee71ad501f01f530e5652438fde0".into()
    });

    let t0 = Instant::now();
    let weights =
        aien_inference_abi::qwen3_coder::load_qwen3_coder(&checkpoint).expect("load weights");
    eprintln!("cpu load: {:.1}s", t0.elapsed().as_secs_f32());

    let t0 = Instant::now();
    let lib = QwenServeLib::load(&QwenServeLib::default_path()).expect("load serve .so");
    let serve = QwenServeModel::upload_all(lib, &weights, &checkpoint).expect("upload all");
    eprintln!("device upload: {:.1}s", t0.elapsed().as_secs_f32());

    // One real token through both paths (fresh KV each).
    let token = 42u32;
    let t_cpu = Instant::now();
    let cpu = forward_token(&weights, token, 0, &mut Qwen3CoderState::new());
    let cpu_ms = t_cpu.elapsed().as_secs_f64() * 1000.0;

    // Warmup, then timed.
    forward_token_serve(
        &weights,
        &serve,
        token,
        0,
        &mut Qwen3CoderState::new(),
        None,
        false,
    )
    .expect("warmup");
    let t_dev = Instant::now();
    let mut prof = [0.0f64; 5];
    let dev = forward_token_serve(
        &weights,
        &serve,
        token,
        0,
        &mut Qwen3CoderState::new(),
        Some(&mut prof),
        false,
    )
    .expect("device forward");
    let dev_ms = t_dev.elapsed().as_secs_f64() * 1000.0;

    let stats = parity_stats(&dev, &cpu);
    println!("cosine={:.8} max_abs={:.6}", stats.cosine, stats.max_abs);
    println!("cpu {cpu_ms:.0}ms/token vs device {dev_ms:.1}ms/token");
    println!(
        "profile ms/token: proj_qkv={:.1} attn_cpu={:.1} proj_o={:.1} moe={:.1} norm_misc={:.1}",
        prof[0], prof[1], prof[2], prof[3], prof[4]
    );
    let pass = stats.cosine >= 0.999;
    println!("{}", if pass { "PASS" } else { "FAIL" });
    if !pass {
        std::process::exit(1);
    }

    // Full device path including GPU attention, same token, fresh device KV.
    let t_full = Instant::now();
    let full = forward_token_serve(
        &weights,
        &serve,
        token,
        0,
        &mut Qwen3CoderState::new(),
        None,
        true,
    )
    .expect("full device forward");
    let full_ms = t_full.elapsed().as_secs_f64() * 1000.0;
    let stats_full = parity_stats(&full, &cpu);
    let stats_iso = parity_stats(&full, &dev);
    println!(
        "full-device: cosine_vs_cpu={:.8} cosine_vs_gemm_path={:.8} max_abs={:.6}",
        stats_full.cosine, stats_iso.cosine, stats_full.max_abs
    );
    println!("full-device {full_ms:.1}ms/token");
    let pass_full = stats_full.cosine >= 0.999;
    println!("{}", if pass_full { "PASS-FULL" } else { "FAIL-FULL" });
    if !pass_full {
        std::process::exit(1);
    }
}

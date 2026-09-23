//! Multi-position logits parity: CPU reference vs device GEMM path vs full
//! device path (GPU attention), teacher-forced on the CPU's greedy tokens so
//! all three see identical inputs at every position.
//!
//! Usage: qwen3_parity_check [checkpoint_dir] [positions]
//!   checkpoint_dir: else $AIEN_QWEN3_CODER_CKPT, else the HF cache under $HOME.
//!   positions: default 16.
//!
//! PASS bars, at every position:
//!   device logits vs CPU: cosine >= 0.999 and identical argmax.
//!   full-device vs GEMM path (attention isolation): cosine >= 0.99999.

use std::path::PathBuf;
use std::time::Instant;

use aien_inference_abi::qwen3_coder::{
    compute_logits, forward_token, forward_token_with_moe, MoeBackend, Qwen3CoderState,
    QWEN3_CODER_VOCAB,
};
use aien_inference_abi::qwen3_moe::parity_stats;
use aien_inference_abi::qwen3_serve::{forward_token_serve, QwenServeLib, QwenServeModel};
use aien_inference_abi::tensor::sample_argmax;

const SNAPSHOT: &str = ".cache/huggingface/hub/models--Qwen--Qwen3-Coder-30B-A3B-Instruct-FP8/snapshots/dcaee4d4dfc5ee71ad501f01f530e5652438fde0";

/// "def fibonacci(n):" style code prefix; any fixed ids work for parity.
const PROMPT: [u32; 6] = [750, 75698, 1445, 982, 262, 421];

fn checkpoint_dir(arg: Option<String>) -> PathBuf {
    arg.map(PathBuf::from)
        .or_else(|| std::env::var_os("AIEN_QWEN3_CODER_CKPT").map(PathBuf::from))
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").expect("HOME or a checkpoint argument"))
                .join(SNAPSHOT)
        })
}

fn device_logits(serve: &QwenServeModel, hidden: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0f32; QWEN3_CODER_VOCAB];
    serve.logits(hidden, &mut out).expect("device logits");
    out
}

fn main() {
    let mut args = std::env::args().skip(1);
    let checkpoint = checkpoint_dir(args.next());
    let positions: usize = args
        .next()
        .map(|s| s.parse().expect("positions"))
        .unwrap_or(16);
    assert!(positions >= PROMPT.len(), "positions must cover the prompt");

    let t0 = Instant::now();
    let weights =
        aien_inference_abi::qwen3_coder::load_qwen3_coder(&checkpoint).expect("load weights");
    eprintln!("cpu load: {:.1}s", t0.elapsed().as_secs_f32());
    let lib = QwenServeLib::load(&QwenServeLib::default_path()).expect("load serve .so");
    let serve = QwenServeModel::upload_all(lib, &weights, &checkpoint).expect("upload all");

    let mut cpu_state = Qwen3CoderState::new();
    let mut gemm_state = Qwen3CoderState::new();
    let mut full_state = Qwen3CoderState::new();
    let mut cpu16_state = Qwen3CoderState::new();

    let mut token = PROMPT[0];
    let (mut min_cos, mut min_iso, mut argmax_misses) = (1.0f64, 1.0f64, 0usize);
    let mut min_cos16 = 1.0f64;
    println!(
        "pos token  cos(dev,cpu)  cos(full,gemm)  cos(gemm,cpu16)  cpu_top2_margin  argmax cpu/gemm/full"
    );
    for pos in 0..positions {
        let cpu_h = forward_token(&weights, token, pos, &mut cpu_state);
        // Diagnostic: CPU reference with the device's BF16 MoE I/O rounding.
        let cpu16_h = forward_token_with_moe(
            &weights,
            token,
            pos,
            &mut cpu16_state,
            &mut MoeBackend::CpuBf16Io,
        );
        let gemm_h =
            forward_token_serve(&weights, &serve, token, pos, &mut gemm_state, None, false)
                .expect("gemm-path forward");
        let full_h = forward_token_serve(&weights, &serve, token, pos, &mut full_state, None, true)
            .expect("full-device forward");

        let cpu_l = compute_logits(&weights, &cpu_h);
        let gemm_l = device_logits(&serve, &gemm_h);
        let full_l = device_logits(&serve, &full_h);

        let cos = parity_stats(&gemm_l, &cpu_l)
            .cosine
            .min(parity_stats(&full_l, &cpu_l).cosine);
        let iso = parity_stats(&full_l, &gemm_l).cosine;
        let (a_cpu, _) = sample_argmax(&cpu_l);
        let (a_gemm, _) = sample_argmax(&gemm_l);
        let (a_full, _) = sample_argmax(&full_l);
        if a_gemm != a_cpu || a_full != a_cpu {
            argmax_misses += 1;
        }
        min_cos = min_cos.min(cos);
        min_iso = min_iso.min(iso);
        let cos16 = parity_stats(&gemm_l, &compute_logits(&weights, &cpu16_h)).cosine;
        min_cos16 = min_cos16.min(cos16);
        let mut sorted = cpu_l.clone();
        sorted.sort_unstable_by(|a, b| b.total_cmp(a));
        let margin = sorted[0] - sorted[1];
        println!(
            "{pos:>3} {token:>6}  {cos:.8}    {iso:.8}      {cos16:.8}       {margin:>8.4}        {a_cpu}/{a_gemm}/{a_full}"
        );

        // Teacher-force: prompt first, then the CPU's greedy choice.
        token = PROMPT.get(pos + 1).copied().unwrap_or(a_cpu);
    }

    let pass = min_cos >= 0.999 && argmax_misses == 0 && min_iso >= 0.99999;
    println!(
        "positions={positions} min_cos(dev,cpu)={min_cos:.8} min_cos(full,gemm)={min_iso:.8} min_cos(gemm,cpu16)={min_cos16:.8} argmax_misses={argmax_misses}"
    );
    println!("{}", if pass { "PASS" } else { "FAIL" });
    if !pass {
        std::process::exit(1);
    }
}

//! C1 SPEC 5.1 / 5.3 noise-floor calibration for Qwen3-Coder-30B-A3B-FP8.
//!
//! Measures correct-vs-correct logit differences across frozen seeded prompts
//! and positions, and the same metrics for candidate (device vs CPU) pairs.
//! Emits a JSON receipt with per-pair distributions; thresholds are NOT
//! chosen here. Freezing them is a versioned SPEC amendment (Section 14).
//!
//! Usage: qwen3_noise_floor <out.json> [checkpoint_dir] [prompts] [positions]
//!
//! Pairs (reference first):
//!   floor.rerun      gemm path vs itself, fresh state (determinism)
//!   floor.attention  gemm path (f64 CPU attn) vs full device (f32 GPU attn)
//!   floor.moe_bf16   CPU f32 MoE I/O vs CPU BF16 MoE I/O
//!   cand.dev_cpu     CPU f32 reference vs gemm path
//!   cand.dev_cpu16   CPU BF16-I/O reference vs gemm path
//! Metrics per compared step (SPEC 5.1): max_abs, max_rel (eps 1e-3), KL(ref||test),
//! top-1 agreement with reference top-2 margin, plus cosine for continuity.
//!
//! Not the SPEC 5.4 Hugging Face reference: this calibrates AIEN paths only.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use aien_inference_abi::qwen3_coder::{
    compute_logits, forward_token_with_moe, MoeBackend, Qwen3CoderState, QWEN3_CODER_VOCAB,
};
use aien_inference_abi::qwen3_moe::parity_stats;
use aien_inference_abi::qwen3_serve::{forward_token_serve, QwenServeLib, QwenServeModel};
use aien_inference_abi::tensor::sample_argmax;

const SNAPSHOT: &str = ".cache/huggingface/hub/models--Qwen--Qwen3-Coder-30B-A3B-Instruct-FP8/snapshots/dcaee4d4dfc5ee71ad501f01f530e5652438fde0";
const REL_EPS: f64 = 1.0e-3; // acceptance.toml relative_error_epsilon
const PROMPT_LEN: usize = 6;
const BASE_SEED: u64 = 0xC1_0000_0000_0053; // frozen: campaign C1, Section 5.3

#[derive(Default)]
struct Series {
    max_abs: Vec<f64>,
    max_rel: Vec<f64>,
    kl: Vec<f64>,
    cosine: Vec<f64>,
    top1_disagree: Vec<(usize, usize, f64)>, // (prompt, pos, reference top-2 margin)
    steps: usize,
}

fn softmax_f64(l: &[f32]) -> Vec<f64> {
    let m = l.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b)) as f64;
    let e: Vec<f64> = l.iter().map(|&x| (x as f64 - m).exp()).collect();
    let s: f64 = e.iter().sum();
    e.into_iter().map(|x| x / s).collect()
}

fn top2_margin(l: &[f32]) -> f64 {
    let (mut a, mut b) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for &x in l {
        if x > a {
            b = a;
            a = x;
        } else if x > b {
            b = x;
        }
    }
    (a - b) as f64
}

impl Series {
    fn push(&mut self, reference: &[f32], test: &[f32], prompt: usize, pos: usize) {
        let (mut max_abs, mut max_rel) = (0.0f64, 0.0f64);
        for (&r, &t) in reference.iter().zip(test) {
            let d = (r as f64 - t as f64).abs();
            max_abs = max_abs.max(d);
            max_rel = max_rel.max(d / (r as f64).abs().max(REL_EPS));
        }
        let (p, q) = (softmax_f64(reference), softmax_f64(test));
        let kl: f64 = p
            .iter()
            .zip(&q)
            .filter(|(&pi, _)| pi > 0.0)
            .map(|(&pi, &qi)| pi * (pi / qi.max(f64::MIN_POSITIVE)).ln())
            .sum();
        self.max_abs.push(max_abs);
        self.max_rel.push(max_rel);
        self.kl.push(kl);
        self.cosine.push(parity_stats(test, reference).cosine);
        if sample_argmax(reference).0 != sample_argmax(test).0 {
            self.top1_disagree
                .push((prompt, pos, top2_margin(reference)));
        }
        self.steps += 1;
    }
}

/// Nearest-rank quantile over a sorted copy (p = 0.0 min, 1.0 max).
fn q(v: &[f64], p: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    s[((s.len() - 1) as f64 * p).round() as usize]
}

fn summary(s: &Series) -> serde_json::Value {
    let hi =
        |v: &[f64]| serde_json::json!({"worst": q(v, 1.0), "p99": q(v, 0.99), "p50": q(v, 0.5)});
    let lo =
        |v: &[f64]| serde_json::json!({"worst": q(v, 0.0), "p01": q(v, 0.01), "p50": q(v, 0.5)});
    serde_json::json!({
        "steps": s.steps,
        "max_abs": hi(&s.max_abs),
        "max_rel": hi(&s.max_rel),
        "kl": hi(&s.kl),
        "cosine": lo(&s.cosine),
        "top1_disagreements": s.top1_disagree.iter()
            .map(|(p, t, m)| serde_json::json!({"prompt": p, "pos": t, "ref_top2_margin": m}))
            .collect::<Vec<_>>(),
    })
}

fn device_logits(serve: &QwenServeModel, hidden: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0f32; QWEN3_CODER_VOCAB];
    serve.logits(hidden, &mut out).expect("device logits");
    out
}

fn prompt(seed: u64) -> Vec<u32> {
    let mut x = seed | 1;
    (0..PROMPT_LEN)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            // Keep to the regular-token range (below special tokens).
            (x % 151_000) as u32
        })
        .collect()
}

fn sha256_file(path: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    std::fs::read(path)
        .map(|b| format!("{:x}", Sha256::digest(&b)))
        .unwrap_or_else(|e| format!("unreadable: {e}"))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out_path = PathBuf::from(
        args.next()
            .expect("usage: qwen3_noise_floor <out.json> ..."),
    );
    let checkpoint = args
        .next()
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("AIEN_QWEN3_CODER_CKPT").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").expect("HOME")).join(SNAPSHOT));
    let n_prompts: usize = args
        .next()
        .map(|s| s.parse().expect("prompts"))
        .unwrap_or(6);
    let positions: usize = args
        .next()
        .map(|s| s.parse().expect("positions"))
        .unwrap_or(24);
    assert!(positions > PROMPT_LEN);

    let started = Instant::now();
    let weights =
        aien_inference_abi::qwen3_coder::load_qwen3_coder(&checkpoint).expect("load weights");
    let lib_path = QwenServeLib::default_path();
    let lib = QwenServeLib::load(&lib_path).expect("load serve .so");
    let serve = QwenServeModel::upload_all(lib, &weights, &checkpoint).expect("upload all");
    eprintln!("load+upload: {:.1}s", started.elapsed().as_secs_f32());

    let names = [
        "floor.rerun",
        "floor.attention",
        "floor.moe_bf16",
        "cand.dev_cpu",
        "cand.dev_cpu16",
    ];
    let mut series: BTreeMap<&str, Series> =
        names.iter().map(|n| (*n, Series::default())).collect();

    for p in 0..n_prompts {
        let tokens = prompt(BASE_SEED.wrapping_add(p as u64));
        let mut cpu = Qwen3CoderState::new();
        let mut cpu16 = Qwen3CoderState::new();
        let mut gemm = Qwen3CoderState::new();
        let mut gemm2 = Qwen3CoderState::new();
        let mut full = Qwen3CoderState::new();
        let mut token = tokens[0];
        for pos in 0..positions {
            let h_cpu =
                forward_token_with_moe(&weights, token, pos, &mut cpu, &mut MoeBackend::Cpu);
            let h_cpu16 = forward_token_with_moe(
                &weights,
                token,
                pos,
                &mut cpu16,
                &mut MoeBackend::CpuBf16Io,
            );
            let h_gemm = forward_token_serve(&weights, &serve, token, pos, &mut gemm, None, false)
                .expect("gemm");
            let h_gemm2 =
                forward_token_serve(&weights, &serve, token, pos, &mut gemm2, None, false)
                    .expect("gemm rerun");
            let h_full = forward_token_serve(&weights, &serve, token, pos, &mut full, None, true)
                .expect("full");

            let l_cpu = compute_logits(&weights, &h_cpu);
            let l_cpu16 = compute_logits(&weights, &h_cpu16);
            let l_gemm = device_logits(&serve, &h_gemm);
            let l_gemm2 = device_logits(&serve, &h_gemm2);
            let l_full = device_logits(&serve, &h_full);

            series
                .get_mut("floor.rerun")
                .unwrap()
                .push(&l_gemm, &l_gemm2, p, pos);
            series
                .get_mut("floor.attention")
                .unwrap()
                .push(&l_gemm, &l_full, p, pos);
            series
                .get_mut("floor.moe_bf16")
                .unwrap()
                .push(&l_cpu, &l_cpu16, p, pos);
            series
                .get_mut("cand.dev_cpu")
                .unwrap()
                .push(&l_cpu, &l_gemm, p, pos);
            series
                .get_mut("cand.dev_cpu16")
                .unwrap()
                .push(&l_cpu16, &l_gemm, p, pos);

            // Teacher-force on the CPU f32 reference's greedy token.
            token = tokens
                .get(pos + 1)
                .copied()
                .unwrap_or(sample_argmax(&l_cpu).0);
        }
        eprintln!(
            "prompt {}/{} done at {:.0}s",
            p + 1,
            n_prompts,
            started.elapsed().as_secs_f32()
        );
    }

    let git_rev = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let receipt = serde_json::json!({
        "schema": "aien.c1.noise_floor.v1",
        "model": "qwen3_coder_30b_a3b_fp8",
        "spec": {"campaign": "c1", "spec_version": 1, "sections": ["5.1", "5.3"]},
        "not_hf_reference": true,
        "git_rev": git_rev,
        "serve_lib": {"path": lib_path.display().to_string(), "sha256": sha256_file(&lib_path)},
        "checkpoint": checkpoint.display().to_string(),
        "prompts": n_prompts, "positions": positions, "prompt_len": PROMPT_LEN,
        "base_seed": format!("{BASE_SEED:#x}"),
        "relative_error_epsilon": REL_EPS,
        "teacher_forcing": "cpu_f32_greedy",
        "wall_seconds": started.elapsed().as_secs_f64(),
        "pairs": series.iter().map(|(k, s)| (k.to_string(), summary(s))).collect::<BTreeMap<_, _>>(),
    });
    std::fs::write(&out_path, serde_json::to_string_pretty(&receipt).unwrap())
        .expect("write receipt");
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt["pairs"]).unwrap()
    );
    println!("receipt: {}", out_path.display());
}

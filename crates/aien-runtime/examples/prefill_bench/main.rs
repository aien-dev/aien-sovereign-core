//! PREFILL-BENCH (convergence item 6): AIEN shared prefill / N-branch fork
//! versus a MODELED vLLM-style automatic-prefix-cache baseline, same model,
//! same prompts, same CPU, same tensor backend.
//!
//! One command (release build; real checkpoint; takes tens of minutes):
//!
//!   AIEN_BENCH_CHECKPOINT=$HOME/models/TinyLlama-1.1B-Chat-v1.0 \
//!   AIEN_BENCH_OUT=/tmp/prefill_bench.json \
//!   cargo run --release -p aien-runtime --example prefill_bench
//!
//! Knobs (all optional): AIEN_BENCH_N="1,2,4,8,16", AIEN_BENCH_LENS="40,200,700"
//! (prefix length in tokens: short, medium, long), AIEN_BENCH_STEPS=8,
//! AIEN_BENCH_REPS=1, AIEN_BENCH_PARITY_BRANCHES=2.
//! Unit tests of the baseline model: `cargo test -p aien-runtime --example prefill_bench`.
//!
//! Arms, per (N, prefix length) cell:
//! - AIEN: the real spine path (launch_swarm, scheduler, one pooled paged KV,
//!   root prefill once, fence, N forks with copy-on-write). Branch tokens come
//!   from AIEN's own per-branch seeded sampling.
//! - BASELINE (modeled vLLM APC): N independent requests with the same prompt.
//!   Request 1 prefills everything. Requests 2..N get a cache hit on the full
//!   blocks (<= len-1 tokens, see vllm_model.rs), by copying the donor's
//!   first-F-token K/V (copy time is NOT charged, which favors the baseline)
//!   and really prefilling only the remaining tokens with the same backend.
//!   Decode is the same batched decode kernel. Decode inputs are teacher
//!   forced with AIEN's sampled tokens so both arms do identical work.
//!   Physical KV bytes of the baseline come from the bookkeeping model, not a
//!   measurement of a real vLLM.
//! - CONTROL: fresh unpaged backend, one-shot prefill, teacher forced. Used for
//!   correctness parity on the first AIEN_BENCH_PARITY_BRANCHES branches. Its
//!   one-shot prefill time is also the per-copy cost of a NO-sharing engine,
//!   used to model "naive" time and bytes (N x).

mod vllm_model;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aien_inference_abi::{
    AienInferenceBackend, DecodeOutput, FinishReason, ModelConfig, NativeTransformerBackend,
    ReferenceCpuBackend, SamplingParams, ScheduledBatch, SequenceState, StepMetrics,
    TinyLlamaTokenizer, TransformerWeights,
};
use aien_runtime::shared_kv::{build_shared_kv_runtime, SharedKvSizing};
use aien_runtime::swarm::SwarmConfig;
use aien_scheduler::SchedulerConfig;
use async_trait::async_trait;
use serde_json::json;
use vllm_model::PrefixCacheModel;

const PREFILL_CHUNK: usize = 128;
const SYSTEM_PROMPT: &str = "You are a careful engineering assistant. You answer in plain \
English, you show the steps of your reasoning, and you never invent numbers you were not given.";
const USER_PROMPT: &str = "A small workshop builds wooden chairs. Each chair needs four legs, \
one seat, two side rails, one back rest and twelve screws. The workshop has ninety legs, \
twenty seats, forty side rails, eighteen back rests and two hundred screws in stock. Two of \
the seats are cracked and cannot be used, and one box of twenty screws turned out to be the \
wrong size. The carpenter wants to know how many complete chairs can be built today, which \
part runs out first, and how many of every other part will be left over afterwards. After \
that, suggest the smallest order of extra parts that would let the workshop build exactly \
twenty five chairs tomorrow, and explain briefly why that order is the smallest one. ";

fn env_list(name: &str, default: &str) -> Vec<usize> {
    std::env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .split(',')
        .map(|s| {
            s.trim()
                .parse()
                .unwrap_or_else(|_| panic!("{name}: bad number {s:?}"))
        })
        .collect()
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .map(|v| v.parse().unwrap_or_else(|_| panic!("{name}: bad number")))
        .unwrap_or(default)
}

// ---------------------------------------------------------------------------
// AIEN arm: recording backend around the real NativeTransformerBackend
// ---------------------------------------------------------------------------

struct Tap {
    inner: NativeTransformerBackend,
    branches: Vec<u64>,
    prompt_len: usize,
    launch_at: Instant,
    prefill_wall: Duration,
    prefill_tokens: usize,
    decode_wall: Duration,
    decode_steps: usize,
    decode_logits: HashMap<u64, Vec<Vec<f32>>>,
    decode_tokens: HashMap<u64, Vec<u32>>,
    first_token_at: HashMap<u64, Duration>,
    root_token: Option<u32>,
    first_decode_snapshot: Option<(usize, usize, usize)>, // physical, logical, shared pages
    peak_physical_pages: usize,
    block_bytes: usize,
}

#[async_trait]
impl AienInferenceBackend for Tap {
    async fn load_model(&mut self, _c: &ModelConfig) -> Result<(), String> {
        Ok(())
    }

    async fn execute_step(
        &mut self,
        batch: &ScheduledBatch,
    ) -> Result<(Vec<DecodeOutput>, StepMetrics), String> {
        let mut outputs = Vec::new();
        let mut metrics = StepMetrics::default();
        if !batch.prefill_requests.is_empty() {
            self.prefill_tokens += batch
                .prefill_requests
                .iter()
                .map(|r| r.prompt_tokens.len())
                .sum::<usize>();
            let prefill_only = ScheduledBatch {
                decode_requests: Vec::new(),
                ..batch.clone()
            };
            let t0 = Instant::now();
            let (o, m) = self.inner.execute_step(&prefill_only).await?;
            self.prefill_wall += t0.elapsed();
            outputs.extend(o);
            metrics = m;
        }
        if !batch.decode_requests.is_empty() {
            if self.first_decode_snapshot.is_none() {
                if let Some(kv) = self.inner.kv_manager.as_ref() {
                    let m = kv.read().metrics();
                    self.first_decode_snapshot =
                        Some((m.physical_pages, m.logical_pages, m.shared_pages));
                }
                self.root_token = batch.decode_requests.first().and_then(|id| {
                    self.inner
                        .pending_prefill_token
                        .get(id)
                        .copied()
                        .or_else(|| {
                            self.inner
                                .sequences
                                .get(id)
                                .and_then(|s| s.tokens.get(self.prompt_len).copied())
                        })
                });
            }
            let t0 = Instant::now();
            let (o, logits) = self
                .inner
                .forward_decode_batch_with_logits(&batch.decode_requests)?;
            self.decode_wall += t0.elapsed();
            self.decode_steps += 1;
            let preempted: Vec<u64> = o
                .iter()
                .filter_map(|out| match out {
                    DecodeOutput::Finished {
                        request_id,
                        reason: FinishReason::Preempted,
                        ..
                    } => Some(*request_id),
                    _ => None,
                })
                .collect();
            let computed = batch
                .decode_requests
                .iter()
                .filter(|id| !preempted.contains(*id));
            for (id, row) in computed.zip(logits) {
                self.decode_logits.entry(*id).or_default().push(row);
                if let Some(tok) = self.inner.sequences.get(id).and_then(|s| s.tokens.last()) {
                    self.decode_tokens.entry(*id).or_default().push(*tok);
                }
                self.first_token_at
                    .entry(*id)
                    .or_insert_with(|| self.launch_at.elapsed());
            }
            if let Some(kv) = self.inner.kv_manager.as_ref() {
                let pages = kv.read().metrics().physical_pages;
                self.peak_physical_pages = self.peak_physical_pages.max(pages);
            }
            outputs.extend(o);
            metrics.decode_tokens_emitted += batch.decode_requests.len();
        }
        Ok((outputs, metrics))
    }

    fn manages_kv_cache(&self) -> bool {
        self.inner.manages_kv_cache()
    }

    fn fork_sequence(&mut self, parent: u64, child: u64) -> Result<(), String> {
        self.inner.fork_sequence(parent, child)
    }

    fn fork_sequence_with_sampling(
        &mut self,
        parent: u64,
        child: u64,
        sampling: &SamplingParams,
    ) -> Result<(), String> {
        self.inner
            .fork_sequence_with_sampling(parent, child, sampling)
    }

    fn release_sequence(&mut self, seq_id: u64) -> Result<(), String> {
        AienInferenceBackend::release_sequence(&mut self.inner, seq_id)
    }
}

struct AienRun {
    prefill_s: f64,
    prefill_tokens: usize,
    decode_s: f64,
    total_s: f64,
    ttft_ms: Vec<f64>,
    physical_first_decode: usize,
    logical_first_decode: usize,
    peak_pages: usize,
    block_bytes: usize,
    allocated_after: usize,
    root_token: u32,
    tokens: Vec<Vec<u32>>,
    logits: Vec<Vec<Vec<f32>>>,
    error: Option<String>,
    diag: String,
}

async fn run_aien(weights: &TransformerWeights, prompt: &[u32], n: usize, steps: usize) -> AienRun {
    let block_size = weights.config.block_size;
    let total_blocks = prompt.len().div_ceil(block_size) + n * 4 + 32;
    let scheduler = SchedulerConfig {
        max_batch_size: 64,
        max_batch_tokens: 4096,
        max_prefill_tokens: 8192,
        prefill_chunk_size: PREFILL_CHUNK,
        chunk_prefill: true,
        watermark_blocks: 2,
    };
    let (mut spine, backend) = build_shared_kv_runtime(
        weights.clone(),
        Arc::new(ReferenceCpuBackend::new()),
        scheduler,
        SharedKvSizing {
            arena_capacity: 128,
            total_blocks,
        },
    )
    .expect("build shared KV runtime");
    let kv = spine.kv_manager.clone();
    let block_bytes = kv.read().tensor_pool().map_or(0, |p| p.block_bytes());
    let launch_at = Instant::now();
    let swarm_id = spine
        .launch_swarm(
            SwarmConfig {
                model_handle: 1,
                branch_count: n,
                max_active_sequences: n,
                max_tokens_per_branch: steps,
                root_world_id: 0,
                priority: 1,
            },
            prompt,
        )
        .expect("launch swarm");
    let swarm = spine
        .swarm_manager
        .get_swarm(swarm_id)
        .expect("swarm")
        .clone();
    let branches: Vec<u64> = swarm.branch_sequences.iter().map(|s| s.as_u64()).collect();
    let mut tap = Tap {
        inner: backend,
        branches: branches.clone(),
        prompt_len: prompt.len(),
        launch_at,
        prefill_wall: Duration::ZERO,
        prefill_tokens: 0,
        decode_wall: Duration::ZERO,
        decode_steps: 0,
        decode_logits: HashMap::new(),
        decode_tokens: HashMap::new(),
        first_token_at: HashMap::new(),
        root_token: None,
        first_decode_snapshot: None,
        peak_physical_pages: 0,
        block_bytes,
    };
    let mut error = None;
    let mut guard = 0usize;
    while (spine.scheduler.running_count() > 0
        || spine.scheduler.waiting_count() > 0
        || spine.swarm_manager.is_root_prefill_pending(swarm_id))
        && guard < 1000
    {
        guard += 1;
        if let Err(e) = spine.step(&mut tap).await {
            error = Some(e);
            break;
        }
    }
    let total_s = launch_at.elapsed().as_secs_f64();
    let diag = format!(
        "spine_steps={guard} running={} waiting={} root_prefill_pending={} prefill_tokens={} decode_steps={}",
        spine.scheduler.running_count(),
        spine.scheduler.waiting_count(),
        spine.swarm_manager.is_root_prefill_pending(swarm_id),
        tap.prefill_tokens,
        tap.decode_steps
    );
    let (phys, logical, _shared) = tap.first_decode_snapshot.unwrap_or((0, 0, 0));
    let allocated_after = kv.read().allocated_block_count();
    AienRun {
        prefill_s: tap.prefill_wall.as_secs_f64(),
        prefill_tokens: tap.prefill_tokens,
        decode_s: tap.decode_wall.as_secs_f64(),
        total_s,
        ttft_ms: tap
            .branches
            .iter()
            .filter_map(|b| tap.first_token_at.get(b).map(|d| d.as_secs_f64() * 1e3))
            .collect(),
        physical_first_decode: phys,
        logical_first_decode: logical,
        peak_pages: tap.peak_physical_pages,
        block_bytes: tap.block_bytes,
        allocated_after,
        root_token: tap.root_token.unwrap_or(u32::MAX),
        tokens: branches
            .iter()
            .map(|b| tap.decode_tokens.get(b).cloned().unwrap_or_default())
            .collect(),
        logits: branches
            .iter()
            .map(|b| tap.decode_logits.get(b).cloned().unwrap_or_default())
            .collect(),
        error,
        diag,
    }
}

// ---------------------------------------------------------------------------
// Baseline arm: modeled vLLM APC policy on the same backend
// ---------------------------------------------------------------------------

struct BaseRun {
    prefill_s: f64,
    prefill_tokens: usize,
    decode_s: f64,
    total_s: f64,
    ttft_ms: f64,
    model_physical_after_prefill: usize,
    model_logical_after_prefill: usize,
    model_physical_end: usize,
    logits: Vec<Vec<Vec<f32>>>,
}

fn truncated_donor(donor: &SequenceState, keep: usize, kv_dim: usize) -> SequenceState {
    let mut s = donor.clone();
    s.tokens.truncate(keep);
    for l in s.layers.iter_mut() {
        l.cached_k.truncate(keep);
        l.cached_v.truncate(keep);
        l.flat_k.truncate(keep * kv_dim);
        l.flat_v.truncate(keep * kv_dim);
    }
    s
}

fn run_baseline(
    weights: &TransformerWeights,
    prompt: &[u32],
    n: usize,
    root_token: u32,
    forced: &[Vec<u32>],
    steps: usize,
) -> BaseRun {
    let bs = weights.config.block_size;
    let kv_dim = weights.config.num_kv_heads * weights.config.head_dim;
    let mut backend = NativeTransformerBackend::new(weights.clone());
    let mut model = PrefixCacheModel::new(bs);
    let mut reqs = Vec::new();
    let t_start = Instant::now();
    let mut prefill = Duration::ZERO;
    let mut prefill_tokens = 0usize;
    let ids: Vec<u64> = (0..n as u64).map(|i| 1000 + i).collect();
    for (i, id) in ids.iter().enumerate() {
        let req = model.admit(prompt);
        let hit = req.cached_tokens;
        reqs.push(req);
        if i > 0 && hit > 0 {
            let donor = backend.sequences.get(&ids[0]).expect("donor");
            let st = truncated_donor(donor, hit, kv_dim);
            backend.sequences.insert(*id, st);
        }
        let t0 = Instant::now();
        backend
            .prefill_sequence(*id, &prompt[hit..])
            .expect("baseline prefill");
        prefill += t0.elapsed();
        prefill_tokens += prompt.len() - hit;
    }
    // vLLM samples the first token in the prefill step: push the root token.
    for id in &ids {
        backend
            .sequences
            .get_mut(id)
            .unwrap()
            .tokens
            .push(root_token);
    }
    // Model bookkeeping: the generated token also lives in the request.
    for r in reqs.iter_mut() {
        model.append(r, root_token);
    }
    let after_prefill_phys = model.physical_blocks();
    let after_prefill_logical = model.logical_blocks(&reqs);
    let mut decode = Duration::ZERO;
    let mut ttft = 0.0;
    let mut logits: Vec<Vec<Vec<f32>>> = vec![Vec::new(); n];
    for s in 0..steps.min(forced.iter().map(Vec::len).min().unwrap_or(0)) {
        let t0 = Instant::now();
        let (_o, rows) = backend
            .forward_decode_batch_with_logits(&ids)
            .expect("baseline decode");
        decode += t0.elapsed();
        if s == 0 {
            ttft = t_start.elapsed().as_secs_f64() * 1e3;
        }
        for (i, row) in rows.into_iter().enumerate() {
            logits[i].push(row);
            let tok = forced[i][s];
            let seq = backend.sequences.get_mut(&ids[i]).unwrap();
            seq.tokens.pop();
            seq.tokens.push(tok);
            model.append(&mut reqs[i], tok);
        }
    }
    BaseRun {
        prefill_s: prefill.as_secs_f64(),
        prefill_tokens,
        decode_s: decode.as_secs_f64(),
        total_s: t_start.elapsed().as_secs_f64(),
        ttft_ms: ttft,
        model_physical_after_prefill: after_prefill_phys,
        model_logical_after_prefill: after_prefill_logical,
        model_physical_end: model.physical_blocks(),
        logits,
    }
}

// ---------------------------------------------------------------------------
// Control arm and parity
// ---------------------------------------------------------------------------

struct Control {
    prefill_s: f64,
    logits: Vec<Vec<Vec<f32>>>,
}

fn run_control(
    weights: &TransformerWeights,
    prompt: &[u32],
    root_token: u32,
    forced: &[Vec<u32>],
    branches: usize,
) -> Control {
    let mut b = NativeTransformerBackend::new(weights.clone());
    let t0 = Instant::now();
    b.prefill_sequence(1, prompt).expect("control prefill");
    let prefill_s = t0.elapsed().as_secs_f64();
    b.sequences.get_mut(&1).unwrap().tokens.push(root_token);
    let proto = b.sequences.get(&1).unwrap().clone();
    let mut out = Vec::new();
    for br in 0..branches.min(forced.len()) {
        let id = 10 + br as u64;
        b.sequences.insert(id, proto.clone());
        let mut rows = Vec::new();
        for tok in &forced[br] {
            let (_o, l) = b
                .forward_decode_batch_with_logits(&[id])
                .expect("control decode");
            rows.push(l.into_iter().next().unwrap());
            let seq = b.sequences.get_mut(&id).unwrap();
            seq.tokens.pop();
            seq.tokens.push(*tok);
        }
        out.push(rows);
    }
    Control {
        prefill_s,
        logits: out,
    }
}

/// (max abs difference, number of compared values, number of bitwise-differing values)
fn compare(a: &[Vec<Vec<f32>>], b: &[Vec<Vec<f32>>]) -> (f64, usize, usize) {
    let mut max_abs = 0f64;
    let mut count = 0usize;
    let mut diff = 0usize;
    for (ba, bb) in a.iter().zip(b) {
        for (ra, rb) in ba.iter().zip(bb) {
            if ra.len() != rb.len() {
                return (f64::INFINITY, count, usize::MAX);
            }
            for (x, y) in ra.iter().zip(rb) {
                count += 1;
                if x.to_bits() != y.to_bits() {
                    diff += 1;
                }
                let d = (f64::from(*x) - f64::from(*y)).abs();
                if d.is_nan() || d > max_abs {
                    max_abs = if d.is_nan() { f64::INFINITY } else { d };
                }
            }
        }
    }
    (max_abs, count, diff)
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        f64::NAN
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn hardware() -> serde_json::Value {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let processors = cpuinfo
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count();
    let mut parts: Vec<String> = Vec::new();
    for l in cpuinfo.lines() {
        if let Some((k, v)) = l.split_once(':') {
            if k.trim() == "CPU part" && !parts.contains(&v.trim().to_string()) {
                parts.push(v.trim().to_string());
            }
        }
    }
    let run = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    json!({
        "processors": processors,
        "cpu_parts": parts,
        "uname": run("uname", &["-a"]),
        "lscpu_model": run("sh", &["-c", "lscpu | grep 'Model name' | sort -u"]),
        "loadavg_at_start": std::fs::read_to_string("/proc/loadavg").ok(),
    })
}

#[tokio::main]
async fn main() {
    let ckpt = PathBuf::from(
        std::env::var("AIEN_BENCH_CHECKPOINT")
            .expect("AIEN_BENCH_CHECKPOINT (TinyLlama dir) is required; this bench never skips"),
    );
    let out_path = PathBuf::from(
        std::env::var("AIEN_BENCH_OUT").expect("AIEN_BENCH_OUT (JSON result path) is required"),
    );
    let ns = env_list("AIEN_BENCH_N", "1,2,4,8,16");
    let lens = env_list("AIEN_BENCH_LENS", "40,200,700");
    let steps = env_usize("AIEN_BENCH_STEPS", 8);
    let reps = env_usize("AIEN_BENCH_REPS", 1);
    let parity_branches = env_usize("AIEN_BENCH_PARITY_BRANCHES", 2);

    let state_dir = tempfile::tempdir().expect("state dir");
    std::env::set_var("AIEN_RUNTIME_STATE_DIR", state_dir.path());

    let (model_path, dir) = if ckpt.is_dir() {
        (ckpt.join("model.safetensors"), ckpt.clone())
    } else {
        (ckpt.clone(), ckpt.parent().unwrap().to_path_buf())
    };
    let config = ModelConfig::tinyllama_1_1b();
    let weights = TransformerWeights::load_from_safetensors(&model_path, &config)
        .unwrap_or_else(|e| panic!("checkpoint load failed (no fallback): {e:?}"));
    let tok = TinyLlamaTokenizer::from_file(&dir.join("tokenizer.json")).expect("tokenizer");
    let long_text = USER_PROMPT.repeat(8);
    let all_ids = tok
        .encode(&TinyLlamaTokenizer::format_prompt(
            Some(SYSTEM_PROMPT),
            &long_text,
        ))
        .expect("encode");
    let max_len = *lens.iter().max().unwrap();
    assert!(
        all_ids.len() >= max_len,
        "prompt source has only {} tokens",
        all_ids.len()
    );

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(["-C", env!("CARGO_MANIFEST_DIR")])
            .args(args)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let mut cells = Vec::new();
    for &len in &lens {
        let prompt: Vec<u32> = all_ids[..len].to_vec();
        for &n in &ns {
            for rep in 0..reps {
                eprintln!("PREFILL_BENCH cell len={len} n={n} rep={rep}");
                let a = run_aien(&weights, &prompt, n, steps).await;
                let nsteps = a.tokens.iter().map(Vec::len).min().unwrap_or(0);
                let forced: Vec<Vec<u32>> = a.tokens.iter().map(|t| t[..nsteps].to_vec()).collect();
                let a_logits: Vec<Vec<Vec<f32>>> =
                    a.logits.iter().map(|l| l[..nsteps].to_vec()).collect();
                let base = run_baseline(&weights, &prompt, n, a.root_token, &forced, nsteps);
                let ctl = run_control(&weights, &prompt, a.root_token, &forced, parity_branches);
                let pb = ctl.logits.len();
                let (a_ctl, a_ctl_n, a_ctl_d) = compare(&a_logits[..pb], &ctl.logits);
                let (b_ctl, _, b_ctl_d) = compare(&base.logits[..pb], &ctl.logits);
                let (a_b, a_b_n, a_b_d) = compare(&a_logits, &base.logits);
                let bb = a.block_bytes;
                let tokens_out = n * nsteps;
                let aien_ttft = mean(&a.ttft_ms);
                // MODELED naive (no sharing): N full prefills at the control's
                // measured one-shot rate; N x full-prompt blocks.
                let naive_prefill_s = ctl.prefill_s * n as f64;
                let blocks_per_prompt = len.div_ceil(config.block_size);
                cells.push(json!({
                    "n": n, "prefix_tokens": len, "rep": rep, "decode_steps": nsteps,
                    "aien_error": a.error, "aien_exit_state": a.diag,
                    "aien": {
                        "prefill_s": a.prefill_s, "prefill_tokens_computed": a.prefill_tokens,
                        "decode_s": a.decode_s, "total_s": a.total_s,
                        "ttft_ms_mean": aien_ttft,
                        "ttft_ms_max": a.ttft_ms.iter().cloned().fold(f64::NAN, f64::max),
                        "physical_kv_bytes_first_decode": a.physical_first_decode * bb,
                        "logical_kv_bytes_first_decode": a.logical_first_decode * bb,
                        "physical_kv_bytes_peak": a.peak_pages * bb,
                        "blocks_still_allocated_after_run": a.allocated_after,
                        "tokens_per_s_total": tokens_out as f64 / a.total_s,
                        "tokens_per_s_decode": tokens_out as f64 / a.decode_s,
                    },
                    "baseline_modeled_vllm_apc": {
                        "prefill_s": base.prefill_s, "prefill_tokens_computed": base.prefill_tokens,
                        "decode_s": base.decode_s, "total_s": base.total_s,
                        "ttft_ms": base.ttft_ms,
                        "physical_kv_bytes_after_prefill": base.model_physical_after_prefill * bb,
                        "logical_kv_bytes_after_prefill": base.model_logical_after_prefill * bb,
                        "physical_kv_bytes_end": base.model_physical_end * bb,
                        "kv_bytes_source": "bookkeeping model (examples/prefill_bench/vllm_model.rs), not measured",
                        "tokens_per_s_total": tokens_out as f64 / base.total_s,
                        "tokens_per_s_decode": tokens_out as f64 / base.decode_s,
                    },
                    "naive_no_sharing_modeled": {
                        "prefill_s": naive_prefill_s,
                        "physical_kv_bytes_after_prefill": n * blocks_per_prompt * bb,
                        "source": "N x control one-shot prefill time; N x ceil(len/block) blocks",
                    },
                    "control_prefill_s": ctl.prefill_s,
                    "parity": {
                        "compared_branches_vs_control": pb,
                        "aien_vs_control": {"max_abs": a_ctl, "values": a_ctl_n, "bit_differences": a_ctl_d},
                        "baseline_vs_control": {"max_abs": b_ctl, "bit_differences": b_ctl_d},
                        "aien_vs_baseline_all_branches": {"max_abs": a_b, "values": a_b_n, "bit_differences": a_b_d},
                    },
                }));
                // Flush partial results after every cell (long run, cut-off safe).
                let doc = json!({
                    "bench": "prefill_bench", "complete": false, "cells": cells,
                });
                std::fs::write(&out_path, serde_json::to_vec_pretty(&doc).unwrap()).ok();
            }
        }
    }
    let doc = json!({
        "bench": "prefill_bench",
        "complete": true,
        "commit": git(&["rev-parse", "HEAD"]),
        "dirty": git(&["status", "--porcelain"]).map(|s| !s.is_empty()),
        "model": {"path": model_path.display().to_string(), "id": config.model_id,
                  "kv_block_size": config.block_size},
        "hardware": hardware(),
        "knobs": {"n": ns, "prefix_tokens": lens, "decode_steps": steps, "reps": reps,
                  "parity_branches": parity_branches, "prefill_chunk": PREFILL_CHUNK},
        "baseline_label": "MODELED vLLM automatic prefix caching, not a vLLM run",
        "cells": cells,
    });
    std::fs::write(&out_path, serde_json::to_vec_pretty(&doc).unwrap()).expect("write result");
    println!("PREFILL_BENCH_DONE {}", out_path.display());
}

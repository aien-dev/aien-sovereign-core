//! AIEN Native Transformer engine benchmark driver with Mojo GB10 acceleration.
//! Executes real forward computation on DGX Spark GB10 in unified memory.

use crate::metrics::{calculate_joules_per_token, calculate_percentile};
use crate::oracle::{verify_token_sequence_match, ORACLE_BENCHMARK_128_TOKENS};
use crate::telemetry::HardwareMonitor;
use crate::types::{BenchmarkConfig, ConcurrencyRunResult};
use aien_inference_abi::tokenizer::TinyLlamaTokenizer;
use aien_inference_abi::transformer_backend::NativeTransformerBackend;
use aien_inference_abi::weights::TransformerWeights;
use aien_inference_abi::{
    AienInferenceBackend, DecodeOutput, ModelConfig, SamplingParams, ScheduledBatch,
    SequenceRequest,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

pub struct AienEngineHandle {
    pub backend: NativeTransformerBackend,
    pub tokenizer: TinyLlamaTokenizer,
}

impl AienEngineHandle {
    pub fn init(config: &BenchmarkConfig) -> Result<Self, String> {
        eprintln!("Initializing AIEN Native Transformer with Mojo GB10 Backend...");
        let model_config = ModelConfig {
            model_id: "TinyLlama/TinyLlama-1.1B-Chat-v1.0".to_string(),
            max_sequence_length: 2048,
            block_size: 16,
            num_layers: 22,
            num_heads: 32,
            head_dim: 64,
            num_kv_heads: 4,
            hidden_dim: 2048,
            intermediate_dim: 5632,
            vocab_size: 32000,
            rms_norm_eps: 1e-5,
            rope_theta: 10000.0,
        };

        let safetensors_path = format!("{}/model.safetensors", config.model_path);
        eprintln!("Loading safetensors weights from {}...", safetensors_path);
        let weights = TransformerWeights::load_from_safetensors(&safetensors_path, &model_config)
            .map_err(|e| format!("Failed to construct TransformerWeights: {}", e))?;

        let backend = NativeTransformerBackend::new_mojo(weights);
        eprintln!(
            "AIEN Backend initialized: {}",
            backend.tensor_backend.name()
        );

        let tokenizer_path = resolve_tokenizer_path(&config.tokenizer_path, &config.model_path)?;
        eprintln!("Loading tokenizer from {}...", tokenizer_path.display());
        let tokenizer = TinyLlamaTokenizer::from_file(&tokenizer_path)
            .map_err(|e| format!("Failed to load tokenizer from {}: {}", tokenizer_path.display(), e))?;

        Ok(Self { backend, tokenizer })
    }
}

fn resolve_tokenizer_path(configured: &str, model_path: &str) -> Result<PathBuf, String> {
    let candidates = [
        PathBuf::from(configured),
        PathBuf::from(model_path).join("tokenizer.json"),
        PathBuf::from("crates/aien-inference-abi/fixtures/tokenizer.json"),
    ];

    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }

    Err(format!(
        "Tokenizer file not found. Checked: {:?}",
        candidates
    ))
}

/// Runs a concurrency sweep for AIEN Native Transformer at concurrency level C.
pub async fn run_aien_concurrency_sweep(
    handle: &mut AienEngineHandle,
    config: &BenchmarkConfig,
    concurrency: usize,
) -> Result<ConcurrencyRunResult, String> {
    eprintln!(
        "Executing AIEN (MojoGb10Backend) benchmark at concurrency C={}...",
        concurrency
    );

    let prompt_tokens = handle
        .tokenizer
        .encode(&config.prompt)
        .map_err(|e| format!("Tokenizer encode error: {}", e))?;

    // Reset sequence tracking in backend
    handle.backend.sequences.clear();

    let monitor = HardwareMonitor::start(None);
    let wall_clock_start = Instant::now();

    // 1. Prefill phase: all concurrent requests submitted in ScheduledBatch
    let mut prefill_requests = Vec::with_capacity(concurrency);
    for req_idx in 0..concurrency {
        prefill_requests.push(SequenceRequest {
            request_id: req_idx as u64,
            prompt_tokens: prompt_tokens.clone(),
            sampling_params: SamplingParams {
                temperature: 0.0,
                top_p: 1.0,
                max_tokens: config.output_token_count,
                stop_token_ids: vec![0, 1, 2],
            },
            arrival_time_ns: 0,
            priority: 1,
        });
    }

    let prefill_batch = ScheduledBatch {
        prefill_requests,
        decode_requests: Vec::new(),
        block_tables: HashMap::new(),
        step_id: 0,
    };

    let prefill_start = Instant::now();
    let (prefill_outputs, _step_metrics) = handle
        .backend
        .execute_step(&prefill_batch)
        .await
        .map_err(|e| format!("Prefill step failed: {}", e))?;
    let prefill_elapsed_ms = prefill_start.elapsed().as_secs_f64() * 1000.0;

    let ttft_per_request = prefill_elapsed_ms / (concurrency as f64);
    let ttft_list = vec![ttft_per_request; concurrency];

    let mut request_output_tokens: Vec<Vec<u32>> =
        vec![Vec::with_capacity(config.output_token_count); concurrency];
    let mut request_itls: Vec<Vec<f64>> =
        vec![Vec::with_capacity(config.output_token_count); concurrency];
    let mut active_req_ids: Vec<u64> = (0..concurrency as u64).collect();

    for out in prefill_outputs {
        if let DecodeOutput::Token {
            request_id,
            token_id,
            ..
        } = out
        {
            if let Some(toks) = request_output_tokens.get_mut(request_id as usize) {
                toks.push(token_id);
            }
        }
    }

    // 2. Decode phase: up to config.output_token_count - 1 steps
    let max_decode_steps = config.output_token_count.saturating_sub(1);
    for step_num in 1..=max_decode_steps {
        if active_req_ids.is_empty() {
            break;
        }

        let decode_batch = ScheduledBatch {
            prefill_requests: Vec::new(),
            decode_requests: active_req_ids.clone(),
            block_tables: HashMap::new(),
            step_id: step_num as u64,
        };

        let step_start = Instant::now();
        let (decode_outputs, _) = handle
            .backend
            .execute_step(&decode_batch)
            .await
            .map_err(|e| format!("Decode step {} failed: {}", step_num, e))?;
        let step_elapsed_ms = step_start.elapsed().as_secs_f64() * 1000.0;
        let per_token_itl = step_elapsed_ms / (active_req_ids.len() as f64);

        let mut finished_in_step = Vec::new();
        for out in decode_outputs {
            match out {
                DecodeOutput::Token {
                    request_id,
                    token_id,
                    ..
                } => {
                    let req_idx = request_id as usize;
                    if let Some(toks) = request_output_tokens.get_mut(req_idx) {
                        toks.push(token_id);
                    }
                    if let Some(itls) = request_itls.get_mut(req_idx) {
                        itls.push(per_token_itl);
                    }
                }
                DecodeOutput::Finished { request_id, .. } => {
                    finished_in_step.push(request_id);
                }
            }
        }

        active_req_ids.retain(|id| !finished_in_step.contains(id));
    }

    let wall_clock_elapsed = wall_clock_start.elapsed().as_secs_f64();
    let (avg_power_watts, peak_power_watts, peak_rss_gb) = monitor.stop().await;

    let mut total_output_tokens = 0usize;
    let mut all_itls = Vec::new();
    for itls in &request_itls {
        all_itls.extend_from_slice(itls);
    }
    for toks in &request_output_tokens {
        total_output_tokens += toks.len();
    }

    let throughput = (total_output_tokens as f64) / wall_clock_elapsed;
    let ttft_p50 = calculate_percentile(&ttft_list, 50.0);
    let ttft_p95 = calculate_percentile(&ttft_list, 95.0);
    let ttft_p99 = calculate_percentile(&ttft_list, 99.0);
    let itl_p50 = calculate_percentile(&all_itls, 50.0);
    let itl_p95 = calculate_percentile(&all_itls, 95.0);
    let itl_p99 = calculate_percentile(&all_itls, 99.0);
    let energy_j_per_tok =
        calculate_joules_per_token(avg_power_watts, wall_clock_elapsed, total_output_tokens);

    let parity_match_rate_pct = if let Some(first_req_tokens) = request_output_tokens.first() {
        verify_token_sequence_match(first_req_tokens, &ORACLE_BENCHMARK_128_TOKENS)
    } else {
        0.0
    };

    let sample_text = if let Some(first_req_tokens) = request_output_tokens.first() {
        handle.tokenizer.decode(first_req_tokens).unwrap_or_default()
    } else {
        String::new()
    };

    Ok(ConcurrencyRunResult {
        engine: "AIEN (MojoGb10Backend)".to_string(),
        concurrency,
        total_requests: concurrency,
        total_output_tokens,
        elapsed_wall_clock_secs: wall_clock_elapsed,
        throughput_tokens_sec: throughput,
        ttft_p50_ms: ttft_p50,
        ttft_p95_ms: ttft_p95,
        ttft_p99_ms: ttft_p99,
        itl_p50_ms: itl_p50,
        itl_p95_ms: itl_p95,
        itl_p99_ms: itl_p99,
        peak_rss_gb,
        avg_gpu_power_watts: avg_power_watts,
        peak_gpu_power_watts: peak_power_watts,
        energy_joules_per_token: energy_j_per_tok,
        parity_match_rate_pct,
        sample_text,
    })
}

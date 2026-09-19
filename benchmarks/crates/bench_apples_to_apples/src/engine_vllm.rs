//! vLLM engine benchmark driver.
//! Spawns `vllm serve` container on DGX Spark GB10 and drives concurrent load.

use crate::metrics::{calculate_joules_per_token, calculate_percentile};
use crate::oracle::{verify_token_sequence_match, ORACLE_BENCHMARK_128_TOKENS};
use crate::telemetry::HardwareMonitor;
use crate::types::{BenchmarkConfig, ConcurrencyRunResult, RequestRecord};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use std::process::Command;
use std::time::{Duration, Instant};

pub struct VllmContainerHandle {
    pub container_name: String,
    pub port: u16,
    pub host_pid: Option<u32>,
}

impl VllmContainerHandle {
    pub async fn start(config: &BenchmarkConfig) -> Result<Self, String> {
        let container_name = format!("aien-bench-vllm-{}", config.vllm_serve_port);
        // Ensure any previous container is killed
        let _ = Command::new("docker").args(["rm", "-f", &container_name]).output();

        eprintln!(
            "Starting vLLM container '{}' on port {}...",
            container_name, config.vllm_serve_port
        );

        let port_mapping = format!("{}:8000", config.vllm_serve_port);
        let status = Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                &container_name,
                "--rm",
                "--gpus",
                "all",
                "-p",
                &port_mapping,
                "-v",
                "/home/drakestapleton/.cache/huggingface:/root/.cache/huggingface",
                "atlas-vllm-s60:runtime-20260905",
                "--model",
                "/root/.cache/huggingface/hub/models--TinyLlama--TinyLlama-1.1B-Chat-v1.0/snapshots/fe8a4ea1ffedaf415f4da2f062534de366a451e6",
                "--dtype",
                "bfloat16",
                "--port",
                "8000",
                "--gpu-memory-utilization",
                "0.12",
                "--enforce-eager",
            ])
            .status()
            .map_err(|e| format!("Failed to spawn docker run: {}", e))?;

        if !status.success() {
            return Err("Docker run failed to start vLLM container.".to_string());
        }

        // Get container PID
        let inspect_output = Command::new("docker")
            .args(["inspect", "-f", "{{.State.Pid}}", &container_name])
            .output()
            .ok();

        let host_pid = inspect_output.and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            s.trim().parse::<u32>().ok()
        });

        let handle = Self {
            container_name,
            port: config.vllm_serve_port,
            host_pid,
        };

        // Poll readiness
        let client = Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .map_err(|e| e.to_string())?;

        let url = format!("http://127.0.0.1:{}/v1/models", config.vllm_serve_port);
        let start = Instant::now();
        let timeout = Duration::from_secs(90);

        eprintln!("Waiting for vLLM container readiness on {}...", url);
        while start.elapsed() < timeout {
            if let Ok(res) = client.get(&url).send().await {
                if res.status().is_success() {
                    eprintln!("vLLM container is ready in {:.1}s.", start.elapsed().as_secs_f64());
                    return Ok(handle);
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        Err("vLLM container failed to become ready within 90 seconds.".to_string())
    }
}

impl Drop for VllmContainerHandle {
    fn drop(&mut self) {
        eprintln!("Stopping vLLM container '{}'...", self.container_name);
        let _ = Command::new("docker")
            .args(["stop", "-t", "5", &self.container_name])
            .output();
    }
}

/// Executes a single streaming chat completion request against vLLM.
async fn execute_single_vllm_request(
    client: Client,
    port: u16,
    request_id: usize,
    prompt: String,
    max_tokens: usize,
) -> Result<RequestRecord, String> {
    let url = format!("http://127.0.0.1:{}/v1/chat/completions", port);
    let payload = serde_json::json!({
        "model": "/root/.cache/huggingface/hub/models--TinyLlama--TinyLlama-1.1B-Chat-v1.0/snapshots/fe8a4ea1ffedaf415f4da2f062534de366a451e6",
        "messages": [
            {
                "role": "system",
                "content": "You are a helpful and truthful AI assistant. Explain technical concepts with clarity and precision."
            },
            {
                "role": "user",
                "content": prompt
            }
        ],
        "max_tokens": max_tokens,
        "temperature": 0.0,
        "stream": true
    });

    let start = Instant::now();
    let res = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("HTTP request error: {}", e))?;

    if !res.status().is_success() {
        return Err(format!("vLLM request returned status: {}", res.status()));
    }

    let mut stream = res.bytes_stream();
    let mut ttft_ms = 0.0;
    let mut itl_ms = Vec::new();
    let mut last_chunk_time = start;
    let mut generated_text = String::new();
    let mut token_count = 0usize;

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| format!("Stream read error: {}", e))?;
        let text = String::from_utf8_lossy(&chunk);

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            if line == "data: [DONE]" {
                break;
            }
            if let Some(json_str) = line.strip_prefix("data: ") {
                if let Ok(val) = serde_json::from_str::<Value>(json_str) {
                    if let Some(choices) = val.get("choices").and_then(|c| c.as_array()) {
                        if let Some(first) = choices.first() {
                            if let Some(content) = first
                                .get("delta")
                                .and_then(|d| d.get("content"))
                                .and_then(|s| s.as_str())
                            {
                                let now = Instant::now();
                                if token_count == 0 {
                                    ttft_ms = start.elapsed().as_secs_f64() * 1000.0;
                                } else {
                                    itl_ms.push(now.duration_since(last_chunk_time).as_secs_f64() * 1000.0);
                                }
                                last_chunk_time = now;
                                token_count += 1;
                                generated_text.push_str(content);
                            }
                        }
                    }
                }
            }
        }
    }

    let total_duration_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(RequestRecord {
        request_id,
        ttft_ms,
        itl_ms,
        total_duration_ms,
        output_tokens_count: token_count,
        output_tokens: Vec::new(),
        text_preview: generated_text,
    })
}

/// Runs a concurrency sweep for vLLM at concurrency level C.
pub async fn run_vllm_concurrency_sweep(
    handle: &VllmContainerHandle,
    config: &BenchmarkConfig,
    concurrency: usize,
) -> Result<ConcurrencyRunResult, String> {
    eprintln!(
        "Executing vLLM benchmark at concurrency C={}...",
        concurrency
    );

    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    let monitor = HardwareMonitor::start(handle.host_pid);
    let wall_clock_start = Instant::now();

    let mut handles = Vec::with_capacity(concurrency);
    for req_idx in 0..concurrency {
        let client_clone = client.clone();
        let port = handle.port;
        let prompt = config.prompt.clone();
        let max_tokens = config.output_token_count;

        handles.push(tokio::spawn(async move {
            execute_single_vllm_request(client_clone, port, req_idx, prompt, max_tokens).await
        }));
    }

    let mut records = Vec::with_capacity(concurrency);
    for h in handles {
        match h.await {
            Ok(Ok(rec)) => records.push(rec),
            Ok(Err(e)) => eprintln!("Request failed: {}", e),
            Err(e) => eprintln!("Join error: {}", e),
        }
    }

    let wall_clock_elapsed = wall_clock_start.elapsed().as_secs_f64();
    let (avg_power_watts, peak_power_watts, peak_rss_gb) = monitor.stop().await;

    if records.is_empty() {
        return Err("All requests failed during vLLM concurrency sweep.".to_string());
    }

    let mut all_ttft = Vec::new();
    let mut all_itl = Vec::new();
    let mut total_output_tokens = 0usize;
    let mut sample_text = String::new();

    for r in &records {
        all_ttft.push(r.ttft_ms);
        all_itl.extend_from_slice(&r.itl_ms);
        total_output_tokens += r.output_tokens_count;
        if sample_text.is_empty() && !r.text_preview.is_empty() {
            sample_text = r.text_preview.clone();
        }
    }

    let throughput = (total_output_tokens as f64) / wall_clock_elapsed;
    let ttft_p50 = calculate_percentile(&all_ttft, 50.0);
    let ttft_p95 = calculate_percentile(&all_ttft, 95.0);
    let ttft_p99 = calculate_percentile(&all_ttft, 99.0);
    let itl_p50 = calculate_percentile(&all_itl, 50.0);
    let itl_p95 = calculate_percentile(&all_itl, 95.0);
    let itl_p99 = calculate_percentile(&all_itl, 99.0);
    let energy_j_per_tok = calculate_joules_per_token(avg_power_watts, wall_clock_elapsed, total_output_tokens);

    let parity_match_rate_pct = if let Ok(tok) =
        aien_inference_abi::tokenizer::TinyLlamaTokenizer::from_file(&config.tokenizer_path)
    {
        if let Ok(toks) = tok.encode(&sample_text) {
            verify_token_sequence_match(&toks, &ORACLE_BENCHMARK_128_TOKENS)
        } else {
            0.0
        }
    } else {
        0.0
    };

    Ok(ConcurrencyRunResult {
        engine: "vLLM (vllm serve)".to_string(),
        concurrency,
        total_requests: records.len(),
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

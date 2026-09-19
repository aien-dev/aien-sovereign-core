//! Modular MAX engine benchmark driver.
//! Spawns `max serve` on DGX Spark GB10 with `--no-device-graph-capture` and drives concurrent load.

use crate::metrics::{calculate_joules_per_token, calculate_percentile};
use crate::oracle::{verify_token_sequence_match, ORACLE_BENCHMARK_128_TOKENS};
use crate::telemetry::HardwareMonitor;
use crate::types::{BenchmarkConfig, ConcurrencyRunResult, RequestRecord};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct MaxServerHandle {
    child: Child,
    pub port: u16,
}

impl MaxServerHandle {
    pub async fn start(config: &BenchmarkConfig) -> Result<Self, String> {
        eprintln!(
            "Starting Modular MAX server on port {} with --no-device-graph-capture...",
            config.max_serve_port
        );

        let child = Command::new("max")
            .args([
                "serve",
                "--model-path",
                &config.model_path,
                "--port",
                &config.max_serve_port.to_string(),
                "--device-memory-utilization",
                "0.4",
                "--no-device-graph-capture",
            ])
            .env("MODULAR_CACHE_DIR", "/tmp/modular_cache")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Failed to spawn max serve: {}", e))?;

        let handle = Self {
            child,
            port: config.max_serve_port,
        };

        // Poll endpoint readiness
        let client = Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .map_err(|e| e.to_string())?;

        let url = format!("http://127.0.0.1:{}/v1/models", config.max_serve_port);
        let start = Instant::now();
        let timeout = Duration::from_secs(180);

        eprintln!("Waiting for Modular MAX server readiness on {}...", url);
        while start.elapsed() < timeout {
            if let Ok(res) = client.get(&url).send().await {
                if res.status().is_success() {
                    eprintln!("Modular MAX server is ready in {:.1}s.", start.elapsed().as_secs_f64());
                    return Ok(handle);
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        Err("Modular MAX server failed to become ready within 180 seconds.".to_string())
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for MaxServerHandle {
    fn drop(&mut self) {
        eprintln!("Terminating Modular MAX server PID {}...", self.child.id());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Executes a single streaming chat completion request against MAX.
async fn execute_single_max_request(
    client: Client,
    port: u16,
    request_id: usize,
    prompt: String,
    max_tokens: usize,
) -> Result<RequestRecord, String> {
    let url = format!("http://127.0.0.1:{}/v1/chat/completions", port);
    let payload = serde_json::json!({
        "model": "TinyLlama/TinyLlama-1.1B-Chat-v1.0",
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
        return Err(format!("MAX request returned status: {}", res.status()));
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

/// Runs a concurrency sweep for Modular MAX at concurrency level C.
pub async fn run_max_concurrency_sweep(
    server: &MaxServerHandle,
    config: &BenchmarkConfig,
    concurrency: usize,
) -> Result<ConcurrencyRunResult, String> {
    eprintln!(
        "Executing Modular MAX benchmark at concurrency C={}...",
        concurrency
    );

    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    let monitor = HardwareMonitor::start(Some(server.pid()));
    let wall_clock_start = Instant::now();

    let mut handles = Vec::with_capacity(concurrency);
    for req_idx in 0..concurrency {
        let client_clone = client.clone();
        let port = server.port;
        let prompt = config.prompt.clone();
        let max_tokens = config.output_token_count;

        handles.push(tokio::spawn(async move {
            execute_single_max_request(client_clone, port, req_idx, prompt, max_tokens).await
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
        return Err("All requests failed during MAX concurrency sweep.".to_string());
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
        engine: "Modular MAX (max serve)".to_string(),
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

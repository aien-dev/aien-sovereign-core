//! Standalone neutral apples-to-apples benchmark driver comparing
//! AIEN (MojoGb10Backend), Modular MAX (max serve), and vLLM (vllm serve)
//! under identical conditions on DGX Spark GB10.

mod engine_aien;
mod engine_max;
mod engine_vllm;
mod metrics;
mod oracle;
mod telemetry;
mod types;

use clap::Parser;
use engine_aien::{run_aien_concurrency_sweep, AienEngineHandle};
use engine_max::{run_max_concurrency_sweep, MaxServerHandle};
use engine_vllm::{run_vllm_concurrency_sweep, VllmContainerHandle};
use oracle::ORACLE_BENCHMARK_128_TOKENS;
use std::fs;
use std::path::Path;
use telemetry::enforce_thermal_cooldown;
use types::{BenchmarkConfig, BenchmarkReport, ConcurrencyRunResult};

#[derive(Parser, Debug)]
#[command(
    name = "bench_apples_to_apples",
    about = "Neutral Apples-to-Apples LLM Inference Benchmark Suite on DGX Spark GB10"
)]
struct Args {
    /// Comma-separated list of engines to benchmark: aien,max,vllm
    #[arg(short, long, default_value = "aien,max,vllm")]
    engines: String,

    /// Comma-separated list of concurrency levels to test
    #[arg(short, long, default_value = "1,2,4,8,16,32,64")]
    concurrency: String,

    /// Target output tokens per request
    #[arg(long, default_value_t = 128)]
    output_tokens: usize,

    /// Directory where JSON benchmark artifacts are written
    #[arg(long, default_value = "benchmarks/data")]
    output_dir: String,

    /// Modular MAX serve port
    #[arg(long, default_value_t = 18090)]
    max_port: u16,

    /// vLLM container port
    #[arg(long, default_value_t = 18094)]
    vllm_port: u16,

    /// Idle GPU power threshold for thermal cooldown envelope in Watts
    #[arg(long, default_value_t = 12.0)]
    target_power_watts: f64,

    /// Max GPU temperature for thermal cooldown envelope in Celsius
    #[arg(long, default_value_t = 42.0)]
    max_temp_c: f64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut config = BenchmarkConfig::default();
    config.output_token_count = args.output_tokens;
    config.max_serve_port = args.max_port;
    config.vllm_serve_port = args.vllm_port;
    config.cool_down_target_power_watts = args.target_power_watts;
    config.cool_down_max_temp_c = args.max_temp_c;

    let concurrency_levels: Vec<usize> = args
        .concurrency
        .split(',')
        .filter_map(|s| s.trim().parse::<usize>().ok())
        .collect();
    config.concurrency_levels = concurrency_levels.clone();

    let target_engines: Vec<String> = args
        .engines
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    println!("================================================================================");
    println!("   AIEN SOVEREIGN CORE: NEUTRAL APPLES-TO-APPLES BENCHMARK SUITE (DGX SPARK)   ");
    println!("================================================================================");
    println!("Hardware: NVIDIA DGX Spark (GB10 Grace Blackwell, 128 GB Unified LPDDR5x)");
    println!("Model: TinyLlama/TinyLlama-1.1B-Chat-v1.0 (BF16)");
    println!("Workload: 128 input tokens -> 128 output tokens (Greedy, Temp=0.0)");
    println!("Engines: {:?}", target_engines);
    println!("Concurrency levels: {:?}", concurrency_levels);
    println!("Thermal cooldown envelope: <= {:.1}W, <= {:.1}C", config.cool_down_target_power_watts, config.cool_down_max_temp_c);
    println!("================================================================================");

    let mut all_results: Vec<ConcurrencyRunResult> = Vec::new();

    for engine in &target_engines {
        println!("\n>>> Preparing engine: {} <<<", engine.to_uppercase());

        // Enforce thermal cooldown and memory cache flush before each engine run
        enforce_thermal_cooldown(
            config.cool_down_target_power_watts,
            config.cool_down_max_temp_c,
        )
        .await;

        match engine.as_str() {
            "aien" => {
                let mut handle = match AienEngineHandle::init(&config) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("Failed to initialize AIEN engine: {}", e);
                        continue;
                    }
                };

                for &c in &concurrency_levels {
                    match run_aien_concurrency_sweep(&mut handle, &config, c).await {
                        Ok(res) => {
                            print_sweep_result_line(&res);
                            all_results.push(res);
                        }
                        Err(e) => eprintln!("AIEN C={} sweep failed: {}", c, e),
                    }
                }
            }

            "max" => {
                let server = match MaxServerHandle::start(&config).await {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Failed to start Modular MAX server: {}", e);
                        continue;
                    }
                };

                for &c in &concurrency_levels {
                    match run_max_concurrency_sweep(&server, &config, c).await {
                        Ok(res) => {
                            print_sweep_result_line(&res);
                            all_results.push(res);
                        }
                        Err(e) => eprintln!("MAX C={} sweep failed: {}", c, e),
                    }
                }
                // Server process killed when `server` drops at end of block
            }

            "vllm" => {
                let handle = match VllmContainerHandle::start(&config).await {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("Failed to start vLLM container: {}", e);
                        continue;
                    }
                };

                for &c in &concurrency_levels {
                    match run_vllm_concurrency_sweep(&handle, &config, c).await {
                        Ok(res) => {
                            print_sweep_result_line(&res);
                            all_results.push(res);
                        }
                        Err(e) => eprintln!("vLLM C={} sweep failed: {}", c, e),
                    }
                }
                // Container stopped when `handle` drops at end of block
            }

            other => {
                eprintln!("Unknown engine: '{}'. Skipping.", other);
            }
        }
    }

    println!("\n================================================================================");
    println!("                           FINAL BENCHMARK COMPARISON                           ");
    println!("================================================================================");
    print_summary_table(&all_results);
    println!("================================================================================");

    // Save JSON report
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let report = BenchmarkReport {
        benchmark_timestamp: chrono::Utc::now().to_rfc3339(),
        hardware: "NVIDIA DGX Spark GB10 Grace Blackwell 128GB Unified Memory".to_string(),
        model_id: "TinyLlama/TinyLlama-1.1B-Chat-v1.0".to_string(),
        precision: "BF16".to_string(),
        prompt_tokens: config.prompt_token_count,
        target_output_tokens: config.output_token_count,
        results: all_results,
        parity_oracle_sequence: ORACLE_BENCHMARK_128_TOKENS.to_vec(),
    };

    let out_dir = Path::new(&args.output_dir);
    fs::create_dir_all(out_dir)?;

    let report_filename = format!("benchmark_results_{}.json", timestamp);
    let report_path = out_dir.join(&report_filename);
    let latest_path = out_dir.join("latest.json");

    let json_bytes = serde_json::to_string_pretty(&report)?;
    fs::write(&report_path, &json_bytes)?;
    fs::write(&latest_path, &json_bytes)?;

    println!("Benchmark report saved to: {}", report_path.display());
    println!("Latest benchmark updated:  {}", latest_path.display());

    Ok(())
}

fn print_sweep_result_line(res: &ConcurrencyRunResult) {
    println!(
        "[{}] C={:<2} | Throughput: {:>7.2} tok/s | TTFT p50: {:>6.2} ms | ITL p50: {:>5.2} ms | Peak RSS: {:>5.2} GB | Power: {:>5.1} W | J/tok: {:>5.3} | Parity: {:>5.1}%",
        res.engine,
        res.concurrency,
        res.throughput_tokens_sec,
        res.ttft_p50_ms,
        res.itl_p50_ms,
        res.peak_rss_gb,
        res.avg_gpu_power_watts,
        res.energy_joules_per_token,
        res.parity_match_rate_pct,
    );
}

fn print_summary_table(results: &[ConcurrencyRunResult]) {
    println!(
        "| {:<24} | {:>3} | {:>10} | {:>10} | {:>10} | {:>9} | {:>9} | {:>8} | {:>9} | {:>8} |",
        "Engine", "C", "Throughput", "TTFT p50", "TTFT p95", "ITL p50", "ITL p95", "Peak RSS", "Avg Power", "Parity"
    );
    println!(
        "|--------------------------|-----|------------|------------|------------|-----------|-----------|----------|-----------|----------|"
    );

    for r in results {
        println!(
            "| {:<24} | {:>3} | {:>8.2} t/s | {:>7.2} ms | {:>7.2} ms | {:>6.2} ms | {:>6.2} ms | {:>6.2} GB | {:>7.1} W | {:>6.1} % |",
            r.engine,
            r.concurrency,
            r.throughput_tokens_sec,
            r.ttft_p50_ms,
            r.ttft_p95_ms,
            r.itl_p50_ms,
            r.itl_p95_ms,
            r.peak_rss_gb,
            r.avg_gpu_power_watts,
            r.parity_match_rate_pct,
        );
    }
}

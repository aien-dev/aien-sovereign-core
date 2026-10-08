//! NEXT-PHASE-1 v6 CPU reference driver (ACCEPTANCE-v6 R1 and the pre-freeze
//! ground checks). Runs one compose prompt through the CPU reference backend
//! (`ReferenceCpuBackend`, never the GPU) on the daemon's path: config.json ->
//! ModelConfig, strict safetensors load, the daemon's scheduler config and
//! shared KV sizing, `ChatTokenizer::from_model_dir`, one warm-up turn on
//! `WARM_UP_TEXT`, then `format_chat` + `COMPOSE_ASSISTANT_PREFIX`, greedy,
//! the model's stop set. Prints one JSON object.
//!
//! usage: np1_reference <model_dir> <goal> <workspace> <max_tokens> <new|task> [retry reason]
//!   new  = `proposal_prompt(goal, workspace)` (the v5 template, no edit block)
//!   task = `task_prompt(goal, workspace)` on the workspace as it is now
//!   retry reason: the prompt of attempt k > 1, `retry_prompt(<prompt>, reason)`
use aien_inference_abi::{ChatTokenizer, SamplingParams};
use aien_runtime::shared_kv::{build_shared_kv_runtime, SharedKvSizing};
use aien_scheduler::{ChannelCompletionSink, CompletionEvent};
use sha2::Digest;
use std::sync::Arc;

async fn turn(
    spine: &mut aien_runtime::AienRuntimeSpine,
    backend: &mut aien_inference_abi::NativeTransformerBackend,
    ids: &[u32],
    max_tokens: usize,
    stop: &[u32],
) -> Result<(Vec<u32>, String, f64), String> {
    let (sink, mut ev) = ChannelCompletionSink::channel();
    let sid = spine.register_completion_sink(Arc::new(sink));
    let sampling = SamplingParams {
        temperature: 0.0,
        top_p: 0.95,
        max_tokens,
        stop_token_ids: stop.to_vec(),
    };
    spine.submit_work(Arc::from(ids), sampling, 2, Some(sid))?;
    let t0 = std::time::Instant::now();
    let mut out = Vec::new();
    loop {
        if spine.scheduler.running_count() > 0 || spine.scheduler.waiting_count() > 0 {
            spine.step(backend).await.map_err(|e| format!("{e:?}"))?;
        }
        while let Ok(e) = ev.try_recv() {
            match e {
                CompletionEvent::Token { token, .. } => out.push(token),
                CompletionEvent::Finished { finish_reason, .. } => {
                    let label = aien_runtime::server::finish_reason_label(&finish_reason);
                    return Ok((out, label.into(), t0.elapsed().as_secs_f64() * 1e3));
                }
                CompletionEvent::Error { message, .. } => return Err(message),
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    if !(6..=7).contains(&a.len()) || !matches!(a[5].as_str(), "new" | "task") {
        eprintln!(
            "usage: np1_reference <model_dir> <goal> <workspace> <max_tokens> <new|task> [retry reason]"
        );
        std::process::exit(2);
    }
    let dir = std::path::PathBuf::from(&a[1]);
    let (goal, ws, mode) = (&a[2], std::path::PathBuf::from(&a[3]), a[5].as_str());
    let max_tokens: usize = a[4].parse().expect("max_tokens");
    let config = aien_inference_abi::load_model_config(&dir).expect("config.json");
    let weights = aien_inference_abi::TransformerWeights::load_from_safetensors(&dir, &config)
        .expect("weights");
    let tok = ChatTokenizer::from_model_dir(&dir, None).expect("tokenizer");
    let tb: Arc<dyn aien_inference_abi::TensorBackend> =
        Arc::new(aien_inference_abi::ReferenceCpuBackend::new());
    // The daemon's values (aien-cli run_daemon_server).
    let cfg = aien_scheduler::SchedulerConfig {
        max_batch_size: 256,
        max_batch_tokens: 16384,
        max_prefill_tokens: 8192,
        prefill_chunk_size: 128,
        chunk_prefill: true,
        watermark_blocks: 64,
    };
    let sizing = SharedKvSizing {
        arena_capacity: 4096,
        total_blocks: 8192,
    };
    let (mut spine, mut backend) =
        build_shared_kv_runtime(weights, tb.clone(), cfg, sizing).expect("shared KV");
    let stop = tok.stop_token_ids().to_vec();
    let wm = vec![aien_runtime::ChatTurn {
        role: "user".into(),
        content: aien_runtime::WARM_UP_TEXT.into(),
    }];
    let wids = tok
        .encode(&aien_runtime::format_chat(tok.template(), &wm).expect("chat"))
        .expect("encode");
    let (w, _, _) = turn(&mut spine, &mut backend, &wids, 1, &stop)
        .await
        .expect("warm-up");
    let base = match mode {
        "new" => aien_runtime::spine::proposal_prompt(goal, &ws.display().to_string()),
        _ => aien_runtime::spine::task_prompt(goal, &ws),
    };
    let retry = a.get(6).cloned();
    let prompt = match &retry {
        Some(why) => aien_runtime::spine::retry_prompt(&base, why),
        None => base,
    };
    let msgs = vec![aien_runtime::ChatTurn {
        role: "user".into(),
        content: prompt.clone(),
    }];
    let text = format!(
        "{}{}",
        aien_runtime::format_chat(tok.template(), &msgs).expect("chat"),
        aien_runtime::spine::COMPOSE_ASSISTANT_PREFIX
    );
    let ids = tok.encode(&text).expect("encode");
    let (out, finish, ms) = turn(&mut spine, &mut backend, &ids, max_tokens, &stop)
        .await
        .expect("turn");
    let reply = format!(
        "{}{}",
        aien_runtime::spine::COMPOSE_ASSISTANT_PREFIX,
        tok.decode_opts(&out, true).expect("decode")
    );
    let hex = |b: &[u8]| -> String {
        sha2::Sha256::digest(b)
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect()
    };
    println!(
        "{}",
        serde_json::json!({
            "backend": aien_inference_abi::TensorBackend::name(tb.as_ref()),
            "model_dir": dir, "model_id": config.model_id, "template": tok.template().name(),
            "stop_ids": stop, "warm_up_ids": w, "mode": mode, "retry_reason": retry, "goal": goal, "workspace": ws,
            "prompt_sha256": hex(prompt.as_bytes()),
            "prompt_tokens": ids.len(), "prompt_ids_sha256": aien_runtime::spine::token_ids_sha256(&ids),
            "prompt_ids": ids, "max_tokens": max_tokens, "output_ids": out, "n_out": out.len(),
            "finish": finish, "ms": ms, "reply": reply, "reply_sha256": hex(reply.as_bytes()),
        })
    );
}

//! NEXT-PHASE-1 v6 CPU reference driver (ACCEPTANCE-v6 R1 and the pre-freeze
//! ground checks). Runs one compose prompt through the CPU reference backend
//! (`ReferenceCpuBackend`, never the GPU) on the daemon's path: config.json ->
//! ModelConfig, strict safetensors load, the daemon's scheduler config and
//! shared KV sizing, `ChatTokenizer::from_model_dir`, one warm-up turn on
//! `WARM_UP_TEXT`, then `format_chat` + `COMPOSE_ASSISTANT_PREFIX`, greedy,
//! the model's stop set. Prints one JSON object.
//!
//! usage: np1_reference <model_dir> <goal> <workspace> <max_tokens> <new|task> [retry reason]
//!   new  = the product's NEW-destination prompt (`spine::task_decision`, `TargetClass::New`):
//!          `proposal_prompt(goal)` plus `new_document_block` for the destination the
//!          goal names, from the product's own public pieces, so it still holds after the run wrote
//!          that destination (the v5 template alone when the goal names none)
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
    let base = reference_prompt(mode, goal, &ws);
    let retry = a.get(6).cloned();
    let prompt = match &retry {
        Some(why) => aien_runtime::spine::retry_prompt(&base, why),
        None => base,
    };
    let allen = std::env::var("NP1_REFERENCE_ALLEN").unwrap_or_else(|_| "none".into());
    let prompt = allen_prompt(&prompt, &allen);
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
            "backend": aien_inference_abi::TensorBackend::name(tb.as_ref()), "allen": allen,
            "model_dir": dir, "model_id": config.model_id, "template": tok.template().name(),
            "stop_ids": stop, "warm_up_ids": w, "mode": mode, "retry_reason": retry, "goal": goal, "workspace": ws,
            "prompt_sha256": hex(prompt.as_bytes()),
            "prompt_tokens": ids.len(), "prompt_ids_sha256": aien_runtime::spine::token_ids_sha256(&ids),
            "prompt_ids": ids, "max_tokens": max_tokens, "output_ids": out, "n_out": out.len(),
            "finish": finish, "ms": ms, "reply": reply, "reply_sha256": hex(reply.as_bytes()),
        })
    );
}

/// The prompt of one reference run. `new` repeats the `TargetClass::New` branch of
/// `spine::task_decision` (the template plus `new_document_block` for the named
/// destination) with the product's own functions; it does not call `task_decision`
/// because the run being replayed may already have written that destination, which
/// would turn the live decision into an edit. `task` is the product's `task_prompt`
/// on the workspace as it is now. The tests below hold both byte-equal to the product.
/// Limit: a word without an extension is a destination only once it exists as a file
/// (`destination::path_like`), so a goal like "save it into notes/tea" can gain the block
/// after the run wrote `notes/tea`; name replayed destinations with an extension (v5 R1:
/// `notes/swim-tip.txt`).
fn reference_prompt(mode: &str, goal: &str, ws: &std::path::Path) -> String {
    use aien_runtime::spine::{
        classify_destination, new_document_block, proposal_prompt, task_prompt,
    };
    match mode {
        "new" => {
            let mut prompt = proposal_prompt(goal);
            if let Some(dest) = classify_destination(goal, ws).1 {
                prompt.push_str(&new_document_block(&dest));
            }
            prompt
        }
        _ => task_prompt(goal, ws),
    }
}

#[cfg(test)]
mod tests {
    use super::reference_prompt;
    use aien_runtime::spine::{task_decision, ProposalKind};
    use std::path::{Path, PathBuf};

    fn workspace() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let ws = std::fs::canonicalize(t.path()).unwrap();
        std::fs::create_dir_all(ws.join("docs")).unwrap();
        std::fs::write(ws.join("README.md"), "# workspace\n").unwrap();
        (t, ws)
    }

    fn product(goal: &str, ws: &Path) -> (String, ProposalKind) {
        let ((prompt, _, kind), _) = task_decision(goal, ws).unwrap();
        (prompt, kind)
    }

    // NEW: a goal naming a missing file (here in a folder that does not exist yet, like
    // v5 R1). The reference prompt equals the product prompt before the run, and stays
    // byte-equal to it after the run wrote the destination.
    #[test]
    fn new_destination_prompt_equals_the_product_before_and_after_the_write() {
        let (_t, ws) = workspace();
        let goal = "Save a one-sentence reminder about stretching the shoulders before swimming into notes/swim-tip.txt.";
        let (want, kind) = product(goal, &ws);
        assert_eq!(kind, ProposalKind::Document);
        assert!(want.ends_with("filename: notes/swim-tip.txt"), "{want}");
        assert_eq!(reference_prompt("new", goal, &ws), want);
        std::fs::create_dir_all(ws.join("notes")).unwrap();
        std::fs::write(ws.join("notes/swim-tip.txt"), "Stretch first.\n").unwrap();
        assert_eq!(reference_prompt("new", goal, &ws), want);
    }

    // EDIT: a goal naming an existing file. The reference `task` mode is the product
    // prompt with the edit block.
    #[test]
    fn edit_destination_prompt_equals_the_product() {
        let (_t, ws) = workspace();
        std::fs::write(ws.join("docs/plan.md"), "## Steps\n1. Start.\n").unwrap();
        let goal = "In the existing file docs/plan.md, add the line \"2. Finish.\" at the end.";
        let (want, kind) = product(goal, &ws);
        assert_eq!(kind, ProposalKind::Edit);
        assert!(
            want.contains("The file docs/plan.md already exists."),
            "{want}"
        );
        assert_eq!(reference_prompt("task", goal, &ws), want);
    }

    // Plain task: a goal naming no destination. Both reference modes are the product
    // prompt, the template with no added block.
    #[test]
    fn plain_task_prompt_equals_the_product() {
        let (_t, ws) = workspace();
        let goal = "Write a short note about brewing green tea.";
        let (want, kind) = product(goal, &ws);
        assert_eq!(kind, ProposalKind::Document);
        assert_eq!(want, aien_runtime::spine::proposal_prompt(goal));
        assert_eq!(reference_prompt("new", goal, &ws), want);
        assert_eq!(reference_prompt("task", goal, &ws), want);
    }
}

/// The model-visible prompt with the ALLEN context the product adds in front of
/// the task prompt (sc#384, v7 row R1-P): `NP1_REFERENCE_ALLEN=default` repeats
/// `spine` exactly, `persona::prefix_prompt` with `PersonaContext::defaults()`
/// (an engaged subject with no saved profile: persona state "default", revision
/// 0, as the campaign's fixture subject reports). `none` (the default) adds
/// nothing, the v6 prompt. Any other value is refused: a replay must say what
/// it replays. A saved profile (state "applied") is not replayed by this tool.
fn allen_prompt(prompt: &str, allen: &str) -> String {
    match allen {
        "none" => prompt.to_string(),
        "default" => aien_runtime::persona::prefix_prompt(
            prompt,
            Some(&aien_allen_profile::PersonaContext::defaults()),
        ),
        other => {
            eprintln!("NP1_REFERENCE_ALLEN={other}: only \"none\" or \"default\" is replayed");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod allen_tests {
    use super::{allen_prompt, reference_prompt};
    use aien_allen_profile::PersonaContext;

    const R1_GOAL: &str = "Save a one-sentence reminder about stretching the shoulders before swimming into notes/swim-tip.txt.";

    // The default ALLEN block goes in front of the whole task prompt, as the
    // product does it (persona::prefix_prompt after the plan is built), not
    // in front of the goal inside the "Goal:" line. v7 R1-P failed because the
    // replay carried no block at all.
    #[test]
    fn default_allen_block_prefixes_the_whole_task_prompt() {
        let t = tempfile::tempdir().unwrap();
        let ws = std::fs::canonicalize(t.path()).unwrap();
        let base = reference_prompt("new", R1_GOAL, &ws);
        let got = allen_prompt(&base, "default");
        let want = aien_runtime::persona::prefix_prompt(&base, Some(&PersonaContext::defaults()));
        assert_eq!(got, want);
        assert!(
            got.starts_with("[ALLEN style preferences saved by the user]\n"),
            "{got}"
        );
        assert!(
            got.contains("[end of style preferences]\n\nGoal: Save a one-sentence"),
            "{got}"
        );
        assert_eq!(allen_prompt(&base, "none"), base);
    }

    // Deterministic replay check against the v7 GB10 run (evidence-v7/run/
    // s3-reports/s3-report-R1.json, attempt 1: prompt_tokens 164,
    // prompt_ids_sha256 eee4e094...): the bytes this tool sends for R1 with
    // the default block, through the product's template and assistant prefix,
    // tokenize to exactly those ids. Needs the Qwen3-8B tokenizer:
    // AIEN_QWEN3_MODEL_DIR=<folder with tokenizer.json>; skipped otherwise.
    #[test]
    fn v7_r1_prompt_ids_are_reproduced_exactly() {
        let Ok(dir) = std::env::var("AIEN_QWEN3_MODEL_DIR") else {
            eprintln!("AIEN_QWEN3_MODEL_DIR not set: v7 R1 replay check skipped");
            return;
        };
        let tok =
            aien_inference_abi::ChatTokenizer::from_model_dir(std::path::Path::new(&dir), None)
                .expect("tokenizer");
        let t = tempfile::tempdir().unwrap();
        let ws = std::fs::canonicalize(t.path()).unwrap();
        let prompt = allen_prompt(&reference_prompt("new", R1_GOAL, &ws), "default");
        let msgs = vec![aien_runtime::ChatTurn {
            role: "user".into(),
            content: prompt,
        }];
        let text = format!(
            "{}{}",
            aien_runtime::format_chat(tok.template(), &msgs).expect("chat"),
            aien_runtime::spine::COMPOSE_ASSISTANT_PREFIX
        );
        let ids = tok.encode(&text).expect("encode");
        assert_eq!(ids.len(), 164, "prompt tokens");
        assert_eq!(
            aien_runtime::spine::token_ids_sha256(&ids),
            "eee4e094335087285f3c4ff52fda6f9ecc946c3f145abbe9fbbccf403a6adfe6"
        );
    }
}

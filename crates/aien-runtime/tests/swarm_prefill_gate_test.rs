//! PREFILL-GATE end to end: a launched swarm must run the root prompt through
//! the model before any branch decodes.
//!
//! Uses only APIs that already exist on main (cfd9982), so this file also runs
//! against main, where it must FAIL (see scripts/prefill-gate/fails-on-main.sh):
//! on main SwarmManager::launch_swarm forks branches from allocated-but-zeroed
//! root blocks (crates/aien-runtime/src/swarm.rs:95-111 at cfd9982) and the
//! scheduler decodes them with no prefill at all.
//!
//! The stock MockInferenceBackend ignores prompt content, so this test uses a
//! recording backend that logs every prefill span and every decode, in order.

use std::collections::HashMap;

use aien_inference_abi::{
    AienInferenceBackend, DecodeOutput, ModelConfig, ScheduledBatch, StepMetrics,
};
use aien_kv_cache::create_shared_kv_manager;
use aien_runtime::spine::AienRuntimeSpine;
use aien_runtime::swarm::SwarmConfig;
use aien_scheduler::SchedulerConfig;
use async_trait::async_trait;

#[derive(Debug, Clone)]
enum Event {
    Prefill { id: u64, tokens: Vec<u32> },
    Decode { id: u64 },
}

#[derive(Default)]
struct RecordingBackend {
    events: Vec<Event>,
}

#[async_trait]
impl AienInferenceBackend for RecordingBackend {
    async fn load_model(&mut self, _config: &ModelConfig) -> Result<(), String> {
        Ok(())
    }

    async fn execute_step(
        &mut self,
        batch: &ScheduledBatch,
    ) -> Result<(Vec<DecodeOutput>, StepMetrics), String> {
        let mut outputs = Vec::new();
        let mut prefill_tokens = 0;
        for req in &batch.prefill_requests {
            prefill_tokens += req.prompt_tokens.len();
            self.events.push(Event::Prefill {
                id: req.request_id,
                tokens: req.prompt_tokens.clone(),
            });
            outputs.push(DecodeOutput::Token {
                request_id: req.request_id,
                token_id: 100,
                logprob: None,
            });
        }
        for &id in &batch.decode_requests {
            self.events.push(Event::Decode { id });
            outputs.push(DecodeOutput::Token {
                request_id: id,
                token_id: 101,
                logprob: None,
            });
        }
        let metrics = StepMetrics {
            prefill_tokens_processed: prefill_tokens,
            decode_tokens_emitted: batch.decode_requests.len() + batch.prefill_requests.len(),
            step_latency_us: 0,
            active_kv_blocks: batch.block_tables.values().map(|v| v.len()).sum(),
        };
        Ok((outputs, metrics))
    }
}

#[tokio::test]
async fn prefill_gate_swarm_branches_never_decode_before_root_prompt_prefill() {
    let kv_manager = create_shared_kv_manager(256, 16);
    let sched_cfg = SchedulerConfig {
        max_batch_size: 32,
        max_batch_tokens: 2048,
        max_prefill_tokens: 1024,
        prefill_chunk_size: 16,
        chunk_prefill: true,
        watermark_blocks: 4,
    };
    let mut spine = AienRuntimeSpine::new(64, sched_cfg, kv_manager.clone());
    let mut backend = RecordingBackend::default();

    let prompt: Vec<u32> = (500..540).collect();
    let branch_count = 4usize;
    let config = SwarmConfig {
        model_handle: 1,
        branch_count,
        max_active_sequences: branch_count,
        max_tokens_per_branch: 3,
        root_world_id: 0,
        priority: 1,
    };
    let swarm_id = spine.launch_swarm(config, &prompt).expect("launch");
    let swarm = spine
        .swarm_manager
        .get_swarm(swarm_id)
        .expect("swarm record")
        .clone();
    let root = swarm.root_sequence_id.as_u64();
    let branches: Vec<u64> = swarm.branch_sequences.iter().map(|s| s.as_u64()).collect();

    let mut steps = 0;
    while (spine.scheduler.running_count() > 0 || spine.scheduler.waiting_count() > 0)
        && steps < 100
    {
        steps += 1;
        spine.step(&mut backend).await.expect("step");
    }

    // Replay the backend log in order. Every branch decode must come after
    // the full prompt was run through the model, for the shared root or for
    // that branch itself.
    let mut prefilled: HashMap<u64, Vec<u32>> = HashMap::new();
    let mut branch_decodes = 0usize;
    let covers_prompt = |seen: Option<&Vec<u32>>| {
        seen.map(|s| s.len() >= prompt.len() && s[..prompt.len()] == prompt[..])
            .unwrap_or(false)
    };
    for event in &backend.events {
        match event {
            Event::Prefill { id, tokens } => {
                prefilled.entry(*id).or_default().extend_from_slice(tokens);
            }
            Event::Decode { id } => {
                if branches.contains(id) {
                    branch_decodes += 1;
                    assert!(
                        covers_prompt(prefilled.get(&root)) || covers_prompt(prefilled.get(id)),
                        "PREFILL_GATE_VIOLATION: branch {} decoded before its prompt was prefilled \
                         (root prefilled {:?} tokens, branch prefilled {:?} tokens, prompt {} tokens)",
                        id,
                        prefilled.get(&root).map(|v| v.len()),
                        prefilled.get(id).map(|v| v.len()),
                        prompt.len()
                    );
                }
            }
        }
    }

    assert!(
        branch_decodes > 0,
        "branches must decode once the root prompt is prefilled"
    );
    assert_eq!(
        spine.scheduler.metrics().finished_requests as usize,
        branch_count,
        "every branch must finish"
    );
}

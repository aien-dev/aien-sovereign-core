use aien_inference_abi::{
    AienInferenceBackend, DecodeOutput, FinishReason, ScheduledBatch, SequenceRequest, StepMetrics,
};
use aien_kv_cache::AienKvManager;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    pub max_batch_size: usize,
    pub max_batch_tokens: usize,
    pub max_prefill_tokens: usize,
    pub chunk_prefill: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 64,
            max_batch_tokens: 4096,
            max_prefill_tokens: 2048,
            chunk_prefill: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunningSequence {
    pub request: SequenceRequest,
    pub tokens_generated: usize,
    pub is_prefilled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SchedulerMetrics {
    pub total_steps: u64,
    pub admitted_requests: u64,
    pub finished_requests: u64,
    pub preempted_requests: u64,
    pub total_prefill_tokens: u64,
    pub total_decode_tokens: u64,
    pub avg_step_latency_us: f64,
}

pub struct AienScheduler {
    config: SchedulerConfig,
    kv_manager: Arc<RwLock<AienKvManager>>,
    waiting_queue: VecDeque<SequenceRequest>,
    preempted_queue: VecDeque<SequenceRequest>,
    running_sequences: HashMap<u64, RunningSequence>,
    step_id: u64,
    metrics: SchedulerMetrics,
}

impl AienScheduler {
    pub fn new(config: SchedulerConfig, kv_manager: Arc<RwLock<AienKvManager>>) -> Self {
        Self {
            config,
            kv_manager,
            waiting_queue: VecDeque::new(),
            preempted_queue: VecDeque::new(),
            running_sequences: HashMap::new(),
            step_id: 0,
            metrics: SchedulerMetrics::default(),
        }
    }

    pub fn submit_request(&mut self, request: SequenceRequest) {
        // Higher priority requests placed toward front
        let insert_idx = self
            .waiting_queue
            .iter()
            .position(|r| r.priority < request.priority)
            .unwrap_or(self.waiting_queue.len());
        self.waiting_queue.insert(insert_idx, request);
    }

    pub fn waiting_count(&self) -> usize {
        self.waiting_queue.len()
    }

    pub fn running_count(&self) -> usize {
        self.running_sequences.len()
    }

    /// Forks an existing running sequence for instant zero-copy subagent branching.
    pub fn fork_subagent(&mut self, parent_id: u64, child_id: u64) -> Result<(), String> {
        let parent = self
            .running_sequences
            .get(&parent_id)
            .ok_or_else(|| format!("Parent sequence {} not found in running set", parent_id))?
            .clone();

        // Fork in KV manager
        {
            let mut kv = self.kv_manager.write();
            kv.fork_sequence(parent_id, child_id)?;
        }

        let mut child_req = parent.request.clone();
        child_req.request_id = child_id;

        self.running_sequences.insert(
            child_id,
            RunningSequence {
                request: child_req,
                tokens_generated: parent.tokens_generated,
                is_prefilled: true,
            },
        );

        Ok(())
    }

    /// Constructs the next batch according to token budgets and available KV memory.
    pub fn build_scheduled_batch(&mut self) -> Result<Option<ScheduledBatch>, String> {
        if self.running_sequences.is_empty()
            && self.preempted_queue.is_empty()
            && self.waiting_queue.is_empty()
        {
            return Ok(None);
        }

        self.step_id += 1;
        let mut prefill_requests = Vec::new();
        let mut decode_requests = Vec::new();
        let mut block_tables = HashMap::new();

        let mut remaining_batch_tokens = self.config.max_batch_tokens;
        let mut remaining_prefill_tokens = self.config.max_prefill_tokens;

        // 1. Prioritize ongoing decodes (Continuous Batching)
        let running_ids: Vec<u64> = self.running_sequences.keys().copied().collect();
        for req_id in running_ids {
            if remaining_batch_tokens == 0 {
                break;
            }
            if let Some(table) = self.kv_manager.read().get_block_table(req_id) {
                decode_requests.push(req_id);
                block_tables.insert(req_id, table.block_ids.clone());
                remaining_batch_tokens = remaining_batch_tokens.saturating_sub(1);
            }
        }

        // 2. Admit preempted or waiting requests if batch capacity and KV blocks allow
        while !self.preempted_queue.is_empty() || !self.waiting_queue.is_empty() {
            if self.running_sequences.len() >= self.config.max_batch_size {
                break;
            }

            let next_req = if let Some(preempted) = self.preempted_queue.pop_front() {
                preempted
            } else if let Some(waiting) = self.waiting_queue.pop_front() {
                waiting
            } else {
                break;
            };

            let req_tokens = next_req.prompt_tokens.len();
            if req_tokens > remaining_batch_tokens || req_tokens > remaining_prefill_tokens {
                // Token budget exhausted for this step, return to front of waiting queue
                self.waiting_queue.push_front(next_req);
                break;
            }

            // Attempt KV allocation
            let alloc_result = {
                let mut kv = self.kv_manager.write();
                kv.allocate_sequence(next_req.request_id, &next_req.prompt_tokens)
            };

            match alloc_result {
                Ok(blocks) => {
                    block_tables.insert(next_req.request_id, blocks);
                    remaining_batch_tokens = remaining_batch_tokens.saturating_sub(req_tokens);
                    remaining_prefill_tokens = remaining_prefill_tokens.saturating_sub(req_tokens);

                    self.running_sequences.insert(
                        next_req.request_id,
                        RunningSequence {
                            request: next_req.clone(),
                            tokens_generated: 0,
                            is_prefilled: false,
                        },
                    );
                    prefill_requests.push(next_req);
                    self.metrics.admitted_requests += 1;
                }
                Err(_) => {
                    // KV cache full, return request to waiting queue
                    self.waiting_queue.push_front(next_req);
                    break;
                }
            }
        }

        if prefill_requests.is_empty() && decode_requests.is_empty() {
            return Ok(None);
        }

        Ok(Some(ScheduledBatch {
            prefill_requests,
            decode_requests,
            block_tables,
            step_id: self.step_id,
        }))
    }

    /// Advances the engine by one step using the configured backend.
    pub async fn step<B: AienInferenceBackend>(
        &mut self,
        backend: &mut B,
    ) -> Result<Option<(Vec<DecodeOutput>, StepMetrics)>, String> {
        let batch = match self.build_scheduled_batch()? {
            Some(b) => b,
            None => return Ok(None),
        };

        let t0 = std::time::Instant::now();
        let (outputs, metrics) = backend.execute_step(&batch).await?;
        let step_latency = t0.elapsed().as_micros() as u64;

        // Process outputs and manage KV state
        let mut final_outputs = Vec::new();

        for output in outputs {
            match output {
                DecodeOutput::Token {
                    request_id,
                    token_id,
                    logprob,
                } => {
                    let mut is_finished = false;
                    let mut finish_reason = FinishReason::StopToken;
                    let mut total_tokens = 0;

                    if let Some(seq) = self.running_sequences.get_mut(&request_id) {
                        seq.is_prefilled = true;
                        seq.tokens_generated += 1;
                        total_tokens = seq.tokens_generated;

                        if seq.request.sampling_params.stop_token_ids.contains(&token_id) {
                            is_finished = true;
                            finish_reason = FinishReason::StopToken;
                        } else if seq.tokens_generated >= seq.request.sampling_params.max_tokens {
                            is_finished = true;
                            finish_reason = FinishReason::LengthLimit;
                        } else {
                            // Append token in KV manager
                            let append_result = {
                                let mut kv = self.kv_manager.write();
                                kv.append_token(request_id)
                            };

                            if append_result.is_err() {
                                // KV cache pressure: preempt sequence
                                is_finished = true;
                                finish_reason = FinishReason::Preempted;
                            }
                        }
                    }

                    if is_finished {
                        let running = self.running_sequences.remove(&request_id);
                        if finish_reason == FinishReason::Preempted {
                            if let Some(r) = running {
                                self.preempted_queue.push_back(r.request);
                                self.metrics.preempted_requests += 1;
                            }
                        } else {
                            self.kv_manager.write().free_sequence(request_id);
                            self.metrics.finished_requests += 1;
                        }

                        final_outputs.push(DecodeOutput::Finished {
                            request_id,
                            reason: finish_reason,
                            total_tokens,
                        });
                    } else {
                        final_outputs.push(DecodeOutput::Token {
                            request_id,
                            token_id,
                            logprob,
                        });
                    }
                }
                DecodeOutput::Finished {
                    request_id,
                    reason,
                    total_tokens,
                } => {
                    self.running_sequences.remove(&request_id);
                    self.kv_manager.write().free_sequence(request_id);
                    self.metrics.finished_requests += 1;
                    final_outputs.push(DecodeOutput::Finished {
                        request_id,
                        reason,
                        total_tokens,
                    });
                }
            }
        }

        self.metrics.total_steps += 1;
        self.metrics.total_prefill_tokens += metrics.prefill_tokens_processed as u64;
        self.metrics.total_decode_tokens += metrics.decode_tokens_emitted as u64;
        let n = self.metrics.total_steps as f64;
        self.metrics.avg_step_latency_us =
            ((n - 1.0) * self.metrics.avg_step_latency_us + step_latency as f64) / n;

        Ok(Some((final_outputs, metrics)))
    }

    pub fn metrics(&self) -> &SchedulerMetrics {
        &self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aien_inference_abi::{MockInferenceBackend, SamplingParams};
    use aien_kv_cache::create_shared_kv_manager;

    #[tokio::test]
    async fn test_scheduler_lifecycle() {
        let kv_manager = create_shared_kv_manager(100, 16);
        let config = SchedulerConfig::default();
        let mut scheduler = AienScheduler::new(config, kv_manager);

        let mut backend = MockInferenceBackend::new(5);

        let req = SequenceRequest {
            request_id: 1,
            prompt_tokens: vec![10, 20, 30, 40],
            sampling_params: SamplingParams {
                temperature: 0.7,
                top_p: 0.9,
                max_tokens: 3,
                stop_token_ids: vec![999],
            },
            arrival_time_ns: 0,
            priority: 1,
        };

        scheduler.submit_request(req);
        assert_eq!(scheduler.waiting_count(), 1);

        // Step 1: Prefill + first token
        let res1 = scheduler.step(&mut backend).await.unwrap().unwrap();
        assert_eq!(res1.0.len(), 1);
        assert_eq!(scheduler.running_count(), 1);

        // Step 2: Decode token 2
        let res2 = scheduler.step(&mut backend).await.unwrap().unwrap();
        assert_eq!(res2.0.len(), 1);
        assert_eq!(scheduler.running_count(), 1);

        // Step 3: Decode token 3 -> hits max_tokens (3), finishes
        let res3 = scheduler.step(&mut backend).await.unwrap().unwrap();
        assert_eq!(res3.0.len(), 1);
        match &res3.0[0] {
            DecodeOutput::Finished {
                request_id,
                reason,
                total_tokens,
            } => {
                assert_eq!(*request_id, 1);
                assert_eq!(*reason, FinishReason::LengthLimit);
                assert_eq!(*total_tokens, 3);
            }
            _ => panic!("Expected Finished output"),
        }
        assert_eq!(scheduler.running_count(), 0);
        assert_eq!(scheduler.metrics().finished_requests, 1);
    }

    #[tokio::test]
    async fn test_scheduler_subagent_fork() {
        let kv_manager = create_shared_kv_manager(100, 16);
        let config = SchedulerConfig::default();
        let mut scheduler = AienScheduler::new(config, kv_manager.clone());
        let mut backend = MockInferenceBackend::new(5);

        let parent_req = SequenceRequest {
            request_id: 10,
            prompt_tokens: vec![1, 2, 3, 4, 5],
            sampling_params: SamplingParams {
                temperature: 0.5,
                top_p: 0.9,
                max_tokens: 5,
                stop_token_ids: vec![],
            },
            arrival_time_ns: 0,
            priority: 10,
        };

        scheduler.submit_request(parent_req);
        scheduler.step(&mut backend).await.unwrap();

        // Fork subagent 20 from parent 10
        scheduler.fork_subagent(10, 20).unwrap();
        assert_eq!(scheduler.running_count(), 2);

        // Verify KV blocks are shared
        let metrics = kv_manager.read().metrics();
        assert!(metrics.shared_blocks > 0);
    }
}

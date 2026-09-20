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
    pub prefill_chunk_size: usize,
    pub chunk_prefill: bool,
    pub watermark_blocks: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 64,
            max_batch_tokens: 4096,
            max_prefill_tokens: 2048,
            prefill_chunk_size: 512,
            chunk_prefill: true,
            watermark_blocks: 4,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunningSequence {
    pub request: SequenceRequest,
    pub tokens_generated: usize,
    pub prompt_tokens_prefilled: usize,
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
    pub chunked_prefill_steps: u64,
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
        self.waiting_queue.push_back(request);
    }

    pub fn waiting_count(&self) -> usize {
        self.waiting_queue.len()
    }

    pub fn preempted_count(&self) -> usize {
        self.preempted_queue.len()
    }

    pub fn running_count(&self) -> usize {
        self.running_sequences.len()
    }

    /// Zero-copy subagent sequence branching
    pub fn fork_subagent(&mut self, parent_id: u64, child_id: u64) -> Result<(), String> {
        let parent = self
            .running_sequences
            .get(&parent_id)
            .ok_or_else(|| format!("Parent sequence {} not found in running pool", parent_id))?
            .clone();

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
                prompt_tokens_prefilled: parent.prompt_tokens_prefilled,
                is_prefilled: parent.is_prefilled,
            },
        );

        Ok(())
    }

    /// Builds the next scheduled batch enforcing chunked prefill budgets and watermark preemption.
    pub fn build_scheduled_batch(&mut self) -> Result<Option<ScheduledBatch>, String> {
        self.step_id += 1;
        let mut prefill_requests = Vec::new();
        let mut decode_requests = Vec::new();
        let mut block_tables = HashMap::new();

        let mut current_tokens = 0;
        let mut prefill_budget = self.config.max_prefill_tokens;

        // 1. Watermark Memory Pressure Check: Preempt if below watermark
        let mut is_under_memory_pressure = false;
        {
            let kv = self.kv_manager.read();
            let available = kv.available_blocks();
            if available < self.config.watermark_blocks && !self.running_sequences.is_empty() {
                is_under_memory_pressure = true;
                // Find lowest priority running sequence to preempt
                let mut candidates: Vec<(u64, u8)> = self
                    .running_sequences
                    .iter()
                    .map(|(&id, seq)| (id, seq.request.priority))
                    .collect();
                candidates.sort_by_key(|c| c.1); // lowest priority first

                if let Some((preempt_id, _)) = candidates.first() {
                    let preempt_id = *preempt_id;
                    drop(kv); // release lock before write
                    if let Some(running) = self.running_sequences.remove(&preempt_id) {
                        let _ = self.kv_manager.write().free_sequence(preempt_id);
                        self.preempted_queue.push_back(running.request);
                        self.metrics.preempted_requests += 1;
                    }
                }
            }
        }

        // 2. Schedule active decode sequences (and sequences in intermediate chunked prefill)
        let mut running_ids: Vec<u64> = self.running_sequences.keys().copied().collect();
        running_ids.sort(); // deterministic ordering

        for seq_id in running_ids {
            if decode_requests.len() + prefill_requests.len() >= self.config.max_batch_size {
                break;
            }
            if current_tokens >= self.config.max_batch_tokens {
                break;
            }

            let seq = self.running_sequences.get_mut(&seq_id).unwrap();

            if seq.is_prefilled {
                // Active decode step
                if let Some(table) = self.kv_manager.read().get_block_table(seq_id) {
                    block_tables.insert(seq_id, table.block_ids.clone());
                    decode_requests.push(seq_id);
                    current_tokens += 1;
                }
            } else {
                // Continuing chunked prefill for already admitted sequence
                let total_prompt_len = seq.request.prompt_tokens.len();
                let remaining = total_prompt_len.saturating_sub(seq.prompt_tokens_prefilled);
                let chunk_size = std::cmp::min(remaining, self.config.prefill_chunk_size);
                let chunk_size = std::cmp::min(chunk_size, prefill_budget);

                if chunk_size > 0 {
                    let start = seq.prompt_tokens_prefilled;
                    let end = start + chunk_size;
                    let chunk_tokens = seq.request.prompt_tokens[start..end].to_vec();

                    let mut chunk_req = seq.request.clone();
                    chunk_req.prompt_tokens = chunk_tokens;

                    if let Some(table) = self.kv_manager.read().get_block_table(seq_id) {
                        block_tables.insert(seq_id, table.block_ids.clone());
                    }

                    prefill_requests.push(chunk_req);
                    current_tokens += chunk_size;
                    prefill_budget = prefill_budget.saturating_sub(chunk_size);
                    self.metrics.chunked_prefill_steps += 1;
                }
            }
        }

        // Only admit new or preempted requests if memory pressure is clear
        if !is_under_memory_pressure {
            // 3. Admit preempted requests first (starvation protection)
            while let Some(req) = self.preempted_queue.pop_front() {
                if decode_requests.len() + prefill_requests.len() >= self.config.max_batch_size {
                    self.preempted_queue.push_front(req);
                    break;
                }

                let prompt_len = req.prompt_tokens.len();
                let chunk_size = if self.config.chunk_prefill {
                    std::cmp::min(prompt_len, self.config.prefill_chunk_size)
                } else {
                    prompt_len
                };

                if chunk_size > prefill_budget
                    || current_tokens + chunk_size > self.config.max_batch_tokens
                {
                    self.preempted_queue.push_front(req);
                    break;
                }

                let (alloc_result, is_already_prefilled) = {
                    let mut kv = self.kv_manager.write();
                    if let Some(table) = kv.get_block_table(req.request_id) {
                        let is_prefilled = table.total_tokens >= prompt_len;
                        (Ok(table.block_ids.clone()), is_prefilled)
                    } else {
                        (kv.allocate_sequence(req.request_id, &req.prompt_tokens), false)
                    }
                };

                match alloc_result {
                    Ok(block_ids) => {
                        block_tables.insert(req.request_id, block_ids);

                        if is_already_prefilled {
                            self.running_sequences.insert(
                                req.request_id,
                                RunningSequence {
                                    request: req.clone(),
                                    tokens_generated: 0,
                                    prompt_tokens_prefilled: prompt_len,
                                    is_prefilled: true,
                                },
                            );
                            decode_requests.push(req.request_id);
                            current_tokens += 1;
                        } else {
                            let mut chunk_req = req.clone();
                            chunk_req.prompt_tokens = req.prompt_tokens[0..chunk_size].to_vec();

                            let is_fully_prefilled = chunk_size == prompt_len;
                            self.running_sequences.insert(
                                req.request_id,
                                RunningSequence {
                                    request: req,
                                    tokens_generated: 0,
                                    prompt_tokens_prefilled: 0,
                                    is_prefilled: is_fully_prefilled,
                                },
                            );

                            prefill_requests.push(chunk_req);
                            current_tokens += chunk_size;
                            prefill_budget = prefill_budget.saturating_sub(chunk_size);
                        }
                        self.metrics.admitted_requests += 1;
                    }
                    Err(_) => {
                        self.preempted_queue.push_front(req);
                        break;
                    }
                }
            }

            // 4. Admit new waiting requests
            while let Some(req) = self.waiting_queue.pop_front() {
                if decode_requests.len() + prefill_requests.len() >= self.config.max_batch_size {
                    self.waiting_queue.push_front(req);
                    break;
                }

                let prompt_len = req.prompt_tokens.len();
                let chunk_size = if self.config.chunk_prefill {
                    std::cmp::min(prompt_len, self.config.prefill_chunk_size)
                } else {
                    prompt_len
                };

                if chunk_size > prefill_budget
                    || current_tokens + chunk_size > self.config.max_batch_tokens
                {
                    self.waiting_queue.push_front(req);
                    break;
                }

                let (alloc_result, is_already_prefilled) = {
                    let mut kv = self.kv_manager.write();
                    if let Some(table) = kv.get_block_table(req.request_id) {
                        let is_prefilled = table.total_tokens >= prompt_len;
                        (Ok(table.block_ids.clone()), is_prefilled)
                    } else {
                        (kv.allocate_sequence(req.request_id, &req.prompt_tokens), false)
                    }
                };

                match alloc_result {
                    Ok(block_ids) => {
                        block_tables.insert(req.request_id, block_ids);

                        if is_already_prefilled {
                            self.running_sequences.insert(
                                req.request_id,
                                RunningSequence {
                                    request: req.clone(),
                                    tokens_generated: 0,
                                    prompt_tokens_prefilled: prompt_len,
                                    is_prefilled: true,
                                },
                            );
                            decode_requests.push(req.request_id);
                            current_tokens += 1;
                        } else {
                            let mut chunk_req = req.clone();
                            chunk_req.prompt_tokens = req.prompt_tokens[0..chunk_size].to_vec();

                            let is_fully_prefilled = chunk_size == prompt_len;
                            self.running_sequences.insert(
                                req.request_id,
                                RunningSequence {
                                    request: req,
                                    tokens_generated: 0,
                                    prompt_tokens_prefilled: 0,
                                    is_prefilled: is_fully_prefilled,
                                },
                            );

                            prefill_requests.push(chunk_req);
                            current_tokens += chunk_size;
                            prefill_budget = prefill_budget.saturating_sub(chunk_size);
                        }
                        self.metrics.admitted_requests += 1;
                    }
                    Err(_) => {
                        self.waiting_queue.push_front(req);
                        break;
                    }
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

        let mut final_outputs = Vec::new();

        // Update prefill progress for chunked sequences
        for prefill_req in &batch.prefill_requests {
            if let Some(seq) = self.running_sequences.get_mut(&prefill_req.request_id) {
                if !seq.is_prefilled {
                    seq.prompt_tokens_prefilled += prefill_req.prompt_tokens.len();
                    if seq.prompt_tokens_prefilled >= seq.request.prompt_tokens.len() {
                        seq.is_prefilled = true;
                    }
                }
            }
        }

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
                    let mut should_emit_token = false;

                    if let Some(seq) = self.running_sequences.get_mut(&request_id) {
                        if seq.is_prefilled {
                            should_emit_token = true;
                            seq.tokens_generated += 1;
                            total_tokens = seq.tokens_generated;

                            if seq
                                .request
                                .sampling_params
                                .stop_token_ids
                                .contains(&token_id)
                            {
                                is_finished = true;
                                finish_reason = FinishReason::StopToken;
                            } else if seq.tokens_generated >= seq.request.sampling_params.max_tokens
                            {
                                is_finished = true;
                                finish_reason = FinishReason::LengthLimit;
                            } else {
                                // Append token in KV manager
                                let append_result = {
                                    let mut kv = self.kv_manager.write();
                                    kv.append_token(request_id)
                                };

                                if append_result.is_err() {
                                    is_finished = true;
                                    finish_reason = FinishReason::Preempted;
                                }
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
                            let _ = self.kv_manager.write().free_sequence(request_id);
                            self.metrics.finished_requests += 1;
                        }

                        final_outputs.push(DecodeOutput::Finished {
                            request_id,
                            reason: finish_reason,
                            total_tokens,
                        });
                    } else if should_emit_token {
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
                    let _ = self.kv_manager.write().free_sequence(request_id);
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
    async fn test_chunked_prefill_segmentation() {
        let kv_manager = create_shared_kv_manager(100, 16);
        let config = SchedulerConfig {
            max_batch_size: 16,
            max_batch_tokens: 1024,
            max_prefill_tokens: 512,
            prefill_chunk_size: 64, // 64 token chunks
            chunk_prefill: true,
            watermark_blocks: 2,
        };
        let mut scheduler = AienScheduler::new(config, kv_manager);
        let mut backend = MockInferenceBackend::new(1);

        // 128 tokens prompt -> should segment into 2 chunks of 64 tokens
        let req = SequenceRequest {
            request_id: 100,
            prompt_tokens: (0..128).collect(),
            sampling_params: SamplingParams {
                temperature: 0.0,
                top_p: 1.0,
                max_tokens: 2,
                stop_token_ids: vec![],
            },
            arrival_time_ns: 0,
            priority: 1,
        };

        scheduler.submit_request(req);

        // Step 1: Processes chunk 1 (64 tokens, remaining 64)
        let res1 = scheduler.step(&mut backend).await.unwrap().unwrap();
        assert_eq!(res1.1.prefill_tokens_processed, 64);
        assert_eq!(scheduler.running_count(), 1);

        // Step 2: Processes chunk 2 (64 tokens, completes prompt)
        let res2 = scheduler.step(&mut backend).await.unwrap().unwrap();
        assert_eq!(res2.1.prefill_tokens_processed, 64);

        // Step 3: Decode begins (1 token generated)
        let res3 = scheduler.step(&mut backend).await.unwrap().unwrap();
        assert_eq!(res3.1.decode_tokens_emitted, 1);
    }

    #[tokio::test]
    async fn test_watermark_pressure_preemption() {
        // Pool with 6 blocks, watermark = 3
        let kv_manager = create_shared_kv_manager(6, 16);
        let config = SchedulerConfig {
            max_batch_size: 4,
            max_batch_tokens: 1024,
            max_prefill_tokens: 512,
            prefill_chunk_size: 128,
            chunk_prefill: false,
            watermark_blocks: 3,
        };
        let mut scheduler = AienScheduler::new(config, kv_manager.clone());
        let mut backend = MockInferenceBackend::new(1);

        // Req 1 consumes 2 blocks (32 tokens)
        scheduler.submit_request(SequenceRequest {
            request_id: 1,
            prompt_tokens: (0..32).collect(),
            sampling_params: SamplingParams {
                temperature: 0.0,
                top_p: 1.0,
                max_tokens: 10,
                stop_token_ids: vec![],
            },
            arrival_time_ns: 0,
            priority: 5,
        });

        // Req 2 consumes 2 blocks (32 tokens)
        scheduler.submit_request(SequenceRequest {
            request_id: 2,
            prompt_tokens: (0..32).collect(),
            sampling_params: SamplingParams {
                temperature: 0.0,
                top_p: 1.0,
                max_tokens: 10,
                stop_token_ids: vec![],
            },
            arrival_time_ns: 0,
            priority: 1, // lower priority
        });

        // Step 1: Admits both (free blocks = 6 - 4 = 2, which is < watermark 3)
        scheduler.step(&mut backend).await.unwrap();

        // Step 2: Memory pressure triggers preemption of lower priority req 2
        scheduler.step(&mut backend).await.unwrap();
        assert_eq!(scheduler.metrics().preempted_requests, 1);
        assert_eq!(scheduler.preempted_count(), 1);
        assert_eq!(scheduler.running_count(), 1);
    }
}

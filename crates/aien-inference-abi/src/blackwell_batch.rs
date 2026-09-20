use crate::ScheduledBatch;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Single token row within a unified Blackwell batch plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlackwellBatchRow {
    pub seq_id: u64,
    pub token_id: u32,
    pub pos: usize,
    pub is_decode: bool,
    pub is_terminal: bool,
}

/// Unified Blackwell execution batch plan with T = D + P rows.
/// Consolidates linear projections (QKV, MLP gate/up, down) into single GEMM operations
/// across all active decode sequences and chunked prefill spans.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackwellBatchPlan {
    pub step_id: u64,
    pub rows: Vec<BlackwellBatchRow>,
    pub decode_count: usize,
    pub prefill_count: usize,
    pub total_rows: usize,
    pub terminal_row_indices: Vec<usize>,
    pub block_tables: HashMap<u64, Vec<i32>>,
    pub context_lens: HashMap<u64, usize>,
    pub max_blocks_per_seq: usize,
}

impl BlackwellBatchPlan {
    /// Constructs a unified batch plan from a ScheduledBatch and current sequence states.
    pub fn from_scheduled_batch(
        batch: &ScheduledBatch,
        last_tokens: &HashMap<u64, (u32, usize)>,
        prefill_positions: &HashMap<u64, usize>,
    ) -> Self {
        let mut rows = Vec::new();
        let mut terminal_row_indices = Vec::new();
        let mut decode_count = 0;
        let mut prefill_count = 0;

        // 1. Decode rows (D rows)
        for &seq_id in &batch.decode_requests {
            let (token_id, pos) = last_tokens.get(&seq_id).copied().unwrap_or((1, 0));
            let row_idx = rows.len();
            rows.push(BlackwellBatchRow {
                seq_id,
                token_id,
                pos,
                is_decode: true,
                is_terminal: true,
            });
            terminal_row_indices.push(row_idx);
            decode_count += 1;
        }

        // 2. Prefill rows (P rows)
        for req in &batch.prefill_requests {
            let seq_id = req.request_id;
            let start_pos = prefill_positions.get(&seq_id).copied().unwrap_or(0);
            let n_tokens = req.prompt_tokens.len();

            for (idx, &token_id) in req.prompt_tokens.iter().enumerate() {
                let pos = start_pos + idx;
                let is_terminal = idx == n_tokens.saturating_sub(1);
                let row_idx = rows.len();

                rows.push(BlackwellBatchRow {
                    seq_id,
                    token_id,
                    pos,
                    is_decode: false,
                    is_terminal,
                });

                if is_terminal {
                    terminal_row_indices.push(row_idx);
                }
                prefill_count += 1;
            }
        }

        let total_rows = rows.len();

        let mut block_tables = HashMap::new();
        let mut max_blocks = 1;
        for (&seq_id, blocks) in &batch.block_tables {
            let i32_blocks: Vec<i32> = blocks.iter().map(|&b| b as i32).collect();
            if i32_blocks.len() > max_blocks {
                max_blocks = i32_blocks.len();
            }
            block_tables.insert(seq_id, i32_blocks);
        }

        let mut context_lens = HashMap::new();
        for row in &rows {
            context_lens.insert(row.seq_id, row.pos + 1);
        }

        Self {
            step_id: batch.step_id,
            rows,
            decode_count,
            prefill_count,
            total_rows,
            terminal_row_indices,
            block_tables,
            context_lens,
            max_blocks_per_seq: max_blocks,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.total_rows == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SamplingParams, SequenceRequest};

    #[test]
    fn test_batch_plan_construction_and_terminal_row_indices() {
        let mut block_tables = HashMap::new();
        block_tables.insert(1, vec![0, 1]);
        block_tables.insert(2, vec![2]);
        block_tables.insert(100, vec![3, 4]);

        let prefill_req = SequenceRequest {
            request_id: 100,
            prompt_tokens: vec![10, 20, 30],
            sampling_params: SamplingParams::default(),
            arrival_time_ns: 0,
            priority: 1,
        };

        let batch = ScheduledBatch {
            prefill_requests: vec![prefill_req],
            decode_requests: vec![1, 2],
            block_tables,
            step_id: 42,
        };

        let mut last_tokens = HashMap::new();
        last_tokens.insert(1, (101, 15));
        last_tokens.insert(2, (102, 7));

        let prefill_positions = HashMap::new();

        let plan =
            BlackwellBatchPlan::from_scheduled_batch(&batch, &last_tokens, &prefill_positions);

        assert_eq!(plan.step_id, 42);
        assert_eq!(plan.decode_count, 2);
        assert_eq!(plan.prefill_count, 3);
        assert_eq!(plan.total_rows, 5); // T = 2 + 3 = 5

        // Rows 0 and 1 are decode (terminal)
        assert!(plan.rows[0].is_decode);
        assert!(plan.rows[0].is_terminal);
        assert_eq!(plan.rows[0].token_id, 101);

        assert!(plan.rows[1].is_decode);
        assert!(plan.rows[1].is_terminal);
        assert_eq!(plan.rows[1].token_id, 102);

        // Rows 2 and 3 are intermediate prefill (non-terminal)
        assert!(!plan.rows[2].is_decode);
        assert!(!plan.rows[2].is_terminal);
        assert_eq!(plan.rows[2].token_id, 10);

        assert!(!plan.rows[3].is_decode);
        assert!(!plan.rows[3].is_terminal);
        assert_eq!(plan.rows[3].token_id, 20);

        // Row 4 is final prefill (terminal)
        assert!(!plan.rows[4].is_decode);
        assert!(plan.rows[4].is_terminal);
        assert_eq!(plan.rows[4].token_id, 30);

        // Terminal rows must be [0, 1, 4]
        assert_eq!(plan.terminal_row_indices, vec![0, 1, 4]);
    }
}

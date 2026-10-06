//! Typed Operator Control Protocol and RuntimeController
//! Connects external operator CLI tools to the in-process runtime over local domain sockets.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchSwarmReq {
    pub model_handle: u64,
    pub branch_count: usize,
    pub max_active_sequences: usize,
    pub max_tokens_per_branch: usize,
    pub root_world_id: u64,
    pub priority: u8,
    pub prompt_tokens: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

/// TinyLlama chat template. Generation always continues from an assistant header.
pub fn format_tinyllama_chat(messages: &[ChatTurn]) -> String {
    let mut out = String::new();
    for message in messages {
        let body = message.content.trim();
        if body.is_empty() {
            continue;
        }
        let tag = match message.role.trim().to_ascii_lowercase().as_str() {
            "system" => "system",
            "assistant" => "assistant",
            _ => "user",
        };
        out.push_str(&format!("<|{tag}|>\n{body}</s>\n"));
    }
    let ends_in_assistant = messages.last().is_some_and(|message| {
        message.role.trim().eq_ignore_ascii_case("assistant") && !message.content.trim().is_empty()
    });
    if !ends_in_assistant {
        out.push_str("<|assistant|>\n");
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlCommand {
    LaunchSwarm(LaunchSwarmReq),
    InspectSwarm(u64),
    CancelSwarm(u64),
    GetRuntimeStatus,
    Shutdown,
    /// One operator turn on the in-process PR #68 spine. The server streams
    /// `TurnDelta` lines and finishes with `TurnFinished`.
    StreamTurn {
        messages: Vec<ChatTurn>,
        max_tokens: usize,
        temperature: f32,
    },
    /// NEXT-PHASE-1 cut 1b: one goal through omega COMPOSITION-2 (J-Space
    /// alternatives, AEGIS verification, World commit, Cortex record) with a
    /// "model" Skill that proposes one file change for `workspace`. Handled on
    /// the socket connection (it runs inference). Nothing is written to
    /// `workspace` in this cut: the committed result is the proposal record.
    RunComposeTask {
        goal: String,
        workspace: String,
    },
    /// NEXT-PHASE-1 cut 2: append one operator record (`kind` = "constraint",
    /// "authorization" or "effect") to the composition's Cortex journal through
    /// its own writer. `links` name related Cortex ids (each must exist).
    ComposeNote {
        kind: String,
        text: String,
        links: Vec<u64>,
    },
    /// NEXT-PHASE-1 cut 2: recall every host record (constraints, authorizations,
    /// effects, repair records) plus the records named in `ids`, each digest
    /// re-checked; `prefix` (records 1..=prefix) gets a digest over their digests
    /// so a restart can be compared byte for byte.
    ComposeRecall {
        ids: Vec<u64>,
        prefix: Option<u64>,
    },
    /// NEXT-PHASE-1 cut 2: operator repair of a compose home that refuses to open
    /// (torn journal tail, or a journal behind its J-Space anchor). Closes this
    /// process's handle first; the cut is recorded in the journal.
    RecoverComposeHome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlEnvelope {
    pub protocol_version: u16,
    pub request_id: u64,
    pub operation_id: u128,
    pub operator_session: u64,
    pub command: ControlCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatusReport {
    pub active_sequences: usize,
    pub active_swarms: usize,
    pub active_worlds: usize,
    pub free_kv_blocks: usize,
    pub total_kv_blocks: usize,
    pub shared_kv_pages: usize,
    pub cow_faults: usize,
    pub gpu_utilization_pct: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlResponse {
    SwarmAccepted {
        swarm_id: u64,
        operation_id: u128,
    },
    SwarmCancelled {
        swarm_id: u64,
    },
    Status(RuntimeStatusReport),
    ShutdownAck,
    TurnDelta {
        text: String,
    },
    TurnFinished {
        text: String,
        total_tokens: usize,
    },
    /// Result record of `RunComposeTask`.
    ComposeTaskResult(Box<ComposeTaskReport>),
    /// Result of `ComposeNote`.
    ComposeNoted(ComposeNoteReport),
    /// Result of `ComposeRecall`.
    ComposeRecalled(Box<ComposeRecallReport>),
    /// Result of `RecoverComposeHome`.
    ComposeRecovered(Box<ComposeRecoverReport>),
    Error(String),
}

/// Actor managing operator sessions and enforcing idempotent command execution.
/// Processed operation IDs persist to disk so idempotency survives restarts.
pub struct RuntimeController {
    processed_operations: HashSet<u128>,
}

fn operations_state_path() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("AIEN_RUNTIME_STATE_DIR") {
        if !dir.trim().is_empty() {
            return std::path::PathBuf::from(dir.trim()).join("processed_operations.json");
        }
    }
    std::path::PathBuf::from("/tmp/aien-runtime-processed-ops.json")
}

impl Default for RuntimeController {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeController {
    pub fn new() -> Self {
        let mut processed_operations = HashSet::new();
        if let Ok(bytes) = std::fs::read(operations_state_path()) {
            if let Ok(ids) = serde_json::from_slice::<Vec<u128>>(&bytes) {
                processed_operations.extend(ids);
            }
        }
        Self {
            processed_operations,
        }
    }

    pub fn is_operation_processed(&self, op_id: u128) -> bool {
        self.processed_operations.contains(&op_id)
    }

    pub fn mark_operation_processed(&mut self, op_id: u128) {
        self.processed_operations.insert(op_id);
        let ids: Vec<u128> = self.processed_operations.iter().copied().collect();
        if let Ok(bytes) = serde_json::to_vec(&ids) {
            // Write then rename: a crash mid-write must not leave a truncated
            // file, which new() would silently discard, losing idempotency.
            let path = operations_state_path();
            let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
            if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, &path).is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idempotency_survives_restart() {
        let dir = std::env::temp_dir().join(format!("aien-ops-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("AIEN_RUNTIME_STATE_DIR", &dir);
        let op_id = 0xC0FFEEu128;

        {
            let mut first = RuntimeController::new();
            assert!(!first.is_operation_processed(op_id));
            first.mark_operation_processed(op_id);
            assert!(first.is_operation_processed(op_id));
        }

        // Simulate a process restart: a fresh controller reloads from disk.
        {
            let second = RuntimeController::new();
            assert!(
                second.is_operation_processed(op_id),
                "replayed operation ID must be rejected after restart"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn chat_template_opens_an_assistant_turn() {
        let prompt = format_tinyllama_chat(&[
            ChatTurn {
                role: "system".into(),
                content: "You are AIEN.".into(),
            },
            ChatTurn {
                role: "user".into(),
                content: "Status?".into(),
            },
        ]);
        assert!(prompt.starts_with("<|system|>\nYou are AIEN.</s>\n"));
        assert!(prompt.contains("<|user|>\nStatus?</s>\n"));
        assert!(prompt.ends_with("<|assistant|>\n"));
    }
}

/// The record of one `RunComposeTask` (omega rxc_host result, NEXT-PHASE-1).
/// Cortex ids name records in `<compose dir>/cortex.cx`; digests are hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposeTaskReport {
    /// Composition home (holds machine.id, cortex.cx, jspace).
    pub compose_dir: String,
    /// AienMachineId (32 bytes, hex) the home is bound to.
    pub machine_id: String,
    pub task: u64,
    /// RXC_OUT_*: 1 committed, 2 no winner, 3 not committed, 4 not durable, 5 record failed.
    pub outcome: i32,
    pub committed: bool,
    /// Staged J-Space branches forked (one per routed Skill alternative).
    pub branch_count: u32,
    pub branches_reclaimed: u32,
    /// AEGIS verdict: index of the winning alternative (None = no winner)
    /// and the pass mask (bit k = alternative k met the contract).
    pub winner: Option<u32>,
    pub aegis_pass_mask: u32,
    pub cx_goal: u64,
    pub cx_candidates: Vec<u64>,
    pub cx_evidence: u64,
    pub cx_promotion: u64,
    pub cx_admissions: Vec<u64>,
    pub winner_digest: String,
    pub record_digest: String,
    /// The committed proposal (the model Skill's output) and its sha256.
    pub proposal: Option<String>,
    pub proposal_sha256: Option<String>,
    /// The proposal parsed as one file change (AEGIS contract): relative path
    /// and the sha256 of the new content.
    pub proposal_path: Option<String>,
    pub proposal_content_sha256: Option<String>,
    /// "model" when the Skill ran inference, or the stub label.
    pub proposer: String,
}

/// The record `ComposeNote` appended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposeNoteReport {
    pub machine_id: String,
    pub id: u64,
    pub kind: String,
    /// Cortex record digest (hex), re-read after the append.
    pub digest: String,
    /// sha256 (hex) of the note text.
    pub text_sha256: String,
    pub links: Vec<u64>,
}

/// One Cortex record as recalled (digest re-checked by omega).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposeRecordView {
    pub id: u64,
    pub cls: u32,
    pub kind: u32,
    pub subject: u64,
    pub tag: u64,
    pub links: Vec<u64>,
    pub digest: String,
    pub verified: bool,
    /// "constraint", "authorization", "effect", "repair_tail" for host records.
    pub note: Option<String>,
    /// Host note text (sha256-checked), when the record is a note.
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposeRecallReport {
    pub compose_dir: String,
    pub machine_id: String,
    pub records_total: u64,
    /// Every host record (subject 0), oldest first.
    pub host: Vec<ComposeRecordView>,
    /// The records asked for by id (absent ids are listed in `missing`).
    pub cited: Vec<ComposeRecordView>,
    pub missing: Vec<u64>,
    /// sha256 over the digests of records 1..=prefix (hex), when asked.
    pub prefix: Option<u64>,
    pub prefix_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposeRecoverReport {
    pub compose_dir: String,
    pub repaired: bool,
    pub tail_torn: bool,
    /// CX_ERR_TORN (-8), RX_ERR_REPLAY (-21), 0 = nothing to repair.
    pub cause: i32,
    pub cut_lo: u64,
    pub cut_hi: u64,
    pub records_kept: u64,
    pub dropped_records: u64,
    pub anchor_records: u64,
    /// Cortex id of the repair record (0 = none).
    pub repair_record: u64,
    pub cut_bytes_kept: u64,
    pub cut_sha256: String,
    /// The composition opened after the repair (trial open).
    pub opens: bool,
    pub open_rc: i32,
    pub rolled_back: u32,
    pub recovered_completed: u32,
}

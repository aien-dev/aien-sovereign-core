//! The daemon's generation record: evidence that THIS daemon generated THIS
//! text from the model file it loaded (INTERPLANE provenance, interplane#76).
//!
//! Written by the daemon when a `StreamTurn` finishes, into the compose
//! ledger, as an `effect`-class host note carrying the marker field
//! [`GENERATION`]. EVIDENCE ONLY: no grant, intent, replay, commit or effect
//! decision reads it (`effects::Ledger` skips it: it has neither a `phase` nor a
//! `tool` field). `ComposeNote` refuses to write one (`check_reserved_note`),
//! so only the daemon can. It claims nothing about capability or authority.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Marker field (and refused `ComposeNote` kind) of the generation record.
pub const GENERATION: &str = "generation";

/// The model files the daemon loaded, hashed once at load time.
/// `model_sha256` is the sha256 of the single safetensors file's bytes
/// (streamed, 1 MiB chunks); `tokenizer_sha256` that of the tokenizer file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelIdentity {
    pub model_sha256: String,
    pub model_path: String,
    pub tokenizer_sha256: String,
    pub tokenizer_path: String,
}

/// What one finished turn contributes to its record.
pub struct TurnEvidence<'a> {
    pub prompt_ids: &'a [u32],
    pub output_ids: &'a [u32],
    /// The exact text returned in `TurnFinished`.
    pub text: &'a str,
    pub total_tokens: usize,
    pub finish_reason: &'a str,
    /// Caller-supplied envelope ids (self-asserted by the client, no authority).
    pub request_id: u64,
    pub operation_id: u128,
}

/// When this daemon started: unix milliseconds, captured once.
#[derive(Debug, Clone, Copy)]
pub struct DaemonStart(pub u128);

impl DaemonStart {
    pub fn now() -> Self {
        Self(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
        )
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    aien_omega_compose::hex(&Sha256::digest(bytes))
}

/// The record's JSON body (field list is documented in docs/DAEMON_GENERATION_RECORD.md).
pub fn build_record(id: &ModelIdentity, t: &TurnEvidence, start: DaemonStart) -> Value {
    let (pid, start_ticks) = crate::effects::self_executor();
    json!({
        GENERATION: 1,
        "v": 1,
        "model_sha256": id.model_sha256,
        "model_path": id.model_path,
        "tokenizer_sha256": id.tokenizer_sha256,
        "tokenizer_path": id.tokenizer_path,
        "prompt_ids_sha256": crate::spine::token_ids_sha256(t.prompt_ids),
        "output_token_ids_sha256": crate::spine::token_ids_sha256(t.output_ids),
        "output_text_sha256": hex_sha256(t.text.as_bytes()),
        "total_tokens": t.total_tokens,
        "output_tokens": t.output_ids.len(),
        "finish_reason": t.finish_reason,
        "daemon": {"pid": pid, "start_ticks": start_ticks, "started_unix_ms": start.0.to_string()},
        "request_id": t.request_id,
        "operation_id": t.operation_id.to_string(),
    })
}

/// Append `record` to the ledger; the id is returned only once it is written.
pub(crate) fn write(home: &mut crate::spine::ComposeHome, record: &Value) -> Result<u64, String> {
    crate::effects::append(home, aien_omega_compose::NoteKind::Effect, &[], record).map(|n| n.id)
}

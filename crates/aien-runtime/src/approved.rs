//! Proposer hook: an approved proposal enters the production compose path.
//!
//! The approval desk upstream (INTERPLANE `ComposeLedgerAuthority`,
//! interplane#77) hands over one file change it approved. `ProposerHook`
//! checks it, then drives it through the same compose run `RunComposeTask`
//! uses: the model Skill returns the approved text instead of calling the
//! model, AEGIS checks it with the bridge's verify callback, the winner
//! commits through the World on a J-Space branch, Cortex records it.
//!
//! The hook grants nothing. It never writes the workspace and never mints an
//! authorization: the effect still needs an `authorization` note, a
//! `ComposeEffectIntent` and an ack (crate::effects). Anything it cannot
//! verify, any duplicate and any run that does not commit is refused with a
//! named error (`PROPOSAL_REFUSED <Name>: ...`), never dropped.
//!
//! Hash semantics (both are reported, never conflated):
//! - `approved_proposal_sha256`: sha256 of the compact JSON object
//!   `{"content":..,"path":..}` with sorted keys, as interplane#77 computes it.
//! - `compose_proposal_sha256`: sha256 of the committed proposal text
//!   (`filename: <path>\n<content>`), the value `aien compose authorize` puts
//!   in the authorization grant and `ComposeEffectIntent` must repeat.
//!
//! Design note: docs/COMPOSE_PROPOSER_HOOK.md.
use crate::control::ComposeTaskReport;
use crate::spine::{check_file_proposal, ComposeBridge};
use aien_omega_compose::hex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// Prefix of every hook refusal.
pub const REFUSED: &str = "PROPOSAL_REFUSED";

/// One file change an upstream approval desk approved (the interplane#77
/// ledger fields plus the correlation ids).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovedProposal {
    /// INTERPLANE request id and trace id: carried into the report.
    pub request_id: String,
    pub trace_id: String,
    /// The approval the desk consumed (single use).
    pub approval_id: String,
    pub approver: String,
    /// Workspace-relative path and the complete new content.
    pub path: String,
    pub content: String,
    /// `approved_proposal_sha256(path, content)` as the desk computed it.
    pub approved_proposal_sha256: String,
    /// sha256 of `content`.
    pub content_sha256: String,
}

/// What a committed approved proposal hands to the effect ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovedComposeReport {
    pub request_id: String,
    pub trace_id: String,
    pub approval_id: String,
    pub approver: String,
    pub path: String,
    pub content_sha256: String,
    pub approved_proposal_sha256: String,
    /// sha256 of the committed proposal text: the authorization grant and the
    /// effect intent must name this value.
    pub compose_proposal_sha256: String,
    /// Links the authorization grant carries, as `aien compose authorize`
    /// does: [cx_promotion, cx_evidence].
    pub grant_links: Vec<u64>,
    /// The compose run (J-Space branch, AEGIS verdict, World commit records).
    pub task: ComposeTaskReport,
}

fn sha256_hex(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

fn refuse(name: &str, why: impl std::fmt::Display) -> String {
    format!("{REFUSED} {name}: {why}")
}

/// sha256 of the compact JSON `{"content":..,"path":..}`, keys sorted
/// (inserted in sorted order, so serde_json's map ordering does not matter).
pub fn approved_proposal_sha256(path: &str, content: &str) -> String {
    let mut m = Map::new();
    m.insert("content".into(), Value::String(content.to_string()));
    m.insert("path".into(), Value::String(path.to_string()));
    sha256_hex(Value::Object(m).to_string().as_bytes())
}

/// The proposal text in the compose template. Refused unless the template
/// parser reads back exactly this path and content (a content the parser
/// would normalise is refused, not silently changed).
pub fn proposal_text(path: &str, content: &str) -> Result<String, String> {
    let text = format!("filename: {path}\n{content}");
    let p = check_file_proposal(&text).map_err(|e| refuse("Unverified", e))?;
    if p.path != path || p.content != content {
        return Err(refuse(
            "Unverified",
            "content does not read back unchanged through the compose template \
             (it must end with one newline, not start with a blank line or a code fence)",
        ));
    }
    Ok(text)
}

/// Every check that needs no compose home. Ok = the proposal text.
pub fn verify(p: &ApprovedProposal) -> Result<String, String> {
    for (k, v) in [
        ("request_id", &p.request_id),
        ("trace_id", &p.trace_id),
        ("approval_id", &p.approval_id),
        ("approver", &p.approver),
    ] {
        if v.trim().is_empty() {
            return Err(refuse("Unverified", format!("empty {k}")));
        }
    }
    if sha256_hex(p.content.as_bytes()) != p.content_sha256 {
        return Err(refuse(
            "Unverified",
            "content_sha256 does not match content",
        ));
    }
    if approved_proposal_sha256(&p.path, &p.content) != p.approved_proposal_sha256 {
        return Err(refuse(
            "Unverified",
            "approved_proposal_sha256 does not match {content, path}",
        ));
    }
    proposal_text(&p.path, &p.content)
}

/// The hook over one compose bridge (the daemon's, or a test's).
pub struct ProposerHook {
    bridge: Arc<ComposeBridge>,
    /// Request and approval ids already submitted by this process.
    consumed: Mutex<HashSet<String>>,
}

impl ProposerHook {
    pub fn new(bridge: Arc<ComposeBridge>) -> Self {
        Self {
            bridge,
            consumed: Mutex::new(HashSet::new()),
        }
    }

    /// Blocking (runs the composition). Verify, refuse duplicates, run, and
    /// return the commit only if the World committed exactly this proposal.
    pub fn submit(
        &self,
        p: &ApprovedProposal,
        workspace: &str,
    ) -> Result<ApprovedComposeReport, String> {
        let text = verify(p)?;
        {
            let mut seen = self
                .consumed
                .lock()
                .map_err(|_| refuse("Internal", "replay set lock poisoned"))?;
            let keys = [
                format!("request:{}", p.request_id),
                format!("approval:{}", p.approval_id),
            ];
            if let Some(k) = keys.iter().find(|k| seen.contains(*k)) {
                return Err(refuse("Duplicate", format!("{k} was already submitted")));
            }
            // Consumed before the run: a failed run never frees the approval.
            for k in keys {
                seen.insert(k);
            }
        }
        let goal = format!("apply approved proposal (request {})", p.request_id);
        let r = self
            .bridge
            .run_approved_task(&goal, workspace, &text)
            .map_err(|e| refuse("ComposeError", e))?;
        if !r.committed {
            return Err(refuse(
                "NotCommitted",
                format!(
                    "outcome {} (AEGIS pass mask {:#x}); {}",
                    r.outcome,
                    r.aegis_pass_mask,
                    r.proposer_error.as_deref().unwrap_or("no proposer error")
                ),
            ));
        }
        let compose_sha = sha256_hex(text.as_bytes());
        if r.proposal.as_deref() != Some(text.as_str())
            || r.proposal_sha256.as_deref() != Some(compose_sha.as_str())
            || r.proposal_path.as_deref() != Some(p.path.as_str())
            || r.proposal_content_sha256.as_deref() != Some(p.content_sha256.as_str())
        {
            return Err(refuse(
                "Mismatch",
                "the committed proposal is not the approved one",
            ));
        }
        Ok(ApprovedComposeReport {
            request_id: p.request_id.clone(),
            trace_id: p.trace_id.clone(),
            approval_id: p.approval_id.clone(),
            approver: p.approver.clone(),
            path: p.path.clone(),
            content_sha256: p.content_sha256.clone(),
            approved_proposal_sha256: p.approved_proposal_sha256.clone(),
            compose_proposal_sha256: compose_sha,
            grant_links: vec![r.cx_promotion, r.cx_evidence],
            task: r,
        })
    }
}

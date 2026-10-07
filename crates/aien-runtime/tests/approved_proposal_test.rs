//! Proposer hook (crate::approved): an approved proposal goes through the
//! production compose run (J-Space branch, AEGIS verify, World commit) and
//! then the durable effect ledger; unverified and duplicate proposals are
//! refused with nothing appended. CPU only, no model, no socket: the bridge
//! is driven exactly as the daemon drives it. In a stub build only the
//! refusal paths run.
use aien_omega_compose::hex;
use aien_runtime::approved::{self, ApprovedProposal, ProposerHook};
use aien_runtime::control::{ComposeRecallReport, ControlResponse};
use aien_runtime::effects::{self, IntentRequest, Ledger};
use aien_runtime::spine::{ComposeBridge, ComposeProposer, APPROVED_PROPOSER_LABEL};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;

const PATH: &str = "NOTES.md";
const CONTENT: &str = "approved by the INTERPLANE desk\n";

/// The model proposer must never be called on the hook path.
fn model_never_called() -> ComposeProposer {
    Arc::new(|_: &str, _: std::time::Duration| panic!("the model proposer ran on the hook path"))
}

fn sha(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

/// Independent of approved::approved_proposal_sha256: the interplane#77
/// canonical form written out by hand.
fn interplane_sha(path: &str, content: &str) -> String {
    let s = format!(
        "{{\"content\":{},\"path\":{}}}",
        serde_json::to_string(content).unwrap(),
        serde_json::to_string(path).unwrap()
    );
    sha(s.as_bytes())
}

fn proposal(request: &str, approval: &str) -> ApprovedProposal {
    ApprovedProposal {
        request_id: request.into(),
        trace_id: format!("trace-{request}"),
        approval_id: approval.into(),
        approver: "interplane-host".into(),
        path: PATH.into(),
        content: CONTENT.into(),
        approved_proposal_sha256: interplane_sha(PATH, CONTENT),
        content_sha256: sha(CONTENT.as_bytes()),
    }
}

fn recalled(b: &ComposeBridge, ids: &[u64]) -> ComposeRecallReport {
    match b.recall(ids, None) {
        ControlResponse::ComposeRecalled(r) => *r,
        other => panic!("recall: {other:?}"),
    }
}

fn refused(r: Result<impl std::fmt::Debug, String>, name: &str) {
    match r {
        Err(e) => assert!(
            e.starts_with(&format!("PROPOSAL_REFUSED {name}:")),
            "want {name}, got {e}"
        ),
        Ok(v) => panic!("want PROPOSAL_REFUSED {name}, got {v:?}"),
    }
}

fn setup() -> (tempfile::TempDir, Arc<ComposeBridge>, ProposerHook, String) {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let b = Arc::new(ComposeBridge::new(
        tmp.path().join("compose"),
        model_never_called(),
        "test:model-never-called",
    ));
    let hook = ProposerHook::new(b.clone());
    let ws = ws.display().to_string();
    (tmp, b, hook, ws)
}

#[test]
fn approved_proposal_commits_then_effect_is_done() {
    let (_tmp, b, hook, ws) = setup();
    let out = hook.submit(&proposal("req-1", "appr-1"), &ws);
    if !aien_omega_compose::LINKED {
        refused(out, "ComposeError");
        return;
    }
    let r = out.unwrap();
    // Correlation and both hash semantics, side by side.
    assert_eq!(
        (r.request_id.as_str(), r.trace_id.as_str()),
        ("req-1", "trace-req-1")
    );
    assert_eq!(r.approved_proposal_sha256, interplane_sha(PATH, CONTENT));
    let text = format!("filename: {PATH}\n{CONTENT}");
    assert_eq!(r.compose_proposal_sha256, sha(text.as_bytes()));
    assert_ne!(r.approved_proposal_sha256, r.compose_proposal_sha256);
    // The compose run: one J-Space branch, AEGIS pass, World commit.
    let t = &r.task;
    assert!(t.committed, "{t:?}");
    assert_eq!((t.outcome, t.branch_count, t.winner), (1, 1, Some(0)));
    assert_eq!(t.aegis_pass_mask & 1, 1);
    assert_eq!(t.proposer, APPROVED_PROPOSER_LABEL);
    assert_eq!(t.proposal_attempts.len(), 1);
    assert_eq!(t.proposal_attempts[0].aegis.as_deref(), Some("pass"));
    assert!(t.cx_goal != 0 && t.cx_evidence != 0 && t.cx_promotion != 0);
    assert_eq!(r.grant_links, vec![t.cx_promotion, t.cx_evidence]);
    // The World commit record is in the Cortex journal and verifies.
    let c = recalled(&b, &[t.cx_promotion]);
    assert!(c.missing.is_empty(), "{c:?}");
    assert!(c.cited[0].verified, "{:?}", c.cited[0]);
    assert_eq!(c.cited[0].id, t.cx_promotion);
    // Nothing touched the workspace yet: the hook grants nothing.
    let target = Path::new(&ws).join(PATH);
    assert!(!target.exists());

    // Effect: grant (named as aien compose authorize names it), intent, write, ack.
    let tgt = target.display().to_string();
    let grant = json!({"proposal_sha256": r.compose_proposal_sha256, "path": r.path,
        "content_sha256": r.content_sha256, "approver": r.approver, "target": tgt,
        "prior_sha256": Value::Null});
    let a = match b.note("authorization", &grant.to_string(), &r.grant_links) {
        ControlResponse::ComposeNoted(n) => n.id,
        other => panic!("grant: {other:?}"),
    };
    let (pid, start) = effects::self_executor();
    let req = IntentRequest {
        authorization: a,
        proposal_sha256: r.compose_proposal_sha256.clone(),
        path: r.path.clone(),
        target: tgt.clone(),
        content_sha256: r.content_sha256.clone(),
        executor_pid: pid,
        executor_start: start,
    };
    let i = match effects::open_intent(&b, &req) {
        ControlResponse::ComposeNoted(n) => n.id,
        other => panic!("intent: {other:?}"),
    };
    std::fs::write(&target, CONTENT).unwrap();
    let k = match effects::ack(&b, i, &json!({"written_sha256": r.content_sha256})) {
        ControlResponse::ComposeNoted(n) => n.id,
        other => panic!("ack: {other:?}"),
    };
    let rec = recalled(&b, &[k]);
    let v: Value = serde_json::from_str(rec.cited[0].text.as_deref().unwrap()).unwrap();
    assert_eq!(v["state"], "DONE", "{v}");
    assert_eq!(std::fs::read(&target).unwrap(), CONTENT.as_bytes());
}

#[test]
fn unverified_proposal_is_refused_with_zero_effects() {
    let (_tmp, b, hook, ws) = setup();
    println!(
        "approved_proposal_test: compose LINKED={} (zero-journal assertions {})",
        aien_omega_compose::LINKED,
        if aien_omega_compose::LINKED {
            "run"
        } else {
            "skipped: stub build"
        }
    );
    let before = aien_omega_compose::LINKED.then(|| recalled(&b, &[]).records_total);
    let mut cases: Vec<(&str, ApprovedProposal)> = Vec::new();
    let mut p = proposal("u1", "a1");
    p.content_sha256 = sha(b"other bytes\n");
    cases.push(("content sha mismatch", p));
    let mut p = proposal("u2", "a2");
    // The compose text sha in the approved slot: the other hash semantics.
    p.approved_proposal_sha256 = sha(format!("filename: {PATH}\n{CONTENT}").as_bytes());
    cases.push(("text sha in the approved slot", p));
    let mut p = proposal("u3", "a3");
    p.path = "../escape.md".into();
    p.approved_proposal_sha256 = interplane_sha(&p.path, CONTENT);
    cases.push(("path outside the workspace", p));
    let mut p = proposal("u4", "a4");
    p.content = "no trailing newline".into();
    p.content_sha256 = sha(p.content.as_bytes());
    p.approved_proposal_sha256 = interplane_sha(PATH, &p.content);
    cases.push(("content the template would change", p));
    let mut p = proposal("u5", "");
    p.approval_id = " ".into();
    cases.push(("no approval id", p));
    let mut p = proposal("", "a6");
    p.request_id = String::new();
    cases.push(("no request id", p));
    for (what, p) in &cases {
        let r = hook.submit(p, &ws);
        assert!(r.is_err(), "{what}: {r:?}");
        refused(r, "Unverified");
    }
    assert!(!Path::new(&ws).join(PATH).exists());
    assert!(!Path::new(&ws).join("../escape.md").exists());
    if let Some(n) = before {
        let after = recalled(&b, &[]);
        assert_eq!(
            after.records_total, n,
            "a refused proposal appended records"
        );
        let l = Ledger::from_records(&after.host).unwrap();
        assert!(l.grants.is_empty() && l.intents.is_empty());
    }
}

#[test]
fn duplicate_proposal_is_refused() {
    let (_tmp, b, hook, ws) = setup();
    let first = hook.submit(&proposal("req-d", "appr-d"), &ws);
    if !aien_omega_compose::LINKED {
        refused(first, "ComposeError");
        // Consumed before the run: the replay is refused even in a stub build.
        refused(hook.submit(&proposal("req-d", "appr-d"), &ws), "Duplicate");
        return;
    }
    assert!(first.unwrap().task.committed);
    let n = recalled(&b, &[]).records_total;
    refused(hook.submit(&proposal("req-d", "appr-d"), &ws), "Duplicate");
    refused(
        hook.submit(&proposal("req-other", "appr-d"), &ws),
        "Duplicate",
    );
    refused(
        hook.submit(&proposal("req-d", "appr-other"), &ws),
        "Duplicate",
    );
    assert_eq!(
        recalled(&b, &[]).records_total,
        n,
        "a duplicate appended records"
    );
    // The same change under a fresh request and approval is a new proposal.
    assert!(
        hook.submit(&proposal("req-e", "appr-e"), &ws)
            .unwrap()
            .task
            .committed
    );
}

#[test]
fn approved_hash_is_the_interplane_form() {
    assert_eq!(
        approved::approved_proposal_sha256("a/b.md", "x \"q\"\n\u{1}é\n"),
        interplane_sha("a/b.md", "x \"q\"\n\u{1}é\n")
    );
}

/// The path already exists: RunComposeTask would treat it as an edit target
/// and merge the reply into the seed (v7 T5). An approved whole-file
/// proposal must commit byte-exact, with no seed line merged in.
#[test]
fn approved_proposal_over_existing_file_commits_byte_exact() {
    let (_tmp, _b, hook, ws) = setup();
    let seed = "# Notes\nseed line one\nseed line two\n";
    std::fs::write(Path::new(&ws).join(PATH), seed).unwrap();
    // Shares "# Notes" with the seed, so an edit merge would keep both seed lines.
    let content = "# Notes\napproved replacement line\n";
    let mut p = proposal("req-x", "appr-x");
    p.content = content.into();
    p.content_sha256 = sha(content.as_bytes());
    p.approved_proposal_sha256 = interplane_sha(PATH, content);
    let out = hook.submit(&p, &ws);
    if !aien_omega_compose::LINKED {
        refused(out, "ComposeError");
        return;
    }
    let r = out.unwrap();
    let t = &r.task;
    assert!(t.committed, "{t:?}");
    let text = format!("filename: {PATH}\n{content}");
    assert_eq!(t.proposal.as_deref(), Some(text.as_str()));
    assert!(!t.proposal.as_deref().unwrap().contains("seed line"));
    assert_eq!(
        t.proposal_content_sha256.as_deref(),
        Some(p.content_sha256.as_str())
    );
    assert_eq!(r.compose_proposal_sha256, sha(text.as_bytes()));
    // The hook wrote nothing: the seed is untouched.
    assert_eq!(
        std::fs::read_to_string(Path::new(&ws).join(PATH)).unwrap(),
        seed
    );
}

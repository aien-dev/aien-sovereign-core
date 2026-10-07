//! A document task whose first attempt fails a stated requirement and whose
//! second attempt passes: both attempts happen inside the ONE per-task document
//! deadline (a short injected budget), and the bytes saved are the bytes approved.
//! Own test binary: it sets a process-global environment variable.
use aien_runtime::control::ControlResponse;
use aien_runtime::requirements::extract;
use aien_runtime::spine::{
    check_file_proposal, compose_attempt_budget, compose_budgets_from_env, propose_task_checked,
    ComposeBridge, ComposeProposer, Generation, ProposalKind, COMPOSE_DOC_BUDGET_ENV,
    COMPOSE_MAX_ATTEMPTS,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn doc(n: usize) -> String {
    let mut s = String::from("filename: DOC.md\n");
    for i in 0..n {
        s.push_str(&format!("line {i}\n"));
    }
    s
}

fn reply(text: String) -> Result<Generation, String> {
    Ok(Generation {
        text,
        tokens: 5,
        finish_reason: Some("eos".into()),
        ..Default::default()
    })
}

#[test]
fn requirement_retry_stays_inside_the_single_document_deadline_and_saves_approved_bytes() {
    std::env::set_var(COMPOSE_DOC_BUDGET_ENV, "6000");
    let budget = compose_budgets_from_env().unwrap().doc;
    assert_eq!(budget, Duration::from_millis(6000));
    let attempt_budget = compose_attempt_budget(ProposalKind::Document, budget);
    let reqs = extract("write DOC.md in at least 20 lines");
    assert_eq!(reqs.len(), 1);

    // Part 1 (any build): the shared deadline, observed from the proposer.
    let limits: Mutex<Vec<Duration>> = Mutex::default();
    let t0 = Instant::now();
    let proposer = |_: &str, limit: Duration| {
        let mut l = limits.lock().unwrap();
        l.push(limit);
        std::thread::sleep(Duration::from_millis(300));
        // First attempt: 5 lines (unmet). Second: 25 lines.
        reply(doc(if l.len() == 1 { 5 } else { 25 }))
    };
    let (out, attempts) = propose_task_checked(
        &proposer,
        "base",
        None,
        &reqs,
        budget,
        attempt_budget,
        COMPOSE_MAX_ATTEMPTS,
    );
    assert!(t0.elapsed() < budget, "both attempts inside one deadline");
    let limits = limits.lock().unwrap().clone();
    assert_eq!(limits.len(), 2);
    assert!(limits[0] <= budget, "{limits:?}");
    assert!(
        limits[1] < limits[0],
        "retry gets what is left, not a fresh budget: {limits:?}"
    );
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].outcome, "refused");
    assert_eq!(attempts[0].unmet_requirements.len(), 1);
    assert_eq!(attempts[1].outcome, "parsed");
    assert!(out.is_ok());

    // Part 2 (linked build): the same through the real run; saved == approved.
    let tmp = tempfile::tempdir().unwrap();
    let ws = std::fs::canonicalize({
        std::fs::create_dir_all(tmp.path().join("ws")).unwrap();
        tmp.path().join("ws")
    })
    .unwrap();
    let calls: Arc<Mutex<u32>> = Arc::default();
    let c = calls.clone();
    let doc_proposer: ComposeProposer = Arc::new(move |_: &str, _: Duration| {
        let mut n = c.lock().unwrap();
        *n += 1;
        std::thread::sleep(Duration::from_millis(300));
        reply(doc(if *n == 1 { 5 } else { 25 }))
    });
    let unused: ComposeProposer = Arc::new(|_: &str, _: Duration| Err("edit path unused".into()));
    let bridge = ComposeBridge::new(tmp.path().join("compose"), unused, "test:deadline")
        .with_doc_proposer(doc_proposer);
    let t0 = Instant::now();
    let resp = bridge.run_task("write DOC.md in at least 20 lines", ws.to_str().unwrap());
    if !aien_omega_compose::LINKED {
        assert!(matches!(resp, ControlResponse::Error(_)));
        return;
    }
    let ControlResponse::ComposeTaskResult(r) = resp else {
        panic!("{resp:?}")
    };
    assert!(t0.elapsed() < budget);
    assert!(r.committed, "{r:?}");
    assert_eq!(*calls.lock().unwrap(), 2);
    assert_eq!(r.proposal_attempts.len(), 2);
    assert_eq!(r.proposal_attempts[0].unmet_requirements.len(), 1);
    assert_eq!(r.proposal_attempts[1].aegis.as_deref(), Some("pass"));
    let saved = check_file_proposal(&r.proposal.clone().unwrap()).unwrap();
    assert_eq!(
        r.proposal_content_sha256.clone().unwrap(),
        aien_omega_compose::hex(&<sha2::Sha256 as sha2::Digest>::digest(
            saved.content.as_bytes()
        ))
    );
    assert_eq!(
        saved.content,
        doc(25).strip_prefix("filename: DOC.md\n").unwrap()
    );
}

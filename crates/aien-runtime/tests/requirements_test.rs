//! Requirement validation for composed documents (crate::requirements).
use aien_runtime::approved::{approved_proposal_sha256, proposal_text};
use aien_runtime::control::ControlResponse;
use aien_runtime::effects;
use aien_runtime::requirements::{extract, refusal_reason, unmet, ItemKind, Requirement};
use aien_runtime::spine::{
    check_file_proposal, propose_task_checked, ComposeBridge, ComposeProposer, Generation,
    COMPOSE_MAX_ATTEMPTS,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn doc(n: usize) -> String {
    let mut s = String::from("filename: DOC.md\n");
    for i in 0..n {
        s.push_str(&format!("line {i}\n"));
    }
    s
}

fn gen(text: String) -> Result<Generation, String> {
    Ok(Generation {
        text,
        tokens: 5,
        finish_reason: Some("eos".into()),
        ..Default::default()
    })
}

#[test]
fn extracts_each_supported_pattern() {
    use ItemKind::*;
    use Requirement::*;
    assert_eq!(
        extract("Write a note in at least 20 lines."),
        [MinLines(20)]
    );
    assert_eq!(extract("keep it to AT MOST 8 lines"), [MaxLines(8)]);
    assert_eq!(extract("give at least 5 items"), [MinItems(Items, 5)]);
    assert_eq!(extract("give at least 1 item"), [MinItems(Items, 1)]);
    assert_eq!(
        extract("list at least 3 steps, please"),
        [MinItems(Steps, 3)]
    );
    assert_eq!(extract("with at least 4 sections"), [MinItems(Sections, 4)]);
    assert_eq!(
        extract("ask at least 6 questions."),
        [MinItems(Questions, 6)]
    );
    assert_eq!(
        extract("include the phrase \"Hello World\" and contain the phrase \"x\""),
        [RequiredPhrases(vec!["Hello World".into(), "x".into()])]
    );
    assert_eq!(
        extract("at least 20 lines and at most 40 lines"),
        [MinLines(20), MaxLines(40)]
    );
}

#[test]
fn unsupported_phrasing_is_not_extracted() {
    for g in [
        "write a long document",
        "at least twenty lines",
        "no fewer than 20 lines",
        "more than 20 lines",
        "at least 20 long lines",
        "at least 20 words",
        "at least 20 paragraphs",
        "at most 5 items",
        "at least 0 lines",
        "at least -3 lines",
        "at least 2.5 lines",
        "at least 99999999999999999999999 lines",
        "include the phrase hello",
        "include the phrase \"\"",
        "mention hello in the text",
        "exactly 20 lines",
    ] {
        assert_eq!(extract(g), vec![], "{g}");
    }
}

#[test]
fn checks_pass_and_fail_with_exact_counts() {
    use ItemKind::*;
    use Requirement::*;
    let thirteen: String = (0..13).map(|i| format!("l{i}\n\n")).collect();
    assert_eq!(non_empty_blank_safe(&thirteen), 13);
    assert!(MinLines(13).check(&thirteen).is_none());
    assert_eq!(
        MinLines(20).check(&thirteen).unwrap(),
        "at least 20 non-empty lines, found 13"
    );
    assert!(MaxLines(13).check(&thirteen).is_none());
    assert!(MaxLines(12).check(&thirteen).is_some());
    let md = "# One\n## Two\n#NoSpace\n####### seven\n- a\n  * b\n+ c\n1. d\n2) e\n3.f\nWhy?\nreally ?\n";
    assert!(MinItems(Sections, 2).check(md).is_none());
    assert!(MinItems(Sections, 3).check(md).is_some());
    assert!(MinItems(Items, 5).check(md).is_none());
    assert!(MinItems(Items, 6).check(md).is_some());
    assert!(MinItems(Steps, 2).check(md).is_none());
    assert!(MinItems(Steps, 3).check(md).is_some());
    assert!(MinItems(Questions, 2).check(md).is_none());
    assert!(MinItems(Questions, 3).check(md).is_some());
    let p = RequiredPhrases(vec!["hello world".into(), "Bye".into()]);
    assert_eq!(
        p.check("HeLLo WoRLD only").unwrap(),
        "the phrase \"hello world\", \"Bye\", missing \"Bye\""
    );
    assert!(p.check("Hello World and BYE").is_none());
    assert_eq!(unmet(&[], "x"), Vec::<String>::new());
    assert_eq!(refusal_reason(&[], "x"), None);
}

fn non_empty_blank_safe(s: &str) -> usize {
    s.lines().filter(|l| !l.trim().is_empty()).count()
}

#[test]
fn retry_names_the_unmet_requirement_and_second_attempt_passes() {
    let prompts: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let replies = Mutex::new(vec![doc(25), doc(13)]);
    let proposer = |p: &str, _: Duration| {
        prompts.lock().unwrap().push(p.to_string());
        gen(replies.lock().unwrap().pop().unwrap())
    };
    let reqs = extract("write it in at least 20 lines");
    let (out, a) = propose_task_checked(
        &proposer,
        "base",
        None,
        &reqs,
        Duration::from_secs(5),
        Duration::from_millis(1),
        COMPOSE_MAX_ATTEMPTS,
    );
    assert_eq!(out.unwrap(), doc(25));
    assert_eq!(a.len(), 2);
    assert_eq!(a[0].outcome, "refused");
    assert_eq!(
        a[0].reason.as_deref(),
        Some("unmet requirement: at least 20 non-empty lines, found 13")
    );
    assert_eq!(
        a[0].unmet_requirements,
        ["at least 20 non-empty lines, found 13"]
    );
    assert_eq!(a[1].outcome, "parsed");
    assert!(a[1].unmet_requirements.is_empty());
    let p = prompts.lock().unwrap();
    assert!(!p[0].contains("unmet requirement"));
    assert!(
        p[1].contains("refused (unmet requirement: at least 20 non-empty lines, found 13)"),
        "{}",
        p[1]
    );
}

#[test]
fn exhaustion_returns_no_proposal() {
    let proposer = |_: &str, _: Duration| gen(doc(13));
    let reqs = extract("in at least 20 lines");
    let (out, a) = propose_task_checked(
        &proposer,
        "base",
        None,
        &reqs,
        Duration::from_secs(5),
        Duration::from_millis(1),
        COMPOSE_MAX_ATTEMPTS,
    );
    let why = out.unwrap_err();
    assert!(
        why.contains("at least 20 non-empty lines, found 13"),
        "{why}"
    );
    assert_eq!(a.len(), COMPOSE_MAX_ATTEMPTS as usize);
    assert!(a
        .iter()
        .all(|x| x.outcome == "refused" && x.aegis.is_none()));
    assert!(a.iter().all(|x| x.unmet_requirements.len() == 1));
}

#[test]
fn deadline_is_shared_across_attempts() {
    let seen: Mutex<Vec<Duration>> = Mutex::new(Vec::new());
    let proposer = |_: &str, limit: Duration| {
        seen.lock().unwrap().push(limit);
        std::thread::sleep(Duration::from_millis(150));
        gen(doc(1))
    };
    let reqs = extract("at least 20 lines");
    let budget = Duration::from_millis(500);
    let t0 = std::time::Instant::now();
    let (out, a) = propose_task_checked(
        &proposer,
        "base",
        None,
        &reqs,
        budget,
        Duration::from_millis(100),
        10,
    );
    assert!(out.is_err());
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), a.len());
    assert!(a.len() >= 2 && a.len() < 10, "attempts {}", a.len());
    // Each attempt's limit is what is left of ONE budget: it only shrinks,
    // by at least the time the earlier attempts took.
    for k in 1..seen.len() {
        assert!(
            seen[k] + Duration::from_millis(140 * k as u64) <= seen[0],
            "attempt {} limit {:?} vs first {:?}",
            k + 1,
            seen[k],
            seen[0]
        );
    }
    assert!(seen[0] <= budget);
    assert!(t0.elapsed() < budget + Duration::from_millis(200));
    assert!(out.unwrap_err().contains("per-attempt budget"));
}

#[test]
fn validated_document_is_the_one_approval_binds() {
    let reqs = extract("at least 4 lines");
    let proposer = |_: &str, _: Duration| gen("filename: DOC.md\na\nb\nc\nd\n".into());
    let (out, _) = propose_task_checked(
        &proposer,
        "base",
        None,
        &reqs,
        Duration::from_secs(5),
        Duration::from_millis(1),
        3,
    );
    let text = out.unwrap();
    let saved = check_file_proposal(&text).unwrap();
    // The content that passed is the content that would be written ...
    assert!(refusal_reason(&reqs, &saved.content).is_none());
    // ... and it reads back byte for byte through the approval template, so
    // the digest an approval binds is the digest of the saved bytes.
    assert_eq!(proposal_text(&saved.path, &saved.content).unwrap(), text);
    let bound = approved_proposal_sha256(&saved.path, &saved.content);
    let again = check_file_proposal(&proposal_text(&saved.path, &saved.content).unwrap()).unwrap();
    assert_eq!(approved_proposal_sha256(&again.path, &again.content), bound);
}

#[test]
fn edit_mode_validates_the_merged_file_not_the_reply() {
    // The prior file has 3 lines; the reply adds 2: the saved file has 5.
    let prior = "# T\na\nb\n";
    let proposer = |_: &str, _: Duration| gen("filename: N.md\n# T\na\nnew1\nnew2\nb\n".into());
    let ok = |n: usize| {
        propose_task_checked(
            &proposer,
            "base",
            Some(("N.md", prior)),
            &[Requirement::MinLines(n)],
            Duration::from_secs(5),
            Duration::from_millis(1),
            1,
        )
    };
    let (out, _) = ok(5);
    assert_eq!(
        check_file_proposal(&out.unwrap()).unwrap().content,
        "# T\na\nnew1\nnew2\nb\n"
    );
    let (out, a) = ok(6);
    assert!(out.is_err());
    assert_eq!(
        a[0].unmet_requirements,
        ["at least 6 non-empty lines, found 5"]
    );
}

#[test]
fn unmet_requirement_leaves_nothing_committed_or_written() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let proposer: ComposeProposer = Arc::new(|_: &str, _: Duration| gen(doc(13)));
    let bridge = ComposeBridge::new(tmp.path().join("compose"), proposer, "test:short");
    let resp = bridge.run_task("write DOC.md in at least 20 lines", ws.to_str().unwrap());
    if !aien_omega_compose::LINKED {
        assert!(matches!(resp, ControlResponse::Error(_)));
        return;
    }
    let ControlResponse::ComposeTaskResult(r) = resp else {
        panic!("{resp:?}")
    };
    assert!(!r.committed, "{r:?}");
    assert!(r.proposal.is_none());
    assert_eq!(r.requirements_recognized, ["at least 20 non-empty lines"]);
    assert_eq!(r.proposal_attempts.len(), COMPOSE_MAX_ATTEMPTS as usize);
    assert!(r.proposer_error.unwrap().contains("found 13"));
    assert_eq!(std::fs::read_dir(&ws).unwrap().count(), 0);
}

// ---- linked proofs (real omega compose library; skipped in the CPU stub) ----

fn sha256_hex(b: &[u8]) -> String {
    aien_omega_compose::hex(&<sha2::Sha256 as sha2::Digest>::digest(b))
}

fn recalled(b: &ComposeBridge) -> aien_runtime::control::ComposeRecallReport {
    match b.recall(&[], None) {
        ControlResponse::ComposeRecalled(r) => *r,
        other => panic!("recall: {other:?}"),
    }
}

fn dir_names(d: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// Every attempt of a real compose run fails the requirement: nothing is
/// proposed, approved, written, committed or recorded as an effect.
#[test]
fn linked_exhaustion_leaves_ledger_state_and_files_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let home = tmp.path().join("compose");
    let calls = Arc::new(Mutex::new(0usize));
    let c2 = calls.clone();
    let proposer: ComposeProposer = Arc::new(move |_: &str, _: Duration| {
        *c2.lock().unwrap() += 1;
        gen(doc(13))
    });
    let bridge = ComposeBridge::new(home.clone(), proposer, "test:short");
    let resp = bridge.run_task("write DOC.md in at least 20 lines", ws.to_str().unwrap());
    if !aien_omega_compose::LINKED {
        assert!(matches!(resp, ControlResponse::Error(_)));
        return;
    }
    let ControlResponse::ComposeTaskResult(r) = resp else {
        panic!("{resp:?}")
    };
    // The real model Skill ran COMPOSE_MAX_ATTEMPTS times, each refused.
    assert_eq!(*calls.lock().unwrap(), COMPOSE_MAX_ATTEMPTS as usize);
    assert!(r
        .proposal_attempts
        .iter()
        .all(|a| a.outcome == "refused" && a.aegis.is_none()));
    // No committed proposal, no winner, no AEGIS pass, no promotion record.
    assert!(!r.committed, "{r:?}");
    assert_eq!(r.outcome, 2, "RXC_OUT_NO_WINNER: {r:?}");
    assert!(r.proposal.is_none() && r.proposal_sha256.is_none());
    assert!(r.proposal_path.is_none() && r.proposal_content_sha256.is_none());
    assert_eq!((r.winner, r.aegis_pass_mask), (None, 0), "{r:?}");
    assert_eq!(r.cx_promotion, 0, "{r:?}");
    // No approval request, no effect intent: the host ledger holds no
    // authorization or effect note, and rebuilds to an empty ledger.
    let rec = recalled(&bridge);
    assert!(
        rec.host
            .iter()
            .all(|h| !matches!(h.note.as_deref(), Some("authorization") | Some("effect"))),
        "{:?}",
        rec.host
    );
    let ledger = effects::Ledger::from_records(&rec.host).unwrap();
    assert!(ledger.grants.is_empty() && ledger.intents.is_empty());
    assert!(ledger.approved_by_claim.is_empty() && ledger.spent.is_empty());
    // No file was written and the destination is absent.
    assert!(!ws.join("DOC.md").exists());
    assert_eq!(dir_names(&ws), Vec::<String>::new());
    // The compose home holds only composition files.
    for n in dir_names(&home) {
        assert!(
            n == "machine.id" || n == "cortex.cx" || n.starts_with("jspace"),
            "{n}"
        );
    }
    // Compose state is unchanged by the failed task: a second identical task
    // adds no promotion and the committed World has no new document.
    let before = recalled(&bridge).records_total;
    let again = bridge.run_task("write DOC.md in at least 20 lines", ws.to_str().unwrap());
    let ControlResponse::ComposeTaskResult(r2) = again else {
        panic!("{again:?}")
    };
    assert!(!r2.committed && r2.cx_promotion == 0, "{r2:?}");
    assert!(!ws.join("DOC.md").exists());
    let after = recalled(&bridge);
    assert!(after.records_total >= before);
    assert!(after
        .host
        .iter()
        .all(|h| !matches!(h.note.as_deref(), Some("authorization") | Some("effect"))));
}

/// A document that passes validation commits through the real compose run,
/// is approved by an authorization bound to its digest, and is saved with
/// sha256(disk) == the approved content sha256.
#[test]
fn linked_passing_document_is_approved_and_saved_byte_for_byte() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let ws = std::fs::canonicalize(&ws).unwrap();
    let proposer: ComposeProposer = Arc::new(|_: &str, _: Duration| gen(doc(25)));
    let bridge = ComposeBridge::new(tmp.path().join("compose"), proposer, "test:long");
    let resp = bridge.run_task("write DOC.md in at least 20 lines", ws.to_str().unwrap());
    if !aien_omega_compose::LINKED {
        assert!(matches!(resp, ControlResponse::Error(_)));
        return;
    }
    let ControlResponse::ComposeTaskResult(r) = resp else {
        panic!("{resp:?}")
    };
    assert!(r.committed, "{r:?}");
    assert_eq!(r.proposal_attempts.len(), 1);
    assert_eq!(r.proposal_attempts[0].aegis.as_deref(), Some("pass"));
    let proposal = r.proposal.clone().unwrap();
    let saved = check_file_proposal(&proposal).unwrap();
    assert!(refusal_reason(&extract("at least 20 lines"), &saved.content).is_none());
    let content_sha = r.proposal_content_sha256.clone().unwrap();
    assert_eq!(content_sha, sha256_hex(saved.content.as_bytes()));
    assert!(!ws.join("DOC.md").exists(), "composing writes nothing");
    // Approval (the authorization note) binds the digest; the effect writes.
    let target = ws.join(&saved.path);
    let grant = serde_json::json!({
        "proposal_sha256": r.proposal_sha256.clone().unwrap(), "path": saved.path,
        "content_sha256": content_sha, "approver": "drake",
        "target": target.display().to_string(), "prior_sha256": null,
        "workspace": ws.display().to_string()});
    let a = match bridge.note("authorization", &grant.to_string(), &[]) {
        ControlResponse::ComposeNoted(n) => n.id,
        other => panic!("authorize: {other:?}"),
    };
    let (pid, start) = effects::self_executor();
    let i = match effects::open_intent(
        &bridge,
        &effects::IntentRequest {
            authorization: a,
            proposal_sha256: r.proposal_sha256.clone().unwrap(),
            path: saved.path.clone(),
            target: target.display().to_string(),
            content_sha256: content_sha.clone(),
            executor_pid: pid,
            executor_start: start,
        },
    ) {
        ControlResponse::ComposeNoted(n) => n.id,
        other => panic!("intent: {other:?}"),
    };
    std::fs::write(&target, &saved.content).unwrap();
    assert!(matches!(
        effects::ack(
            &bridge,
            i,
            &serde_json::json!({"written_sha256": content_sha})
        ),
        ControlResponse::ComposeNoted(_)
    ));
    assert_eq!(sha256_hex(&std::fs::read(&target).unwrap()), content_sha);
    let ledger = effects::Ledger::from_records(&recalled(&bridge).host).unwrap();
    assert_eq!(ledger.intents.len(), 1);
}

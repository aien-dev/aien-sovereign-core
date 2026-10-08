//! NEXT-PHASE-1 cut 1b + 2: RunComposeTask, ComposeNote, ComposeRecall and
//! RecoverComposeHome through omega COMPOSITION-2.
//!
//! With librx_compose.a linked (aien-omega-compose built with
//! AIEN_OMEGA_COMPOSE_DIR, see its README) these run the real composition;
//! in a stub build they check that the bridge reports the missing library.
use aien_runtime::control::{ComposeRecallReport, ComposeTaskReport, ControlResponse};
use aien_runtime::spine::{
    check_file_proposal, compose_attempt_budget, parse_compose_budget,
    parse_compose_doc_max_tokens, parse_file_proposal, propose_with_retries, wall_clock_reason,
    ComposeBridge, ComposeBudgets, ComposeProposer, Generation, ProposalKind,
    COMPOSE_ATTEMPT_BUDGET, COMPOSE_DOC_BUDGET, COMPOSE_MAX_ATTEMPTS, COMPOSE_SKILL_BUDGET,
    COMPOSE_WAIT_MARGIN,
};
use std::sync::Arc;

fn proposer() -> ComposeProposer {
    Arc::new(|prompt: &str, _limit: std::time::Duration| {
        assert!(prompt.contains("Authorized workspace:"));
        assert!(prompt.contains("filename: <relative path>"));
        Ok(Generation {
            text: "filename: NOTES.md\nconstraint: keep main green\n".to_string(),
            tokens: 12,
            finish_reason: None,
            ..Default::default()
        })
    })
}

fn report(r: ControlResponse) -> ComposeTaskReport {
    match r {
        ControlResponse::ComposeTaskResult(r) => *r,
        other => panic!("expected ComposeTaskResult, got {other:?}"),
    }
}

fn recalled(r: ControlResponse) -> ComposeRecallReport {
    match r {
        ControlResponse::ComposeRecalled(r) => *r,
        other => panic!("expected ComposeRecalled, got {other:?}"),
    }
}

fn snapshot(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read(e.path()).unwrap_or_default(),
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn compose_task_commits_and_survives_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("compose");
    let ws = tmp.path().join("workspace");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("README.md"), b"project\n").unwrap();
    let ws_before = snapshot(&ws);

    let bridge = ComposeBridge::new(home.clone(), proposer(), "test:fixed-proposer");
    let resp = bridge.run_task(
        "remember the constraints and propose one change",
        ws.to_str().unwrap(),
    );
    if !aien_omega_compose::LINKED {
        match resp {
            ControlResponse::Error(e) => assert!(e.contains("not linked"), "{e}"),
            other => panic!("stub build must refuse, got {other:?}"),
        }
        return;
    }
    let r1 = report(resp);
    assert!(r1.committed, "{r1:?}");
    assert_eq!(r1.outcome, 1);
    assert_eq!(r1.branch_count, 1);
    assert_eq!(r1.winner, Some(0));
    assert_eq!(r1.aegis_pass_mask & 1, 1);
    assert!(
        r1.cx_goal != 0
            && r1.cx_evidence != 0
            && r1.cx_promotion != 0
            && !r1.cx_candidates.is_empty()
    );
    assert_eq!(
        r1.proposal.as_deref(),
        Some("filename: NOTES.md\nconstraint: keep main green\n")
    );
    assert_eq!(r1.proposal_attempts.len(), 1, "{:?}", r1.proposal_attempts);
    assert_eq!(r1.proposal_attempts[0].outcome, "parsed");
    assert_eq!(r1.proposal_attempts[0].aegis.as_deref(), Some("pass"));
    assert_eq!(r1.proposal_attempts[0].tokens, 12);
    assert_eq!(r1.machine_id.len(), 64);
    assert_eq!(r1.proposal_path.as_deref(), Some("NOTES.md"));
    assert_eq!(
        r1.proposal_content_sha256.as_deref(),
        Some(aien_omega_compose::hex(
            &<sha2::Sha256 as sha2::Digest>::digest(b"constraint: keep main green\n")
        ))
        .as_deref()
    );
    let c = match bridge.note("constraint", "keep main green", &[]) {
        ControlResponse::ComposeNoted(n) => n,
        other => panic!("note: {other:?}"),
    };
    assert!(c.id > r1.cx_promotion);
    assert!(matches!(
        bridge.note("effect", "x", &[1 << 40]),
        ControlResponse::Error(_)
    ));
    assert!(matches!(
        bridge.note("rumour", "x", &[]),
        ControlResponse::Error(_)
    ));
    let before = recalled(bridge.recall(&[r1.cx_promotion], Some(c.id)));
    let cited = [
        r1.cx_goal,
        r1.cx_candidates[0],
        r1.cx_evidence,
        r1.cx_promotion,
    ];
    let digests: Vec<String> = cited
        .iter()
        .map(|&id| bridge.record_digest(id).unwrap())
        .collect();
    drop(bridge);

    // restart: same home, same machine, cited records unchanged
    let bridge = ComposeBridge::new(home.clone(), proposer(), "test:fixed-proposer");
    for (id, d) in cited.iter().zip(digests.iter()) {
        assert_eq!(
            &bridge.record_digest(*id).unwrap(),
            d,
            "record {id} after restart"
        );
    }
    let r2 = report(bridge.run_task("second goal", ws.to_str().unwrap()));
    assert!(r2.committed);
    assert_eq!(
        r2.machine_id, r1.machine_id,
        "same AienMachineId across restart"
    );
    assert!(r2.cx_evidence > r1.cx_evidence);
    let after = recalled(bridge.recall(&[r1.cx_promotion], Some(c.id)));
    assert_eq!(after.machine_id, before.machine_id);
    assert_eq!(
        after.prefix_digest, before.prefix_digest,
        "records 1..=S1 unchanged"
    );
    assert_eq!(after.cited, before.cited);
    let k = after
        .host
        .iter()
        .find(|h| h.id == c.id)
        .expect("constraint recalled after restart");
    assert_eq!(k.note.as_deref(), Some("constraint"));
    assert_eq!(k.text.as_deref(), Some("keep main green"));
    assert!(k.verified);

    // containment: nothing in the workspace changed (this cut executes no effect)
    assert_eq!(snapshot(&ws), ws_before);
    // the home holds only the composition files (no staged-branch leftovers)
    for (name, _) in snapshot(&home) {
        assert!(
            name == "machine.id" || name == "cortex.cx" || name.starts_with("jspace"),
            "unexpected file in compose home: {name}"
        );
    }
}

#[test]
fn failing_model_commits_nothing() {
    if !aien_omega_compose::LINKED {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let bridge = ComposeBridge::new(
        tmp.path().join("compose"),
        Arc::new(|_: &str, _: std::time::Duration| Err("model unavailable".to_string())),
        "test:failing",
    );
    let r = report(bridge.run_task("goal", ws.to_str().unwrap()));
    assert!(!r.committed, "{r:?}");
    assert_eq!(r.winner, None);
    assert_eq!(r.proposal, None);
}

#[test]
fn bad_workspace_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let bridge = ComposeBridge::new(tmp.path().join("compose"), proposer(), "test");
    match bridge.run_task("goal", tmp.path().join("missing").to_str().unwrap()) {
        ControlResponse::Error(e) => assert!(e.contains("workspace"), "{e}"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn proposal_template_parser() {
    // filename line present
    let p = check_file_proposal(
        "filename: NOTES.md
line one
",
    )
    .unwrap();
    assert_eq!(
        (p.path.as_str(), p.content.as_str()),
        (
            "NOTES.md",
            "line one
"
        )
    );
    let p = check_file_proposal(
        "
**Filename: `docs/plan.txt`**

```
step 1
step 2
```
",
    )
    .unwrap();
    assert_eq!(
        (p.path.as_str(), p.content.as_str()),
        (
            "docs/plan.txt",
            "step 1
step 2
"
        )
    );
    // filename line absent: refused, even when a path appears later
    for t in [
        "Sure, I can help with that!",
        "NOTES.md
line one
",
        "To create NOTES.md:
filename: NOTES.md
x
",
        "",
    ] {
        assert!(check_file_proposal(t).is_err(), "{t:?}");
        assert!(parse_file_proposal(t).is_none());
    }
    assert!(check_file_proposal(
        "NOTES.md
x"
    )
    .unwrap_err()
    .contains("no filename line"));
    // outside the workspace: refused before AEGIS
    for t in [
        "filename: /etc/passwd
root
",
        "filename: ../escape.txt
x
",
        "filename: docs/../../escape.txt
x
",
        "filename: ~/.bashrc
x
",
    ] {
        let e = check_file_proposal(t).unwrap_err();
        assert!(e.contains("outside the workspace"), "{t:?}: {e}");
    }
    assert!(check_file_proposal(
        "filename: a b.txt
x
"
    )
    .is_err());
    assert!(check_file_proposal(
        "filename: NOTES.md

   
"
    )
    .unwrap_err()
    .contains("empty content"));
    // deterministic
    assert_eq!(
        check_file_proposal(
            "filename: a.txt
z
"
        ),
        check_file_proposal(
            "filename: a.txt
z
"
        )
    );
}

#[test]
fn retry_is_capped_and_recorded() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;
    let budget = Duration::from_secs(25);
    // never parses: exactly COMPOSE_MAX_ATTEMPTS calls, all refused
    let calls = AtomicU32::new(0);
    let chatty = |_: &str, _: Duration| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(Generation {
            text: "Sure!".into(),
            tokens: 2,
            finish_reason: None,
            ..Default::default()
        })
    };
    let (out, a) = propose_with_retries(
        &chatty,
        "base",
        budget,
        COMPOSE_ATTEMPT_BUDGET,
        COMPOSE_MAX_ATTEMPTS,
    );
    assert!(out.unwrap_err().contains("no filename line"));
    assert_eq!(calls.load(Ordering::SeqCst), COMPOSE_MAX_ATTEMPTS);
    assert_eq!(a.len(), 3);
    assert!(a
        .iter()
        .all(|x| x.outcome == "refused" && x.tokens == 2 && x.text_sha256.is_some()));
    assert_eq!(
        a.iter().map(|x| x.attempt).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    // second attempt parses: stops there, and the retry prompt names the refusal
    let prompts = parking_lot::Mutex::new(Vec::<String>::new());
    let second = |p: &str, _: Duration| {
        prompts.lock().push(p.to_string());
        let text = if prompts.lock().len() == 1 {
            "Sure!"
        } else {
            "filename: a.txt
ok
"
        };
        Ok(Generation {
            text: text.into(),
            tokens: 3,
            finish_reason: None,
            ..Default::default()
        })
    };
    let (out, a) = propose_with_retries(
        &second,
        "base",
        budget,
        COMPOSE_ATTEMPT_BUDGET,
        COMPOSE_MAX_ATTEMPTS,
    );
    assert_eq!(
        out.unwrap(),
        "filename: a.txt
ok
"
    );
    assert_eq!(a.len(), 2);
    assert_eq!(
        (a[0].outcome.as_str(), a[1].outcome.as_str()),
        ("refused", "parsed")
    );
    let p = prompts.lock();
    assert_eq!(p[0], "base");
    assert!(
        p[1].starts_with(
            "base
"
        ) && p[1].contains("previous answer was refused")
    );
    drop(p);
    // budget: attempt 2 does not start when less than the per-attempt budget is left
    let slow = |_: &str, _: Duration| {
        std::thread::sleep(Duration::from_millis(30));
        Ok(Generation {
            text: "Sure!".into(),
            tokens: 2,
            finish_reason: None,
            ..Default::default()
        })
    };
    let (out, a) = propose_with_retries(
        &slow,
        "base",
        Duration::from_millis(40),
        Duration::from_millis(30),
        COMPOSE_MAX_ATTEMPTS,
    );
    assert_eq!(a.len(), 1);
    assert!(out.unwrap_err().contains("no attempt 2"));
    // model errors are recorded, not retried past the cap
    let err =
        |_: &str, _: Duration| Err::<Generation, _>("model proposal exceeded 25000 ms".to_string());
    let (_, a) = propose_with_retries(
        &err,
        "base",
        budget,
        COMPOSE_ATTEMPT_BUDGET,
        COMPOSE_MAX_ATTEMPTS,
    );
    assert!(a.len() <= 3 && a.iter().all(|x| x.outcome == "timeout"));
}

/// ACCEPTANCE-v3 Section 3b at 1/100 scale: a slow (cold) attempt 1 of 16.1 s
/// no longer blocks attempt 2; attempt 2 of 11.6 s leaves too little for 3.
#[test]
fn measured_attempt_budget_admits_a_second_attempt() {
    use std::time::Duration;
    assert!(COMPOSE_SKILL_BUDGET < Duration::from_secs(30));
    // the default (AIEN_COMPOSE_EDIT_BUDGET_MS unset): omega's 30 s wait less 1 s
    assert_eq!(COMPOSE_SKILL_BUDGET, Duration::from_secs(29));
    assert_eq!(COMPOSE_ATTEMPT_BUDGET, Duration::from_millis(12_000));
    // 2 839 + 896 + 47 x 166.6 + 60 ms, the measured full retry attempt
    let measured: [f64; 4] = [2_839.0, 896.0, 47.0 * 166.6, 60.0];
    assert!(measured.iter().sum::<f64>() <= COMPOSE_ATTEMPT_BUDGET.as_millis() as f64);
    let n = std::sync::atomic::AtomicU32::new(0);
    let cold_then_warm = |_: &str, _: Duration| {
        let k = n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(if k == 0 { 161 } else { 116 }));
        Ok(Generation {
            text: "Goal: echo".into(),
            tokens: 48,
            finish_reason: None,
            ..Default::default()
        })
    };
    let (out, a) = propose_with_retries(
        &cold_then_warm,
        "base",
        COMPOSE_SKILL_BUDGET / 100,
        COMPOSE_ATTEMPT_BUDGET / 100,
        COMPOSE_MAX_ATTEMPTS,
    );
    assert_eq!(a.len(), 2, "{a:?}");
    let reason = out.unwrap_err();
    assert!(reason.contains("no attempt 3") && reason.contains("120 ms per-attempt budget"));
}

#[test]
fn unparseable_proposal_fails_the_aegis_contract() {
    if !aien_omega_compose::LINKED {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let bridge = ComposeBridge::new(
        tmp.path().join("compose"),
        Arc::new(|_: &str, _: std::time::Duration| {
            Ok(Generation {
                text: "Sure, I can help with that!".to_string(),
                tokens: 8,
                finish_reason: None,
                ..Default::default()
            })
        }),
        "test:chatty",
    );
    let r = report(bridge.run_task("goal", ws.to_str().unwrap()));
    assert!(!r.committed, "{r:?}");
    assert_eq!(r.aegis_pass_mask & 1, 0);
    assert_eq!(r.proposal_path, None);
    // Refused by the template parser before AEGIS, retried up to the cap.
    assert_eq!(
        r.proposal_attempts.len(),
        COMPOSE_MAX_ATTEMPTS as usize,
        "{r:?}"
    );
    assert!(r
        .proposal_attempts
        .iter()
        .all(|a| a.outcome == "refused" && a.aegis.is_none()));
    assert!(r
        .proposer_error
        .as_deref()
        .unwrap_or("")
        .contains("no filename line"));
}

#[test]
fn torn_home_is_refused_by_name_then_recovered() {
    if !aien_omega_compose::LINKED {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("compose");
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let bridge = ComposeBridge::new(home.clone(), proposer(), "test");
    let r1 = report(bridge.run_task("goal", ws.to_str().unwrap()));
    assert!(r1.committed);
    let all = recalled(bridge.recall(&[], Some(r1.cx_promotion)));
    drop(bridge);
    let cx = home.join("cortex.cx");
    let len = std::fs::metadata(&cx).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&cx)
        .unwrap()
        .set_len(len - 5)
        .unwrap();

    let bridge = ComposeBridge::new(home.clone(), proposer(), "test");
    for _ in 0..2 {
        match bridge.note("constraint", "x", &[]) {
            ControlResponse::Error(e) => {
                assert!(e.contains("E_TORN") && e.contains("compose recover"), "{e}")
            }
            other => panic!("torn home must be refused, got {other:?}"),
        }
        assert_eq!(
            std::fs::metadata(&cx).unwrap().len(),
            len - 5,
            "refusal wrote"
        );
    }
    let rep = match bridge.recover() {
        ControlResponse::ComposeRecovered(r) => *r,
        other => panic!("recover: {other:?}"),
    };
    assert!(rep.repaired && rep.tail_torn && rep.opens, "{rep:?}");
    assert_eq!((rep.cause, rep.cut_hi), (-8, len - 5));
    assert!(rep.dropped_records >= 1 && rep.repair_record == rep.records_kept + 1);
    let after = recalled(bridge.recall(&[], Some(r1.cx_promotion)));
    assert_eq!(
        after.prefix_digest, all.prefix_digest,
        "old records unchanged"
    );
    assert_eq!(after.machine_id, all.machine_id);
    assert!(after
        .host
        .iter()
        .any(|h| h.id == rep.repair_record && h.note.as_deref() == Some("repair_tail")));
    let r2 = report(bridge.run_task("goal after recover", ws.to_str().unwrap()));
    assert!(r2.committed);
}

/// ACCEPTANCE-v5 Q3: the proposer's stop reason reaches every attempt record,
/// so the receipt can tell end of sequence from the token limit.
#[test]
fn finish_reason_is_recorded_per_attempt() {
    use std::time::Duration;
    let n = std::sync::atomic::AtomicU32::new(0);
    let truncated_then_eos = |_: &str, _: Duration| {
        let k = n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(if k == 0 {
            Generation {
                text: "Goal: echo".into(),
                tokens: 48,
                finish_reason: Some("max_tokens".into()),
                ..Default::default()
            }
        } else {
            Generation {
                text: "filename: NOTES.md\nkeep changes in the workspace\n".into(),
                tokens: 11,
                finish_reason: Some("eos".into()),
                ..Default::default()
            }
        })
    };
    let (out, a) = propose_with_retries(
        &truncated_then_eos,
        "base",
        Duration::from_secs(5),
        Duration::from_millis(1),
        COMPOSE_MAX_ATTEMPTS,
    );
    assert!(out.is_ok(), "{out:?}");
    assert_eq!(a.len(), 2, "{a:?}");
    assert_eq!(a[0].finish_reason.as_deref(), Some("max_tokens"));
    assert_eq!(a[1].finish_reason.as_deref(), Some("eos"));
    // An attempt record without the field (v1..v4 receipts) still reads.
    let old = r#"{"attempt":1,"ms":1,"tokens":48,"outcome":"parsed","reason":null,
        "text_sha256":null,"text":null,"aegis":null}"#;
    let parsed: aien_runtime::ProposalAttempt = serde_json::from_str(old).unwrap();
    assert_eq!(parsed.finish_reason, None);
}

#[test]
fn finish_reason_labels_match_acceptance_v5() {
    use aien_inference_abi::FinishReason;
    assert_eq!(
        aien_runtime::finish_reason_label(&FinishReason::StopToken),
        "eos"
    );
    assert_eq!(
        aien_runtime::finish_reason_label(&FinishReason::LengthLimit),
        "max_tokens"
    );
    assert_eq!(
        aien_runtime::finish_reason_label(&FinishReason::Aborted),
        "aborted"
    );
    assert_eq!(
        aien_runtime::finish_reason_label(&FinishReason::Preempted),
        "preempted"
    );
}

// A file whose own content holds fenced blocks must not be cut at the
// first inner fence (DIAGNOSTIC-LONG-1 task D5: a 649-token FAQ was saved
// as 4 lines).
#[test]
fn proposal_fence_inner_block_kept() {
    let p = check_file_proposal(
        "filename: docs/FAQ.md\n```markdown\n# FAQ\n\nRun:\n```bash\nls -l\n```\nDone.\n```\nTrailing prose.\n",
    )
    .unwrap();
    assert_eq!(p.path, "docs/FAQ.md");
    assert_eq!(p.content, "# FAQ\n\nRun:\n```bash\nls -l\n```\nDone.\n");
}

#[test]
fn proposal_fence_d5_reply_shape() {
    // D5 reply, trimmed to its first two blocks and closed by the outer fence.
    let reply = include_str!("fixtures/d5_reply_trimmed.txt");
    let reply = format!("{reply}```\n");
    let p = check_file_proposal(&reply).unwrap();
    assert_eq!(p.path, "docs/FAQ.md");
    assert_eq!(p.content.matches("```bash").count(), 2, "{}", p.content);
    assert_eq!(p.content.lines().count(), 14, "{}", p.content);
    assert!(p.content.ends_with("```\n"));
    assert!(p.content.contains("sudo curl"));
    assert!(p.content.lines().count() > 4, "old parser kept 4 lines");
}

#[test]
fn proposal_fence_unbalanced_takes_last_bare_fence() {
    // Inner block opened and never closed: the last bare fence ends the file.
    let p = check_file_proposal("filename: a.md\n```md\ntext\n```bash\nls\nmore\n```\n").unwrap();
    assert_eq!(p.content, "text\n```bash\nls\nmore\n");
    // No bare fence at all: nothing to close, whole body kept (as before).
    let p = check_file_proposal("filename: a.md\n```md\ntext\n```bash\nls\n").unwrap();
    assert_eq!(p.content, "text\n```bash\nls\n");
}

#[test]
fn proposal_fence_four_backtick_outer() {
    let p = check_file_proposal(
        "filename: a.md\n````markdown\nintro\n```\nbare inner\n```\n```bash\nls\n```\n````\n",
    )
    .unwrap();
    assert_eq!(p.content, "intro\n```\nbare inner\n```\n```bash\nls\n```\n");
}

#[test]
fn proposal_fence_plain_replies_unchanged() {
    let p = check_file_proposal("filename: a.txt\nx\ny\n").unwrap();
    assert_eq!(p.content, "x\ny\n");
    let p = check_file_proposal("filename: a.txt\n```\nx\n```\nthanks\n").unwrap();
    assert_eq!(p.content, "x\n");
    let p = check_file_proposal("filename: a.txt\n```text\nx\n").unwrap();
    assert_eq!(p.content, "x\n");
}

// Several code examples in one document and normal prose after the last one:
// every byte is kept, in the fenced reply shape and in the unfenced shape.
#[test]
fn proposal_fence_many_examples_and_trailing_text_keep_every_byte() {
    let doc = "# Guide\n\nStep one:\n```bash\nls -l\n```\nStep two:\n```python\nprint(1)\n```\nStep three:\n```\nplain\n```\nNotes after the last example.\nAnd one more line.\n";
    // Outer fence with a label: inner fences nest, nothing after is lost.
    let p = check_file_proposal(&format!("filename: g.md\n```markdown\n{doc}```\n")).unwrap();
    assert_eq!(p.content, doc);
    // Longer outer fence: inner bare and labelled fences are all content.
    let p = check_file_proposal(&format!("filename: g.md\n````markdown\n{doc}````\n")).unwrap();
    assert_eq!(p.content, doc);
    // No outer fence at all: the document is taken whole.
    let p = check_file_proposal(&format!("filename: g.md\n{doc}")).unwrap();
    assert_eq!(p.content, doc);
}

/// Both budget settings: unset gives the default; 1 000..=599 000 ms is
/// accepted; anything else is refused with a message naming the setting.
#[test]
fn compose_budget_settings_parse_and_refuse() {
    use std::time::Duration;
    let edit = |v| parse_compose_budget("AIEN_COMPOSE_EDIT_BUDGET_MS", COMPOSE_SKILL_BUDGET, v);
    let doc = |v| parse_compose_budget("AIEN_COMPOSE_DOC_BUDGET_MS", COMPOSE_DOC_BUDGET, v);
    assert_eq!(edit(None), Ok(Duration::from_secs(29)));
    assert_eq!(doc(None), Ok(Duration::from_secs(120)));
    assert_eq!(edit(Some("29000")), Ok(COMPOSE_SKILL_BUDGET));
    assert_eq!(doc(Some("90000")), Ok(Duration::from_secs(90)));
    assert_eq!(doc(Some(" 1000 ")), Ok(Duration::from_secs(1)));
    assert_eq!(doc(Some("599000")), Ok(Duration::from_millis(599_000)));
    for bad in [
        "",
        "abc",
        "-5",
        "1.5",
        "999",
        "0",
        "599001",
        "600000",
        "99999999999999999999",
    ] {
        let e = edit(Some(bad)).expect_err(bad);
        assert!(e.contains("AIEN_COMPOSE_EDIT_BUDGET_MS"), "{bad}: {e}");
        let e = doc(Some(bad)).expect_err(bad);
        assert!(e.contains("AIEN_COMPOSE_DOC_BUDGET_MS"), "{bad}: {e}");
    }
}

/// AIEN_COMPOSE_DOC_MAX_TOKENS: default 1024, 16..=4096, bad values refused.
#[test]
fn compose_doc_token_cap_parses_and_refuses() {
    assert_eq!(parse_compose_doc_max_tokens(None), Ok(1024));
    assert_eq!(parse_compose_doc_max_tokens(Some("16")), Ok(16));
    assert_eq!(parse_compose_doc_max_tokens(Some(" 4096 ")), Ok(4096));
    for bad in ["", "x", "-1", "15", "0", "4097", "1e3"] {
        let e = parse_compose_doc_max_tokens(Some(bad)).expect_err(bad);
        assert!(e.contains("AIEN_COMPOSE_DOC_MAX_TOKENS"), "{bad}: {e}");
    }
}

/// A task that names an existing file is an edit (29 s); a task with no
/// existing target creates a new document (120 s). Omega's wait follows by +1 s.
#[test]
fn kind_selects_budget() {
    use std::time::Duration;
    let ws = tempfile::tempdir().expect("tempdir");
    std::fs::write(ws.path().join("notes.md"), "# notes\n").expect("write");
    let (_, _, edit_kind) =
        aien_runtime::spine::task_plan("add a line to notes.md", ws.path()).expect("plan");
    let (_, _, new_kind) =
        aien_runtime::spine::task_plan("write a new file faq.md", ws.path()).expect("plan");
    assert_eq!(edit_kind, ProposalKind::Edit);
    assert_eq!(new_kind, ProposalKind::Document);
    let b = ComposeBudgets::default();
    assert_eq!(b.for_kind(ProposalKind::Edit), Duration::from_secs(29));
    assert_eq!(b.for_kind(ProposalKind::Document), Duration::from_secs(120));
    assert_eq!(
        b.for_kind(ProposalKind::Document) + COMPOSE_WAIT_MARGIN,
        Duration::from_millis(121_000)
    );
    // edits keep the measured 12 s attempt budget; documents need ~the shortest
    // measured full document (30 s), capped at half a small custom budget
    assert_eq!(
        compose_attempt_budget(ProposalKind::Edit, b.edit),
        COMPOSE_ATTEMPT_BUDGET
    );
    assert_eq!(
        compose_attempt_budget(ProposalKind::Document, b.doc),
        Duration::from_secs(30)
    );
    assert_eq!(
        compose_attempt_budget(ProposalKind::Document, Duration::from_secs(20)),
        Duration::from_secs(10)
    );
}

/// The two ways a long reply can fail leave different, stable reasons: a cut
/// at the token limit is refused ("reply cut at the token limit ...
/// finish_reason max_tokens"), a slow model times out ("model proposal
/// exceeded N ms"). Neither text contains the other's marker.
#[test]
fn token_cut_and_wall_clock_reasons_are_distinct() {
    use std::time::Duration;
    // token-limit cut
    let cut = |_: &str, _: Duration| {
        Ok(Generation {
            text: "filename: a.md\nline\n".into(),
            tokens: 1024,
            finish_reason: Some("max_tokens".into()),
            ..Default::default()
        })
    };
    let (out, a) = propose_with_retries(
        &cut,
        "p",
        Duration::from_secs(120),
        Duration::from_secs(30),
        1,
    );
    let why = out.expect_err("a cut reply is never a proposal");
    assert_eq!(a[0].outcome, "refused");
    assert!(
        why.contains("reply cut at the token limit after 1024 tokens (finish_reason max_tokens)"),
        "{why}"
    );
    assert!(!why.contains("exceeded"), "{why}");
    // wall-clock timeout (the text the daemon proposer emits)
    let slow = |_: &str, limit: Duration| -> Result<Generation, String> {
        Err(wall_clock_reason(limit.as_millis()))
    };
    let (out, a) = propose_with_retries(
        &slow,
        "p",
        Duration::from_secs(120),
        Duration::from_secs(30),
        1,
    );
    let why = out.expect_err("timed out");
    assert_eq!(a[0].outcome, "timeout");
    assert!(
        why.starts_with("model proposal exceeded ") && why.ends_with(" ms"),
        "{why}"
    );
    assert!(
        !why.contains("token limit") && !why.contains("max_tokens"),
        "{why}"
    );
}

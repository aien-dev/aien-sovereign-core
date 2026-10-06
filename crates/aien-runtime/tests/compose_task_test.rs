//! NEXT-PHASE-1 cut 1b + 2: RunComposeTask, ComposeNote, ComposeRecall and
//! RecoverComposeHome through omega COMPOSITION-2.
//!
//! With librx_compose.a linked (aien-omega-compose built with
//! AIEN_OMEGA_COMPOSE_DIR, see its README) these run the real composition;
//! in a stub build they check that the bridge reports the missing library.
use aien_runtime::control::{ComposeRecallReport, ComposeTaskReport, ControlResponse};
use aien_runtime::spine::{parse_file_proposal, ComposeBridge, ComposeProposer};
use std::sync::Arc;

fn proposer() -> ComposeProposer {
    Arc::new(|prompt: &str| {
        assert!(prompt.contains("Authorized workspace:"));
        Ok("NOTES.md\nconstraint: keep main green\n".to_string())
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
        Some("NOTES.md\nconstraint: keep main green\n")
    );
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
        Arc::new(|_: &str| Err("model unavailable".to_string())),
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
fn proposal_parsing_is_strict_and_deterministic() {
    let p = parse_file_proposal("NOTES.md\nline one\n").unwrap();
    assert_eq!(
        (p.path.as_str(), p.content.as_str()),
        ("NOTES.md", "line one\n")
    );
    let p = parse_file_proposal(
        "Sure! Here it is.\n\nPath: `docs/plan.txt`\n```\nstep 1\nstep 2\n```\nThanks",
    )
    .unwrap();
    assert_eq!(
        (p.path.as_str(), p.content.as_str()),
        ("docs/plan.txt", "step 1\nstep 2\n")
    );
    assert!(parse_file_proposal("Sure, I can help with that!").is_none());
    assert!(parse_file_proposal("/etc/passwd\nroot\n").is_none());
    assert!(parse_file_proposal("../escape.txt\nx\n").is_none());
    assert!(parse_file_proposal("NOTES.md\n\n   \n").is_none());
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
        Arc::new(|_: &str| Ok("Sure, I can help with that!".to_string())),
        "test:chatty",
    );
    let r = report(bridge.run_task("goal", ws.to_str().unwrap()));
    assert!(!r.committed, "{r:?}");
    assert_eq!(r.aegis_pass_mask & 1, 0);
    assert_eq!(r.proposal_path, None);
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

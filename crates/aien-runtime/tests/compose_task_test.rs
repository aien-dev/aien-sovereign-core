//! NEXT-PHASE-1 cut 1b: RunComposeTask through omega COMPOSITION-2.
//!
//! With librx_compose.a linked (aien-omega-compose built with
//! AIEN_OMEGA_COMPOSE_DIR, see its README) these run the real composition;
//! in a stub build they check that the bridge reports the missing library.
use aien_runtime::control::{ComposeTaskReport, ControlResponse};
use aien_runtime::spine::{ComposeBridge, ComposeProposer};
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

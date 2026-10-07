//! The omega settle wait really set before each run, and the target-kind
//! decision (existing path => Edit/refused, missing path => Document).
//! One test binary, one wait test: `LAST_WAIT_MS` is process-global.
use aien_runtime::control::ControlResponse;
use aien_runtime::spine::{
    classify_target, propose_task_with_retries, task_plan, ComposeBridge, ComposeBudgets,
    ComposeProposer, Generation, ProposalKind, TargetClass, COMPOSE_WAIT_MARGIN,
};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Linked build only: run_task_inner tells omega budget + 1000 ms for each
/// kind before compose.run (observed inside the proposer, i.e. during the run).
#[test]
#[cfg_attr(not(compose_linked), ignore = "needs librx_compose.a: stub build")]
fn run_task_sets_omega_wait_to_budget_plus_margin_for_both_kinds() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("notes.md"), "# notes\n").unwrap();
    let seen: Arc<Mutex<Vec<u32>>> = Arc::default();
    let s = seen.clone();
    let proposer: ComposeProposer = Arc::new(move |_p: &str, _l: Duration| {
        s.lock()
            .unwrap()
            .push(aien_omega_compose::LAST_WAIT_MS.load(Ordering::SeqCst));
        Ok(Generation {
            text: "filename: faq.md\nconstraint: x\n".into(),
            tokens: 4,
            finish_reason: Some("eos".into()),
            ..Default::default()
        })
    });
    let b = ComposeBudgets::default();
    let bridge = ComposeBridge::new(tmp.path().join("home"), proposer, "test:wait");
    for goal in ["add a line to notes.md", "write a new file faq.md"] {
        match bridge.run_task(goal, ws.to_str().unwrap()) {
            ControlResponse::ComposeTaskResult(_) | ControlResponse::Error(_) => {}
            other => panic!("{other:?}"),
        }
    }
    let seen = seen.lock().unwrap().clone();
    let want = |k| (b.for_kind(k) + COMPOSE_WAIT_MARGIN).as_millis() as u32;
    assert_eq!(
        seen,
        vec![want(ProposalKind::Edit), want(ProposalKind::Document)],
        "wait seen during each run: edit 30000, document 121000"
    );
}

/// One deadline across attempts: each retry is given only what is left of
/// the document budget, never a fresh budget.
#[test]
fn one_deadline_is_shared_across_attempts() {
    let limits: Mutex<Vec<Duration>> = Mutex::default();
    let proposer = |_: &str, l: Duration| {
        limits.lock().unwrap().push(l);
        std::thread::sleep(Duration::from_millis(60));
        Ok(Generation {
            text: "not a proposal".into(),
            tokens: 2,
            finish_reason: Some("eos".into()),
            ..Default::default()
        })
    };
    let budget = Duration::from_millis(400);
    let (out, a) = propose_task_with_retries(
        &proposer,
        "base",
        None,
        budget,
        Duration::from_millis(50),
        3,
    );
    assert!(out.is_err());
    let l = limits.lock().unwrap().clone();
    assert!(l.len() >= 2, "{l:?}");
    assert!(l[0] <= budget);
    for w in l.windows(2) {
        assert!(w[1] + Duration::from_millis(55) <= w[0], "{l:?}");
    }
    assert_eq!(a.len(), l.len());
}

fn ws_with(files: &[(&str, &[u8])]) -> (tempfile::TempDir, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let ws = d.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    for (n, c) in files {
        std::fs::write(ws.join(n), c).unwrap();
    }
    let ws = std::fs::canonicalize(ws).unwrap();
    (d, ws)
}

#[test]
fn missing_file_is_document() {
    let (_d, ws) = ws_with(&[]);
    assert_eq!(classify_target("write faq.md", &ws), TargetClass::New);
    let (_, t, k) = task_plan("write faq.md", &ws).unwrap();
    assert_eq!((t, k), (None, ProposalKind::Document));
}

#[test]
fn small_existing_file_is_edit() {
    let (_d, ws) = ws_with(&[("notes.md", b"# notes\n")]);
    let (p, t, k) = task_plan("add a line to notes.md", &ws).unwrap();
    assert_eq!(k, ProposalKind::Edit);
    assert_eq!(t, Some(("notes.md".into(), "# notes\n".into())));
    assert!(p.contains("already exists"));
}

#[test]
fn large_existing_file_is_refused_not_document() {
    let big = vec![b'a'; 11 * 1024];
    let (_d, ws) = ws_with(&[("big.md", &big)]);
    let e = task_plan("rewrite big.md", &ws).unwrap_err();
    assert!(e.contains("big.md") && e.contains("already exists"), "{e}");
    assert!(matches!(
        classify_target("rewrite big.md", &ws),
        TargetClass::Refused(_)
    ));
    assert_eq!(std::fs::read(ws.join("big.md")).unwrap(), big);
}

#[test]
fn non_utf8_existing_file_is_refused() {
    let (_d, ws) = ws_with(&[("bin.md", &[0xff, 0xfe, 0x00, 0x80])]);
    let e = task_plan("rewrite bin.md", &ws).unwrap_err();
    assert!(e.contains("not UTF-8"), "{e}");
}

#[test]
fn outward_symlink_is_refused_and_directory_is_skipped() {
    let d = tempfile::tempdir().unwrap();
    let outside = d.path().join("secret.txt");
    std::fs::write(&outside, "s\n").unwrap();
    let ws = d.path().join("ws");
    std::fs::create_dir_all(ws.join("docs")).unwrap();
    std::os::unix::fs::symlink(&outside, ws.join("link.md")).unwrap();
    let ws = std::fs::canonicalize(ws).unwrap();
    assert!(task_plan("rewrite link.md", &ws).is_err());
    assert_eq!(
        classify_target("put it in docs faq.md", &ws),
        TargetClass::New
    );
}

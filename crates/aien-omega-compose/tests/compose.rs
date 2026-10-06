//! The omega C test (tests/runtime/rxc_host_abi_test.c) repeated from Rust:
//! run one goal, restart + recall, identity refusal, torn tail. Ignored
//! unless librx_compose.a is linked (AIEN_OMEGA_COMPOSE_DIR or
//! AIEN_OMEGA_COMPOSE_LIB at build time; see build.rs / README.md).
use aien_omega_compose::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const ROOT: &[u8] = b"np1-host-abi-test-machine";
fn propose(t: u64) -> u64 {
    t.wrapping_mul(3).wrapping_add(1)
}

fn open(
    dir: &std::path::Path,
    seen: Arc<AtomicU64>,
    refuse_torn: bool,
) -> Result<(Compose, Info), ComposeError> {
    let (mut c, info) = Compose::open(dir, RootKind::Provisioned, ROOT, 0x5E55, refuse_torn)?;
    c.register_skill("np1.echo-proposal", None, 10, move |task| {
        seen.store(task, Ordering::SeqCst);
        Some(propose(task))
    })?;
    c.set_verify(|task, result| result == propose(task))?;
    Ok((c, info))
}

#[test]
#[cfg_attr(
    not(has_omega_compose),
    ignore = "needs librx_compose.a (AIEN_OMEGA_COMPOSE_DIR)"
)]
fn run_restart_recall_torn_tail() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let seen = Arc::new(AtomicU64::new(0));

    // T1: one goal
    let (mut c, i1) = open(&home, seen.clone(), false).unwrap();
    assert_eq!(i1.abi_version, 1);
    assert_eq!(i1.machine_id_was_stored, 0);
    let task = 0x4E50_3100_0000_0001u64;
    let r = c.run(task, 1_000_000).unwrap();
    assert_eq!(r.outcome, OUT_COMMITTED);
    assert_eq!(r.committed, 1);
    assert_eq!(seen.load(Ordering::SeqCst), task);
    assert_eq!(r.result, propose(task));
    assert_eq!((r.n_branches, r.winner, r.winner_skill), (1, 0, 0));
    assert_eq!(r.aegis_pass_mask & 1, 1);
    let cited = [r.cx_goal, r.cx_candidate[0], r.cx_evidence, r.cx_promotion];
    assert!(cited.iter().all(|&id| id != 0), "{cited:?}");
    let cited_digests: Vec<[u8; 32]> = cited
        .iter()
        .map(|&id| c.record(id).unwrap().digest)
        .collect();
    let (before, total) = c.recall(SUBJECT_STATE, 64).unwrap();
    assert!(!before.is_empty() && before.len() as u64 == total);
    let mid = c.info().unwrap().machine_id;
    drop(c);

    // T2: restart + recall (pre-restart records are an unchanged prefix)
    let (mut c, i2) = open(&home, seen.clone(), false).unwrap();
    assert_eq!(i2.machine_id_was_stored, 1);
    assert_eq!(i2.machine_id, mid);
    assert_eq!(i2.tail_torn, 0);
    let (after, _) = c.recall(SUBJECT_STATE, 64).unwrap();
    assert!(after.len() >= before.len());
    for (a, b) in after.iter().zip(before.iter()) {
        assert_eq!((a.id, a.digest, a.verified), (b.id, b.digest, 1));
    }
    for (id, d) in cited.iter().zip(cited_digests.iter()) {
        assert_eq!(&c.record(*id).unwrap().digest, d);
    }
    drop(c);

    // T3: another machine is refused on this home
    let other = Compose::open(
        &home,
        RootKind::Provisioned,
        b"np1-some-other-machine",
        1,
        false,
    );
    assert!(matches!(
        other.err(),
        Some(ComposeError::Code { code: -2, .. })
    ));

    // T5a: torn tail past the anchor (after an open, no run): strict refuses, default repairs
    let (mut c, _) = open(&home, seen.clone(), false).unwrap();
    let n0 = c.info().unwrap().records;
    let all: Vec<Record> = (1..=n0).map(|id| c.record(id).unwrap()).collect();
    drop(c);
    let cx = home.join("cortex.cx");
    let len = std::fs::metadata(&cx).unwrap().len();
    let f = std::fs::OpenOptions::new().write(true).open(&cx).unwrap();
    f.set_len(len - 5).unwrap();
    drop(f);
    let strict = open(&home, seen.clone(), true);
    assert!(matches!(
        strict.err(),
        Some(ComposeError::Code { code: -3, .. })
    ));
    assert_eq!(
        std::fs::metadata(&cx).unwrap().len(),
        len - 5,
        "strict refusal left the journal"
    );
    let (mut c, ir) = open(&home, seen.clone(), false).unwrap();
    assert_eq!(ir.tail_torn, 1);
    let i = c.info().unwrap();
    assert!(i.records >= n0 - 1);
    for rec in &all[..all.len() - 1] {
        assert_eq!(c.record(rec.id).unwrap().digest, rec.digest);
    }
    let r = c.run(0x4E50_3100_0000_0003, 3_000_000).unwrap();
    assert_eq!(r.committed, 1);
    drop(c);

    // T5b: torn tail right after a run: explicit refusal (rx_compose anchor check), stable
    let len = std::fs::metadata(&cx).unwrap().len();
    let f = std::fs::OpenOptions::new().write(true).open(&cx).unwrap();
    f.set_len(len - 5).unwrap();
    drop(f);
    let (mut c, ir) = open(&home, seen.clone(), false).unwrap();
    assert_eq!(ir.tail_torn, 1);
    match c.info() {
        Err(ComposeError::Code { code: -4, detail }) => assert_ne!(detail, 0),
        Ok(_) => {
            for (id, d) in cited.iter().zip(cited_digests.iter()) {
                assert_eq!(&c.record(*id).unwrap().digest, d);
            }
        }
        Err(e) => panic!("unexpected {e}"),
    }
}

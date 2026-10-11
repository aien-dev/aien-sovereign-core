//! The local pipeline end to end, and the negative cases the mission lists
//! (22 to 27): an altered receipt, a bad signature, a bundle from another
//! commit, a missing required check, a hardware-scoped cached result under
//! `--release`, and an executor whose key the repository does not trust.
//! Each must leave MERGE ELIGIBILITY at BLOCKED; none may print PASS.
//!
//! The worker runs with `--sandbox none` here because the exec helper is the
//! INFRA lane's file; the sandbox test at the end runs only when
//! `/usr/local/sbin/aien-run-exec` exists.

use aien_test::bundle;
use aien_test::evidence::canonical;
use aien_test::plan::{self, Request};
use aien_test::runner::{self, Options};
use aien_test::sandbox::Sandbox;
use aien_test::verdict::Verdict;
use aien_test::verify::{self, Ask};
use aien_test::worker::{self, Config, StateDir};
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Tmp {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p =
            std::env::temp_dir().join(format!("aien-local-ci-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(dir: &Path, rel: &str, content: &str, exec: bool) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, content).unwrap();
    if exec {
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn gate(id: &str, input: &str, exec: &str, extra: &str) -> String {
    format!(
        "gate: {id}\nmanifest_version: 2\nowner: t\nrequires:\n  - host\ninputs:\n  - {input}\nrun:\n  exec: {exec}\nexpects:\n  exit: 0\n  verdict: PASS\ntimeout: 10s\n{extra}"
    )
}

/// Everything one scenario needs: an origin with `main` (base) and `topic`
/// (head, changes `a/a.txt`), a trusted clone, a state dir, a signing key
/// whose public half is committed in `.aien/verify.pub`, and a fake `crumb`
/// that always says OK.
struct World {
    _root: Tmp,
    origin: PathBuf,
    clone: PathBuf,
    state: StateDir,
    key: PathBuf,
    other_key: PathBuf,
    crumb: PathBuf,
    base: String,
    head: String,
    /// A second head that changes `h/h.txt` (gate H, cache_scope hardware).
    head_h: String,
    wt_root: PathBuf,
}

const REPO: &str = "repo";

fn world() -> World {
    let root = Tmp::new("world");
    let r = root.path();
    let key = r.join("verify.key");
    let pubkey = bundle::keygen(&key).unwrap();
    let other_key = r.join("other.key");
    bundle::keygen(&other_key).unwrap();
    let crumb = r.join("crumb");
    write(
        r,
        "crumb",
        "#!/bin/sh\necho \"crumb $1: OK\"\nexit 0\n",
        true,
    );

    let origin = r.join("origin");
    fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "-b", "main"]);
    write(&origin, "tools/a.sh", "#!/bin/sh\necho PASS\n", true);
    write(&origin, "tools/h.sh", "#!/bin/sh\necho PASS\n", true);
    write(
        &origin,
        "tests/gates/A.gate",
        &gate("A", "a/", "tools/a.sh", ""),
        false,
    );
    write(
        &origin,
        "tests/gates/H.gate",
        &gate("H", "h/", "tools/h.sh", "cache_scope: hardware\n"),
        false,
    );
    write(&origin, "a/a.txt", "1\n", false);
    write(&origin, "h/h.txt", "1\n", false);
    write(
        &origin,
        ".aien/verify.pub",
        &format!("# trusted executors\n{pubkey}\n"),
        false,
    );
    write(&origin, ".crumb", "crumb: 1\n", false);
    git(&origin, &["add", "-A"]);
    git(&origin, &["commit", "-q", "-m", "base"]);
    let base = git(&origin, &["rev-parse", "HEAD"]);
    git(&origin, &["checkout", "-q", "-b", "topic"]);
    write(&origin, "a/a.txt", "2\n", false);
    git(&origin, &["add", "-A"]);
    git(&origin, &["commit", "-q", "-m", "change a"]);
    let head = git(&origin, &["rev-parse", "HEAD"]);
    git(&origin, &["checkout", "-q", "-b", "topic-h", "main"]);
    write(&origin, "h/h.txt", "2\n", false);
    git(&origin, &["add", "-A"]);
    git(&origin, &["commit", "-q", "-m", "change h"]);
    let head_h = git(&origin, &["rev-parse", "HEAD"]);
    git(&origin, &["checkout", "-q", "main"]);

    let clones = r.join("clones");
    fs::create_dir_all(&clones).unwrap();
    let clone = clones.join(REPO);
    git(&clones, &["clone", "-q", origin.to_str().unwrap(), REPO]);
    git(&clone, &["fetch", "-q", "--all"]);
    World {
        state: StateDir::new(&r.join("state")),
        wt_root: r.join("wt"),
        _root: root,
        origin,
        clone,
        key,
        other_key,
        crumb,
        base,
        head,
        head_h,
    }
}

impl World {
    fn config(&self) -> Config {
        let mut cfg = Config::new(
            self.state.clone(),
            &self.key,
            self.clone.parent().unwrap(),
            &self.wt_root,
        );
        cfg.crumb = Some(self.crumb.display().to_string());
        cfg.gh = self.crumb.display().to_string(); // never called unless github
        cfg
    }

    fn ask(&self, commit: &str) -> Ask {
        Ask {
            state: self.state.clone(),
            commit: commit.to_string(),
            base: "origin/main".to_string(),
            repository: None,
            release: false,
            operator: false,
            hardware_identity: String::new(),
            crumb: Some(self.crumb.display().to_string()),
            keys_override: None,
        }
    }

    fn verify(&self, ask: &Ask) -> verify::Report {
        verify::run(&self.clone, ask)
    }

    fn bundle_path(&self, sha: &str) -> PathBuf {
        self.state.bundle_path(REPO, sha)
    }

    fn read_bundle(&self, sha: &str) -> Value {
        serde_json::from_slice(&fs::read(self.bundle_path(sha)).unwrap()).unwrap()
    }

    fn write_bundle(&self, sha: &str, v: &Value) {
        let p = self.bundle_path(sha);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, canonical(v).unwrap()).unwrap();
    }
}

fn line<'a>(r: &'a verify::Report, prefix: &str) -> &'a str {
    r.lines
        .iter()
        .find(|l| l.starts_with(prefix))
        .map(String::as_str)
        .unwrap_or_else(|| panic!("no line {prefix:?} in {:#?}", r.lines))
}

fn assert_blocked(r: &verify::Report, why: &str) {
    assert!(!r.ready, "{why}: {:#?}", r.lines);
    assert_eq!(
        line(r, "MERGE ELIGIBILITY:"),
        "MERGE ELIGIBILITY: BLOCKED",
        "{why}"
    );
    assert_ne!(line(r, "Final verdict:"), "Final verdict: PASS", "{why}");
}

/// The happy path once, shared by the negative tests through `verified_world`.
fn verified_world() -> World {
    let w = world();
    let s = worker::run_once(&w.config(), REPO, &w.head).expect("worker");
    assert_eq!(s.verdict, Verdict::Pass, "{} {:?}", s.reason, s.notes);
    assert_eq!(s.bundle_verdict, "PASS");
    assert!(w.bundle_path(&w.head).exists());
    assert_eq!(s.publication, "local");
    w
}

#[test]
fn worker_then_verify_is_ready() {
    let w = verified_world();
    let r = w.verify(&w.ask(&w.head));
    assert_eq!(r.lines[0], "AIEN LOCAL VERIFICATION");
    assert_eq!(line(&r, "Evidence status:"), "Evidence status: VERIFIED");
    assert_eq!(line(&r, "Crumb status:"), "Crumb status: CURRENT");
    assert_eq!(
        line(&r, "Hardware status:"),
        "Hardware status: NOT_REQUIRED"
    );
    assert_eq!(line(&r, "Final verdict:"), "Final verdict: PASS");
    assert_eq!(line(&r, "Executed gates:"), "Executed gates: 1 (A)");
    assert_eq!(line(&r, "Build time:"), "Build time: n/a");
    assert!(line(&r, "Test time:").ends_with(" ms"));
    assert_eq!(
        line(&r, "MERGE ELIGIBILITY:"),
        "MERGE ELIGIBILITY: READY",
        "{:#?}",
        r.lines
    );
    assert!(r.ready);
    // The journal has the head, the worktree is gone, the clone's checkout is untouched.
    let j = fs::read_to_string(w.state.journal()).unwrap();
    assert!(j.contains(&w.head) && j.contains("\"publication\":\"local\""));
    assert!(!w.wt_root.join(REPO).join(&w.head).exists());
    assert_eq!(git(&w.clone, &["rev-parse", "HEAD"]), w.base);
    assert_eq!(git(&w.clone, &["status", "--porcelain"]), "");
    // Fresh crumb check leaves no worktree behind.
    assert!(!git(&w.clone, &["worktree", "list"]).contains("aien-verify-"));
    let _ = &w.origin;
}

#[test]
fn altered_receipt_is_rejected() {
    let w = verified_world();
    let b = w.read_bundle(&w.head);
    let digest = b["checks"][0]["receipt"].as_str().unwrap().to_string();
    let p = w
        .state
        .evidence(REPO, &w.head)
        .join(format!("{digest}.json"));
    let text = fs::read_to_string(&p).unwrap();
    assert!(text.contains("\"derived_verdict\":\"PASS\""));
    fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(
        &p,
        text.replace(
            "\"derived_verdict\":\"PASS\"",
            "\"derived_verdict\":\"FAIL\"",
        ),
    )
    .unwrap();
    let r = w.verify(&w.ask(&w.head));
    assert_blocked(&r, "altered receipt");
    assert!(
        r.lines.iter().any(|l| l.starts_with("Problem: A: receipt")),
        "{:#?}",
        r.lines
    );
}

#[test]
fn invalid_signature_is_rejected() {
    let w = verified_world();
    let mut b = w.read_bundle(&w.head);
    let sig = b["signature"]["sig"].as_str().unwrap().to_string();
    let flipped = format!(
        "{}{}",
        if sig.starts_with('0') { "1" } else { "0" },
        &sig[1..]
    );
    b["signature"]["sig"] = json!(flipped);
    w.write_bundle(&w.head, &b);
    let r = w.verify(&w.ask(&w.head));
    assert_blocked(&r, "bad signature");
    assert_eq!(line(&r, "Evidence status:"), "Evidence status: INVALID");
    assert!(line(&r, "Evidence note:").contains("BAD_SIGNATURE"));
}

#[test]
fn bundle_from_another_commit_is_rejected() {
    let w = verified_world();
    let b = w.read_bundle(&w.head);
    // Present the head's bundle as the other branch head's.
    w.write_bundle(&w.head_h, &b);
    let r = w.verify(&w.ask(&w.head_h));
    assert_blocked(&r, "other commit");
    assert!(
        line(&r, "Evidence note:").contains("COMMIT_MISMATCH"),
        "{:#?}",
        r.lines
    );
}

#[test]
fn missing_required_check_never_passes() {
    let w = verified_world();
    let mut b = w.read_bundle(&w.head);
    b["checks"] = json!([]);
    b.as_object_mut().unwrap().remove("signature");
    // Re-signed by the trusted key: the algebra, not the signature, must refuse it.
    let key = bundle::load_signing_key(&w.key).unwrap();
    let signed = bundle::sign(&b, &key).unwrap();
    assert_eq!(
        signed["verdict"], "PASS",
        "an empty bundle declares PASS vacuously"
    );
    w.write_bundle(&w.head, &signed);
    let r = w.verify(&w.ask(&w.head));
    assert_blocked(&r, "missing required check");
    assert!(
        line(&r, "Evidence note:").contains("MISSING_REQUIRED_CHECK: A"),
        "{:#?}",
        r.lines
    );
}

#[test]
fn hardware_cached_result_cannot_satisfy_release() {
    let w = world();
    // A hand-made bundle for the H head: H cached (PASS), signed by the trusted key.
    let wt = w.wt_root.join("h-plan");
    git(
        &w.clone,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            wt.to_str().unwrap(),
            &w.head_h,
        ],
    );
    let p = plan::compute(&wt, &Request::new(&w.base, &w.head_h)).unwrap();
    git(
        &w.clone,
        &["worktree", "remove", "--force", wt.to_str().unwrap()],
    );
    let h = p
        .entries
        .iter()
        .find(|e| e.gate == "H")
        .expect("H selected");
    let key = bundle::load_signing_key(&w.key).unwrap();
    let vk = key.verifying_key();
    let tree = git(&w.clone, &["rev-parse", &format!("{}^{{tree}}", w.head_h)]);
    let unsigned = json!({
        "schema": bundle::SCHEMA,
        "repository": plan::repository_name(&w.clone, None),
        "commit": w.head_h,
        "tree_digest": aien_test::evidence::sha256_hex(tree.as_bytes()),
        "graph_digest": p.graph_digest,
        "receipt_id_scheme": "sha256-canonical-json",
        "checks": [{
            "gate": "H", "check_id": h.check_id, "exactness": "EXACT", "required": true,
            "status": "cached", "verdict": "PASS", "backend": "host",
            "resolution": { "resolved_from": "f".repeat(64) }
        }],
        "verdict": "PASS",
        "executor": { "id": bundle::fingerprint(&vk), "class": "spark", "machine_digest": "0".repeat(64) },
        "started_utc": "2026-10-11T00:00:00Z",
        "finished_utc": "2026-10-11T00:00:02Z",
    });
    let signed = bundle::sign(&unsigned, &key).unwrap();
    w.write_bundle(&w.head_h, &signed);
    let plain = w.verify(&w.ask(&w.head_h));
    assert_eq!(
        line(&plain, "Evidence status:"),
        "Evidence status: VERIFIED",
        "{:#?}",
        plain.lines
    );
    assert_eq!(line(&plain, "Cached gates:"), "Cached gates: 1 (H)");
    assert_eq!(line(&plain, "Final verdict:"), "Final verdict: PASS");
    assert_eq!(line(&plain, "Total time:"), "Total time: 2000 ms");
    let mut release = w.ask(&w.head_h);
    release.release = true;
    let r = w.verify(&release);
    assert_blocked(&r, "hardware cache under --release");
    assert_eq!(line(&r, "Cached gates:"), "Cached gates: none");
    assert!(
        r.lines
            .iter()
            .any(|l| l.contains("cannot satisfy --release")),
        "{:#?}",
        r.lines
    );
}

#[test]
fn executor_not_in_verify_pub_is_rejected() {
    let w = verified_world();
    let mut b = w.read_bundle(&w.head);
    b.as_object_mut().unwrap().remove("signature");
    let other = bundle::load_signing_key(&w.other_key).unwrap();
    b["executor"]["id"] = json!(bundle::fingerprint(&other.verifying_key()));
    let signed = bundle::sign(&b, &other).unwrap();
    w.write_bundle(&w.head, &signed);
    let r = w.verify(&w.ask(&w.head));
    assert_blocked(&r, "untrusted executor");
    assert!(
        line(&r, "Evidence note:").contains("UNKNOWN_KEY"),
        "{:#?}",
        r.lines
    );
    // And with no trusted keys at all the answer is the same.
    let mut none = w.ask(&w.head);
    none.keys_override = Some(Vec::new());
    let r = w.verify(&none);
    assert_blocked(&r, "no keys");
}

#[test]
fn another_uid_cannot_read_the_key() {
    let w = world();
    let probe = Command::new("sudo")
        .args(["-n", "-u", "nobody", "true"])
        .output();
    match probe {
        Ok(o) if o.status.success() => {}
        _ => {
            println!("SKIP: sudo -n -u nobody is not available here");
            return;
        }
    }
    let mode = fs::metadata(&w.key).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let out = Command::new("sudo")
        .args(["-n", "-u", "nobody", "cat", w.key.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "nobody must not read the signing key"
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn github_down_queues_then_flushes() {
    let w = world();
    let fake = w.state.root.join("fake-gh");
    let log = w.state.root.join("gh.log");
    let down = w.state.root.join("gh.down");
    fs::create_dir_all(&w.state.root).unwrap();
    fs::write(
        &fake,
        format!(
            "#!/bin/sh\nif [ -e {} ]; then echo 'connect: network unreachable' >&2; exit 1; fi\necho \"$@\" >> {}\nexit 0\n",
            down.display(),
            log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(&down, "").unwrap();
    let mut cfg = w.config();
    cfg.github = true;
    cfg.gh = fake.display().to_string();
    let s = worker::run_once(&cfg, REPO, &w.head).expect("worker");
    assert_eq!(s.verdict, Verdict::Pass, "{}", s.reason);
    assert_eq!(s.publication, "pending");
    assert!(
        s.notes.iter().any(|n| n.starts_with("GitHub unreachable")),
        "{:?}",
        s.notes
    );
    let queued: Vec<_> = fs::read_dir(w.state.outbox_dir())
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(queued.len(), 1);
    assert!(fs::read_to_string(w.state.journal())
        .unwrap()
        .contains("\"publication\":\"pending\""));
    // Still down: nothing posted.
    assert_eq!(worker::flush_outbox(&cfg), (0, 1));
    // Back: posted, outbox empty, the status names the bundle and is a success.
    fs::remove_file(&down).unwrap();
    assert_eq!(worker::flush_outbox(&cfg), (1, 0));
    assert_eq!(
        fs::read_dir(w.state.outbox_dir())
            .unwrap()
            .flatten()
            .count(),
        0
    );
    let l = fs::read_to_string(&log).unwrap();
    assert!(l.contains(&format!("statuses/{}", w.head)), "{l}");
    assert!(
        l.contains("state=success") && l.contains("context=AIEN Verification (Spark)"),
        "{l}"
    );
    assert!(l.contains("bundle.json"), "{l}");
    assert!(fs::read_to_string(w.state.journal())
        .unwrap()
        .contains("flushed from outbox"));
}

#[test]
fn trigger_file_is_served_by_the_loop() {
    let w = world();
    let cfg = w.config();
    fs::create_dir_all(w.state.trigger_dir()).unwrap();
    fs::write(w.state.trigger_dir().join(format!("{REPO}.{}", w.head)), "").unwrap();
    let served = worker::run_loop(&cfg, Some(1));
    assert_eq!(served, 1);
    assert!(w.bundle_path(&w.head).exists());
    assert_eq!(
        fs::read_dir(w.state.trigger_dir())
            .unwrap()
            .flatten()
            .count(),
        0
    );
}

#[test]
fn bare_exec_runs_and_escapes_are_refused() {
    // `exec: true` (bare) runs from the PATH; `exec: ../x` is a manifest reject.
    let t = Tmp::new("bare");
    let r = t.path();
    git(r, &["init", "-q", "-b", "main"]);
    write(r, "tests/gates/T.gate", &gate("T", "a/", "true", ""), false);
    write(r, "a/a.txt", "1\n", false);
    git(r, &["add", "-A"]);
    git(r, &["commit", "-q", "-m", "init"]);
    let ev = Tmp::new("bare-ev");
    let opts = Options::new(ev.path().join("out"));
    let o = runner::run_gate(r, &r.join("tests/gates/T.gate"), &opts).unwrap();
    assert_eq!(o.verdict, Verdict::Pass, "{}", o.reason);
    for bad in ["../../usr/bin/env", "/usr/bin/env"] {
        write(r, "tests/gates/T.gate", &gate("T", "a/", bad, ""), false);
        git(r, &["commit", "-q", "-am", "bad exec"]);
        match runner::run_gate(r, &r.join("tests/gates/T.gate"), &opts) {
            Err(runner::RunError::BadManifest(m)) => assert!(m.contains("run.exec"), "{m}"),
            other => panic!("{bad}: expected a manifest reject, got {other:?}"),
        }
    }
}

#[test]
fn user_sandbox_when_the_helper_exists() {
    if !Path::new(aien_test::sandbox::EXEC_HELPER).exists() {
        println!(
            "SKIP: {} is not installed (INFRA lane)",
            aien_test::sandbox::EXEC_HELPER
        );
        return;
    }
    let t = Tmp::new("sandbox");
    let r = t.path();
    git(r, &["init", "-q", "-b", "main"]);
    write(r, "tools/a.sh", "#!/bin/sh\necho PASS\n", true);
    write(
        r,
        "tests/gates/A.gate",
        &gate("A", "a/", "tools/a.sh", ""),
        false,
    );
    write(r, "a/a.txt", "1\n", false);
    git(r, &["add", "-A"]);
    git(r, &["commit", "-q", "-m", "init"]);
    let ev = Tmp::new("sandbox-ev");
    let mut opts = Options::new(ev.path().join("out"));
    opts.sandbox = Sandbox::User;
    let o = runner::run_gate(r, &r.join("tests/gates/A.gate"), &opts).unwrap();
    println!("sandbox user: {} {}", o.verdict.as_str(), o.reason);
    assert_eq!(o.verdict, Verdict::Pass, "{}", o.reason);
}

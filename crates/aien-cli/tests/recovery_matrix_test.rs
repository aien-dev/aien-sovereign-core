//! Real-process recovery matrix for the ordinary compose -> grant -> execute -> ack
//! path (arch#162). Every case starts the real `aien daemon` and the real
//! `aien compose ...` CLI as OS processes, each with its OWN fresh
//! AIEN_RUNTIME_STATE_DIR, AIEN_COMPOSE_DIR (the desk key lives under it),
//! AIEN_PROVENANCE_DIR, socket and workspace. Crashes are SIGKILL (kill -9) of
//! the real process at a named fault-hold point of `aien compose execute`
//! (before_intent, after_intent, after_write), then SIGKILL of the daemon,
//! then a restart over the same state. Nothing is a clean shutdown or abort().
//!
//! How the grant is obtained: the ordinary `propose` step needs a trained model
//! to emit `filename: ...` text, and none ships with the repository. The desk
//! path (`ComposeApprovedProposal`, sovereign-core #249) commits an approved
//! proposal through the same production compose run (J-Space, AEGIS, World
//! commit, Cortex records) and the daemon itself writes the grant. Its task
//! report is the S3 report that `aien compose execute` reads, so S5 (intent,
//! write, ack, reconcile) is the unmodified ordinary one. The `authorize` step
//! is exercised separately where the daemon allows it (case R6).
//!
//! Needs a compose-linked CPU build of `aien-cli` with the `fault-hold` feature
//! (never a GPU build). Build once, then run (single documented command):
//!
//! ```text
//! AIEN_OMEGA_COMPOSE_DIR=<omega checkout at omega.lock> AIEN_PHYSICS_DIR=<physics> \
//!   AIEN_AIENOS_LOCK_REPO=<aienos clone holding aienos.lock> CARGO_TARGET_DIR=<dir> \
//!   cargo build --release -p aien-cli --features aien-cli/fault-hold
//! AIEN_BIN=<dir>/release/aien-cli cargo test -p aien-cli --test recovery_matrix_test \
//!   -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! Every case prints one line `MATRIX_RECEIPT {json}` (commit, binary sha256,
//! backend line, model sha256, case id, outcome) and, when AIEN_MATRIX_RECEIPTS
//! names a file, appends it there. The daemon must report the CPU reference
//! backend or the case stops (no GPU use here).
#[path = "../../aien-allen/tests/support/mod.rs"]
mod support;

use aien_allen::binding::pin_path;
use aien_runtime::approved::{
    approved_proposal_sha256, proposal_text, ApprovedComposeReport, ApprovedProposal,
    ApprovedRefusal,
};
use aien_runtime::approved_auth::{desk_key_path, DeskKey};
use aien_runtime::client::AienRuntimeClient;
use aien_runtime::control::{ChatTurn, ControlCommand, ControlResponse};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const PATH: &str = "NOTES.md";
const CONTENT: &str = "recovery matrix note\n";

fn sha(b: &[u8]) -> String {
    aien_omega_compose::hex(&Sha256::digest(b))
}

fn s(p: &Path) -> String {
    p.display().to_string()
}

#[derive(Debug)]
struct Down {
    code: Option<i32>,
    signal: Option<i32>,
    log: String,
}

struct Out {
    code: i32,
    json: Value,
    stdout: String,
    stderr: String,
}

impl Out {
    fn all(&self) -> String {
        format!("{} {}", self.stdout, self.stderr)
    }
}

struct Rig {
    case: &'static str,
    _tmp: tempfile::TempDir,
    root: PathBuf,
    bin: PathBuf,
    daemon: Option<Child>,
    nd: u32,
    env: Vec<(String, String)>,
    rt: tokio::runtime::Runtime,
    backend: String,
    model: String,
    detail: String,
}

impl Rig {
    fn new(case: &'static str) -> Rig {
        let bin = PathBuf::from(std::env::var("AIEN_BIN").unwrap_or_else(|_| {
            panic!("AIEN_BIN must name the compose-linked CPU aien-cli (see the file header)")
        }));
        // A short path: unix socket paths are limited to 108 bytes.
        let tmp = tempfile::Builder::new()
            .prefix("rm-")
            .tempdir_in("/tmp")
            .unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        for d in ["ws", "state", "home", "prov"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        DeskKey::create(&desk_key_path(&root.join("compose"))).unwrap();
        Rig {
            case,
            _tmp: tmp,
            root,
            bin,
            daemon: None,
            nd: 0,
            env: Vec::new(),
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            backend: "not started".into(),
            model: "none".into(),
            detail: String::new(),
        }
    }

    fn sock(&self) -> PathBuf {
        self.root.join("s")
    }
    fn compose(&self) -> PathBuf {
        self.root.join("compose")
    }
    fn ws(&self) -> PathBuf {
        self.root.join("ws")
    }

    fn cmd_env(&self, c: &mut Command) {
        c.current_dir(&self.root)
            .env("HOME", self.root.join("home"))
            .env("NO_COLOR", "1")
            .env("AIEN_RUNTIME_SOCK", self.sock())
            .env("AIEN_RUNTIME_STATE_DIR", self.root.join("state"))
            .env("AIEN_COMPOSE_DIR", self.compose())
            .env("AIEN_PROVENANCE_DIR", self.root.join("prov"))
            .env("AIEN_REQUIRE_CHECKPOINT", "0");
        for k in [
            "AIEN_OMEGA_DIR",
            "AIEN_OMEGA_GPU_LIB",
            "AIEN_REQUIRE_BLACKWELL",
            "AIEN_GPU_BACKEND",
            "AIEN_MODEL_PATH",
            "AIEN_TOKENIZER_PATH",
            "AIEN_MODEL_DIR",
            "AIEN_ALLEN_SUBJECT",
            "AIEN_ALLEN_ADOPT",
            "AIEN_FAULT_HOLD",
            "AIEN_FAULT_HOLD_FILE",
        ] {
            c.env_remove(k);
        }
        for (k, v) in &self.env {
            c.env(k, v);
        }
    }

    fn down_of(&mut self, mut child: Child, log: &Path) -> Down {
        let st = child.wait().unwrap();
        self.daemon = None;
        Down {
            code: st.code(),
            signal: st.signal(),
            log: std::fs::read_to_string(log).unwrap_or_default(),
        }
    }

    fn start(&mut self) -> Result<(), Down> {
        self.start_with(&[])
    }

    /// Start the daemon; `once` is added to this start only. Err when the
    /// process exits before it is serving (a refusal).
    fn start_with(&mut self, once: &[(&str, &str)]) -> Result<(), Down> {
        assert!(self.daemon.is_none(), "a daemon is already running");
        self.nd += 1;
        let log = self.root.join(format!("daemon-{}.log", self.nd));
        let f = std::fs::File::create(&log).unwrap();
        let _ = std::fs::remove_file(self.sock());
        let mut c = Command::new(&self.bin);
        c.arg("daemon");
        self.cmd_env(&mut c);
        for (k, v) in once {
            c.env(k, v);
        }
        c.process_group(0)
            .stdin(Stdio::null())
            .stdout(f.try_clone().unwrap())
            .stderr(f);
        let child = c.spawn().expect("spawn daemon");
        self.daemon = Some(child);
        for _ in 0..1200 {
            std::thread::sleep(Duration::from_millis(50));
            let exited = self.daemon.as_mut().unwrap().try_wait().unwrap().is_some();
            if exited {
                let child = self.daemon.take().unwrap();
                return Err(self.down_of(child, &log));
            }
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            if self.sock().exists() && text.contains("Replay reconcile:") {
                let line = text
                    .lines()
                    .find(|l| l.contains("Backend:"))
                    .unwrap_or("no Backend line")
                    .trim()
                    .to_string();
                if !line.contains("CPU-reference") {
                    self.kill_daemon();
                    panic!(
                        "daemon is not the CPU reference build ({line}); no GPU use allowed here"
                    );
                }
                self.backend = line;
                if let Some(m) = text.lines().find(|l| l.contains("model_sha256=")) {
                    if let Some(i) = m.find("model_sha256=") {
                        self.model = m[i + 13..].split([',', ')']).next().unwrap().to_string();
                    }
                }
                return Ok(());
            }
        }
        self.kill_daemon();
        panic!("daemon did not come up in 60 s: {}", self.root.display());
    }

    /// SIGKILL the daemon this rig started (std `Child::kill` is SIGKILL).
    fn kill_daemon(&mut self) {
        if let Some(mut c) = self.daemon.take() {
            let _ = c.kill();
            let st = c.wait().unwrap();
            assert!(
                st.signal() == Some(9) || st.code().is_some(),
                "daemon end: {st:?}"
            );
        }
        let _ = std::fs::remove_file(self.sock());
    }

    fn cli_cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(&self.bin);
        c.arg("compose").args(args);
        self.cmd_env(&mut c);
        c
    }

    fn cli(&self, args: &[&str]) -> Out {
        let o = self.cli_cmd(args).output().expect("run aien compose");
        let stdout = String::from_utf8_lossy(&o.stdout).to_string();
        let json = stdout
            .lines()
            .rev()
            .find_map(|l| serde_json::from_str::<Value>(l).ok())
            .unwrap_or_else(|| json!({"unparsed": stdout}));
        Out {
            code: o.status.code().unwrap_or(-1),
            json,
            stdout,
            stderr: String::from_utf8_lossy(&o.stderr).to_string(),
        }
    }

    /// Run `aien compose <args>` with AIEN_FAULT_HOLD=`point`, wait until it
    /// holds there, then SIGKILL it. Returns the held process's stderr.
    fn cli_killed_at(&self, args: &[&str], point: &str) {
        let hold = self.root.join(format!("hold-{point}"));
        let _ = std::fs::remove_file(&hold);
        let mut c = self.cli_cmd(args);
        c.env("AIEN_FAULT_HOLD", point)
            .env("AIEN_FAULT_HOLD_FILE", &hold)
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(self.root.join("held.out")).unwrap())
            .stderr(std::fs::File::create(self.root.join("held.err")).unwrap());
        let mut child = c.spawn().expect("spawn held cli");
        let mut pid = None;
        for _ in 0..1200 {
            std::thread::sleep(Duration::from_millis(50));
            if let Ok(t) = std::fs::read_to_string(&hold) {
                if let Some(p) = t
                    .split_whitespace()
                    .next()
                    .and_then(|p| p.parse::<u32>().ok())
                {
                    pid = Some(p);
                    break;
                }
            }
            if child.try_wait().unwrap().is_some() {
                panic!(
                    "cli exited before the hold point {point}: out={} err={}",
                    std::fs::read_to_string(self.root.join("held.out")).unwrap_or_default(),
                    std::fs::read_to_string(self.root.join("held.err")).unwrap_or_default()
                );
            }
        }
        let pid = pid.unwrap_or_else(|| {
            let _ = child.kill();
            panic!("cli never reached hold point {point}")
        });
        assert_eq!(pid, child.id(), "the hold file names our child");
        child.kill().unwrap();
        let st = child.wait().unwrap();
        assert_eq!(
            st.signal(),
            Some(9),
            "{point}: killed by SIGKILL, got {st:?}"
        );
    }

    fn send(&self, cmd: ControlCommand) -> ControlResponse {
        let client = AienRuntimeClient::new(self.sock());
        self.rt
            .block_on(client.send_command(cmd))
            .expect("daemon answers")
    }

    /// The desk approves (path, content); the daemon commits it and writes the
    /// grant. Returns the report or the named refusal.
    fn approve(
        &self,
        req: &str,
        path: &str,
        content: &str,
    ) -> Result<ApprovedComposeReport, Box<ApprovedRefusal>> {
        let mut p = ApprovedProposal {
            request_id: req.into(),
            trace_id: format!("trace-{req}"),
            approval_id: format!("appr-{req}"),
            approver: "interplane-host".into(),
            path: path.into(),
            content: content.into(),
            approved_proposal_sha256: approved_proposal_sha256(path, content),
            content_sha256: sha(content.as_bytes()),
            approval_mac: String::new(),
            requirements: Some(String::new()),
            requirements_mac: String::new(),
            requirements_base: None,
        };
        DeskKey::load(&desk_key_path(&self.compose()))
            .unwrap()
            .seal(&mut p, &self.ws());
        assert!(proposal_text(path, content).is_ok());
        match self.send(ControlCommand::ComposeApprovedProposal {
            proposal: p,
            workspace: s(&self.ws()),
        }) {
            ControlResponse::ComposeApprovedResult(r) => Ok(*r),
            ControlResponse::ComposeApprovedRefused(r) => Err(r),
            other => panic!("approve: {other:?}"),
        }
    }

    /// Approve and return (S3 report file for the CLI, the daemon's grant id).
    fn prepare(&self, req: &str, path: &str, content: &str) -> (PathBuf, u64) {
        let r = self
            .approve(req, path, content)
            .unwrap_or_else(|e| panic!("approve refused: {e:?}"));
        assert_eq!(r.state, "COMMITTED");
        let grant = r.approved_grant.expect("the daemon wrote the grant");
        let task = serde_json::to_value(r.task.expect("task report")).unwrap();
        let f = self.root.join(format!("S3-{req}.json"));
        std::fs::write(&f, json!({"report": task}).to_string()).unwrap();
        (f, grant)
    }

    fn exec_args<'a>(&'a self, report: &'a Path, grant: &'a str) -> Vec<String> {
        vec![
            "execute".into(),
            "--report".into(),
            s(report),
            "--workspace".into(),
            s(&self.ws()),
            "--authorization".into(),
            grant.into(),
        ]
    }

    fn execute(&self, report: &Path, grant: u64) -> Out {
        let g = grant.to_string();
        let a = self.exec_args(report, &g);
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        self.cli(&a)
    }

    fn execute_killed_at(&self, report: &Path, grant: u64, point: &str) {
        let g = grant.to_string();
        let a = self.exec_args(report, &g);
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        self.cli_killed_at(&a, point);
    }

    fn intents(&self) -> Vec<Value> {
        let o = self.cli(&["effects"]);
        assert_eq!(o.code, 0, "effects: {}", o.all());
        o.json["ledger"]["intents"].as_array().cloned().unwrap()
    }

    fn states(&self) -> Vec<String> {
        self.intents()
            .iter()
            .map(|i| i["state"].as_str().unwrap().to_string())
            .collect()
    }

    fn file(&self, rel: &str) -> Option<(Vec<u8>, u64, i64, i64)> {
        let p = self.ws().join(rel);
        let m = std::fs::metadata(&p).ok()?;
        Some((
            std::fs::read(&p).unwrap(),
            m.ino(),
            m.mtime(),
            m.mtime_nsec(),
        ))
    }

    /// Kill the daemon with SIGKILL and start a fresh one over the same state.
    fn crash_and_restart(&mut self) {
        self.kill_daemon();
        self.start()
            .unwrap_or_else(|d| panic!("restart: {}", d.log));
    }

    /// Everything the daemon recorded, as text (for "never claims X" checks).
    fn all_records_text(&self) -> String {
        let o = self.cli(&["recall"]);
        o.json["recall"]["host"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|r| r["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let outcome = if std::thread::panicking() {
            "FAIL"
        } else {
            "PASS"
        };
        self.kill_daemon();
        let commit = Command::new("git")
            .args([
                "-C",
                env!("CARGO_MANIFEST_DIR"),
                "rev-parse",
                "--short=12",
                "HEAD",
            ])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        let bin_sha = std::fs::read(&self.bin)
            .map(|b| sha(&b))
            .unwrap_or_default();
        let line = json!({"case": self.case, "outcome": outcome, "commit": commit,
            "bin_sha256": bin_sha, "backend": self.backend, "model_sha256": self.model,
            "detail": self.detail});
        println!("MATRIX_RECEIPT {line}");
        if let Ok(f) = std::env::var("AIEN_MATRIX_RECEIPTS") {
            use std::io::Write;
            if let Ok(mut h) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(f)
            {
                let _ = writeln!(h, "{line}");
            }
        }
    }
}

fn names_error(o: &Out, any: &[&str]) {
    let t = o.all();
    assert!(
        any.iter().any(|n| t.contains(n)),
        "expected one of {any:?} in: {t}"
    );
}

/// R1: killed before the intent is opened.
#[test]
#[ignore = "needs AIEN_BIN: compose-linked CPU build with fault-hold"]
fn r1_crash_before_commit() {
    let mut r = Rig::new("R1");
    r.start().unwrap();
    let (rep, grant) = r.prepare("r1", PATH, CONTENT);
    r.execute_killed_at(&rep, grant, "before_intent");
    r.crash_and_restart();
    assert!(r.file(PATH).is_none(), "no file written");
    assert!(r.intents().is_empty(), "no intent, so no grant spent");
    let rc = r.cli(&["reconcile"]);
    assert_eq!(rc.code, 0, "{}", rc.all());
    let outcomes = rc.json["reconcile"]["outcomes"].as_array().unwrap();
    assert!(
        outcomes.iter().all(|o| o["state"] != "DONE") && rc.json["reconcile"]["checked"] == 0,
        "reconcile reports nothing: {}",
        rc.json
    );
    // The grant is still unspent: the same grant now does the effect, once.
    let ok = r.execute(&rep, grant);
    assert_eq!(ok.json["state"], "DONE", "{}", ok.all());
    assert_eq!(r.file(PATH).unwrap().0, CONTENT.as_bytes());
    let again = r.execute(&rep, grant);
    assert_ne!(again.code, 0);
    names_error(&again, &["AlreadySpent"]);
    assert_eq!(r.states(), vec!["DONE"]);
    r.detail =
        "SIGKILL at before_intent; restart; nothing done; grant unspent then spent once".into();
}

/// R2: killed after the intent is durable, before the write.
#[test]
#[ignore = "needs AIEN_BIN: compose-linked CPU build with fault-hold"]
fn r2_crash_after_intent_before_write() {
    let mut r = Rig::new("R2");
    r.start().unwrap();
    let (rep, grant) = r.prepare("r2", PATH, CONTENT);
    r.execute_killed_at(&rep, grant, "after_intent");
    r.crash_and_restart();
    assert!(r.file(PATH).is_none(), "the write never happened");
    let st = r.states();
    assert_eq!(st.len(), 1, "{st:?}");
    assert!(
        st[0] == "NOT_DONE" || st[0] == "UNRESOLVED",
        "never DONE for an effect that did not happen, got {st:?}"
    );
    let rc = r.cli(&["reconcile"]);
    assert_eq!(rc.code, 0, "{}", rc.all());
    assert!(
        rc.json["reconcile"]["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["state"] != "DONE"),
        "{}",
        rc.json
    );
    // Re-executing the same grant is refused and writes nothing.
    let again = r.execute(&rep, grant);
    assert_ne!(again.code, 0, "{}", again.all());
    names_error(&again, &["AlreadySpent", "ReconciliationRequired"]);
    assert!(r.file(PATH).is_none());
    assert_eq!(r.intents().len(), 1, "no second intent");
    r.detail = format!("SIGKILL at after_intent; restart settled {}", st[0]);
}

/// R3: killed after the write, before the ack.
#[test]
#[ignore = "needs AIEN_BIN: compose-linked CPU build with fault-hold"]
fn r3_crash_after_write_before_ack() {
    let mut r = Rig::new("R3");
    r.start().unwrap();
    let (rep, grant) = r.prepare("r3", PATH, CONTENT);
    r.execute_killed_at(&rep, grant, "after_write");
    let before = r.file(PATH).expect("the write happened before the crash");
    assert_eq!(before.0, CONTENT.as_bytes());
    r.crash_and_restart();
    // The world matches content_sha256 inside the workspace: DONE.
    assert_eq!(r.states(), vec!["DONE"]);
    let again = r.execute(&rep, grant);
    assert_ne!(again.code, 0, "{}", again.all());
    names_error(&again, &["AlreadySpent"]);
    let after = r.file(PATH).unwrap();
    assert_eq!(
        (before.1, before.2, before.3),
        (after.1, after.2, after.3),
        "same inode and mtime: the file was written once"
    );
    assert_eq!(r.intents().len(), 1);
    r.detail = "SIGKILL at after_write; restart reconciled DONE; no second write".into();
}

/// R3b: same crash, but the file is changed while nothing is running.
#[test]
#[ignore = "needs AIEN_BIN: compose-linked CPU build with fault-hold"]
fn r3b_crash_after_write_then_file_modified_is_unresolved() {
    let mut r = Rig::new("R3b");
    r.start().unwrap();
    let (rep, grant) = r.prepare("r3b", PATH, CONTENT);
    r.execute_killed_at(&rep, grant, "after_write");
    r.kill_daemon();
    std::fs::write(r.ws().join(PATH), "someone else wrote this\n").unwrap();
    r.start().unwrap_or_else(|d| panic!("restart: {}", d.log));
    let st = r.states();
    assert_eq!(st, vec!["UNRESOLVED"], "never DONE for changed bytes");
    let rc = r.cli(&["reconcile"]);
    assert!(
        rc.json["reconcile"]["outcomes"]
            .as_array()
            .map(|a| a.iter().all(|o| o["state"] != "DONE"))
            .unwrap_or(true),
        "{}",
        rc.json
    );
    assert_eq!(r.states(), vec!["UNRESOLVED"]);
    assert_eq!(
        std::fs::read_to_string(r.ws().join(PATH)).unwrap(),
        "someone else wrote this\n",
        "the daemon did not overwrite the modified file"
    );
    r.detail = "SIGKILL at after_write; file modified; restart settled UNRESOLVED".into();
}

/// R4: damaged state is refused with a named error, never reset.
#[test]
#[ignore = "needs AIEN_BIN: compose-linked CPU build with fault-hold"]
fn r4_corrupted_state_is_refused_not_reinitialised() {
    // (a) the compose ledger (Cortex journal), truncated between runs.
    let mut r = Rig::new("R4");
    r.start().unwrap();
    let (rep, grant) = r.prepare("r4", PATH, CONTENT);
    assert_eq!(r.execute(&rep, grant).json["state"], "DONE");
    r.kill_daemon();
    let cx = r.compose().join("cortex.cx");
    let len = std::fs::metadata(&cx).unwrap().len();
    let f = std::fs::OpenOptions::new().write(true).open(&cx).unwrap();
    f.set_len(len / 2).unwrap();
    drop(f);
    let damaged = std::fs::read(&cx).unwrap();
    let verdict = match r.start() {
        Err(d) => {
            assert_ne!(d.code, Some(0), "{}", d.log);
            assert!(!d.log.is_empty(), "a refusal says something");
            format!(
                "daemon refused to start (code {:?}): {}",
                d.code,
                d.log.lines().last().unwrap_or("")
            )
        }
        Ok(()) => {
            // Serving is allowed only if every compose command is refused.
            let o = r.cli(&["recall"]);
            assert!(
                o.json["ok"] == json!(false) && o.json["error"].is_string(),
                "damaged ledger answered as if healthy: {}",
                o.all()
            );
            let e = o.json["error"].as_str().unwrap().to_string();
            let o2 = r.cli(&["effects"]);
            assert_ne!(o2.code, 0, "effects must not succeed: {}", o2.all());
            format!("compose refused with a named error: {e}")
        }
    };
    assert_eq!(
        std::fs::read(&cx).unwrap(),
        damaged,
        "the damaged ledger was not silently reinitialised or repaired"
    );
    assert!(
        r.file(PATH).unwrap().0 == CONTENT.as_bytes(),
        "committed result untouched"
    );
    r.detail = verdict;
    r.kill_daemon();

    // (b) the idempotency state file: damaged -> the daemon refuses to start.
    let mut r2 = Rig::new("R4b");
    std::fs::write(
        r2.root.join("state/processed_operations.json"),
        b"{not json",
    )
    .unwrap();
    let d = match r2.start() {
        Err(d) => d,
        Ok(()) => panic!("daemon started over damaged idempotency state"),
    };
    assert_eq!(d.code, Some(1), "{}", d.log);
    assert!(
        d.log.contains("idempotency state") && d.log.contains("damaged"),
        "{}",
        d.log
    );
    assert_eq!(
        std::fs::read(r2.root.join("state/processed_operations.json")).unwrap(),
        b"{not json",
        "not reset"
    );
    r2.detail = "idempotency state damaged: Fatal named, exit 1, file untouched".into();
}

/// R5: the CPU build says so; GPU demands are refused, never silently served by the CPU.
#[test]
#[ignore = "needs AIEN_BIN: compose-linked CPU build with fault-hold"]
fn r5_missing_gpu_is_honest() {
    let mut r = Rig::new("R5");
    r.start().unwrap();
    assert!(r.backend.contains("CPU-reference"), "{}", r.backend);
    assert!(r.backend.contains("explicit fallback"), "{}", r.backend);
    let log = std::fs::read_to_string(r.root.join("daemon-1.log")).unwrap();
    assert!(
        !log.contains("Omega CTA budget:") && !log.contains("Omega marker spin:"),
        "no GPU engine lines in a CPU build: {log}"
    );
    let (rep, grant) = r.prepare("r5", PATH, CONTENT);
    let done = r.execute(&rep, grant);
    assert_eq!(done.json["state"], "DONE", "{}", done.all());
    // No receipt, record or CLI output claims GPU execution.
    let mut text = r.all_records_text() + &done.all();
    for e in std::fs::read_dir(r.root.join("prov")).unwrap().flatten() {
        if e.path().is_file() {
            text += &String::from_utf8_lossy(&std::fs::read(e.path()).unwrap());
        }
    }
    let low = text.to_lowercase();
    for w in ["gpu", "blackwell", "gb10", "omega_gpu", "cuda"] {
        assert!(!low.contains(w), "a receipt or record mentions {w:?}");
    }
    r.kill_daemon();
    // Demanding the GPU on a build without it is a refusal, not a CPU run.
    for (k, v) in [
        ("AIEN_REQUIRE_BLACKWELL", "1"),
        ("AIEN_GPU_BACKEND", "omega"),
    ] {
        let d = match r.start_with(&[(k, v)]) {
            Err(d) => d,
            Ok(()) => panic!("daemon served with {k}={v} on a CPU build"),
        };
        assert_eq!(d.code, Some(1), "{k}: {}", d.log);
        assert!(d.log.contains("GB10 GPU is required"), "{k}: {}", d.log);
        assert!(!r.sock().exists());
    }
    r.detail =
        "CPU-reference reported; no GPU claim in records; GPU demand refused (exit 1)".into();
}

/// R6: stale, spent, revoked and replayed permission.
#[test]
#[ignore = "needs AIEN_BIN: compose-linked CPU build with fault-hold"]
fn r6_stale_permission_is_not_spendable() {
    let mut r = Rig::new("R6");
    r.start().unwrap();
    // (a) minted by daemon A, spent, kill -9, daemon B: AlreadySpent, no second write.
    let (rep, grant) = r.prepare("r6a", PATH, CONTENT);
    assert_eq!(r.execute(&rep, grant).json["state"], "DONE");
    let first = r.file(PATH).unwrap();
    r.crash_and_restart();
    let again = r.execute(&rep, grant);
    assert_ne!(again.code, 0, "{}", again.all());
    names_error(&again, &["AlreadySpent"]);
    let second = r.file(PATH).unwrap();
    assert_eq!((first.1, first.2, first.3), (second.1, second.2, second.3));
    assert_eq!(r.intents().len(), 1);

    // (b) a revoked grant stays unusable, also across a kill -9 restart.
    let (rep_b, grant_b) = r.prepare("r6b", "B.md", CONTENT);
    let rv = r.cli(&[
        "revoke",
        "--authorization",
        &grant_b.to_string(),
        "--approver",
        "drake",
    ]);
    assert_eq!(rv.code, 0, "{}", rv.all());
    r.crash_and_restart();
    let o = r.execute(&rep_b, grant_b);
    assert_ne!(o.code, 0, "{}", o.all());
    names_error(&o, &["Revoked"]);
    assert!(r.file("B.md").is_none());

    // (c) a grant older than an operator stop stays stale after resume.
    let (rep_c, grant_c) = r.prepare("r6c", "C.md", CONTENT);
    assert_eq!(r.cli(&["stop", "--approver", "drake"]).code, 0);
    assert_eq!(r.cli(&["resume", "--approver", "drake"]).code, 0);
    let o = r.execute(&rep_c, grant_c);
    assert_ne!(o.code, 0, "{}", o.all());
    names_error(&o, &["Stale"]);
    assert!(r.file("C.md").is_none());

    // (d) replaying the same desk approval mints no second grant and runs nothing.
    let before = r.all_records_text().matches("approved_grant").count();
    match r.approve("r6a", PATH, CONTENT) {
        Ok(rep) => assert!(
            rep.approved_grant.is_none() && rep.task.is_none(),
            "a replay returned a fresh grant: {rep:?}"
        ),
        Err(e) => assert!(e.refused_by.contains("REFUSED"), "{e:?}"),
    }
    assert_eq!(
        r.all_records_text().matches("approved_grant").count(),
        before,
        "a replay wrote a grant record"
    );
    let o = r.execute(&rep, grant);
    assert_ne!(o.code, 0);
    assert_eq!(r.intents().len(), 1, "still exactly one effect");

    // (e) the ordinary `authorize` step on a committed proposal that already has a
    // grant: record what the daemon does; it must not open a second effect.
    let rep_json = std::fs::read_to_string(&rep).unwrap();
    let report: Value = serde_json::from_str(&rep_json).unwrap();
    let promo = report["report"]["cx_promotion"]
        .as_u64()
        .unwrap()
        .to_string();
    let _ = promo;
    let au = r.cli(&[
        "authorize",
        "--report",
        &s(&rep),
        "--workspace",
        &s(&r.ws()),
        "--approver",
        "drake",
    ]);
    r.detail = format!(
        "a-d PASS; authorize on an approved report: code {} {}",
        au.code,
        au.all().chars().take(160).collect::<String>()
    );
    if au.code == 0 {
        let g2 = au.json["authorization"]["id"].as_u64().expect("grant id");
        let o = r.execute(&rep, g2);
        assert_ne!(
            o.json["state"],
            "DONE",
            "a second effect for the same file: {}",
            o.all()
        );
    }
    assert_eq!(r.states(), vec!["DONE"], "exactly one effect in the ledger");
}

/// Model B: the same tiny checkpoint with one weight byte changed (a different file digest).
fn make_models(root: &Path) -> (PathBuf, PathBuf) {
    let src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../aien-inference-abi/fixtures/openwaldo-byte");
    let a = root.join("model-a");
    let b = root.join("model-b");
    for d in [&a, &b] {
        std::fs::create_dir_all(d).unwrap();
        for f in [
            "config.json",
            "tokenizer.json",
            "model.safetensors",
            "generation_config.json",
        ] {
            let p = src.join(f);
            if p.exists() {
                std::fs::copy(&p, d.join(f)).unwrap();
            }
        }
    }
    // Room for the daemon warm-up prompt and a chat turn (the fixture declares 16 positions).
    for d in [&a, &b] {
        let mut c: Value =
            serde_json::from_slice(&std::fs::read(d.join("config.json")).unwrap()).unwrap();
        c["max_position_embeddings"] = json!(2048);
        std::fs::write(d.join("config.json"), c.to_string()).unwrap();
    }
    // A Zephyr-layout chat template, so the daemon accepts a chat turn on a tiny test model.
    let tc = json!({"chat_template":
        "{% for m in messages %}<|user|>\n{{ m.content }}</s>\n{% endfor %}<|assistant|>\n"});
    for d in [&a, &b] {
        std::fs::write(d.join("tokenizer_config.json"), tc.to_string()).unwrap();
    }
    let mut w = std::fs::read(b.join("model.safetensors")).unwrap();
    let n = w.len();
    w[n - 1] ^= 0x01;
    std::fs::write(b.join("model.safetensors"), w).unwrap();
    (a, b)
}

fn model_env(r: &mut Rig, dir: &Path) {
    r.env
        .retain(|(k, _)| k != "AIEN_MODEL_PATH" && k != "AIEN_TOKENIZER_PATH");
    r.env
        .push(("AIEN_MODEL_PATH".into(), s(&dir.join("model.safetensors"))));
    r.env
        .push(("AIEN_TOKENIZER_PATH".into(), s(&dir.join("tokenizer.json"))));
}

/// Hex lineage (record 1 digest) and machine id of the home the daemon opened.
fn home_lineage(r: &Rig) -> [u8; 32] {
    let o = r.cli(&["recall", "--ids", "1"]);
    assert_eq!(o.code, 0, "{}", o.all());
    aien_allen::unhex32(o.json["recall"]["cited"][0]["digest"].as_str().unwrap())
        .expect("record 1 digest is 32 bytes hex")
}

/// One generation by the daemon; returns its generation record id.
fn generate(r: &Rig) -> u64 {
    let client = AienRuntimeClient::new(r.sock());
    let (_, rec) =
        r.rt.block_on(client.stream_turn_recorded(
            vec![ChatTurn {
                role: "user".into(),
                content: "hi".into(),
            }],
            4,
            0.0,
        ))
        .expect("turn");
    rec.expect("the daemon wrote a generation record")
}

fn record_text(r: &Rig, id: u64) -> Value {
    let o = r.cli(&["recall", "--ids", &id.to_string()]);
    serde_json::from_str(o.json["recall"]["cited"][0]["text"].as_str().unwrap()).unwrap()
}

/// R7: the daemon restarts with a different model.
#[test]
#[ignore = "needs AIEN_BIN: compose-linked CPU build with fault-hold"]
fn r7_different_model_on_restart() {
    let mut r = Rig::new("R7");
    let (ma, mb) = make_models(&r.root);
    model_env(&mut r, &ma);
    // Create the home, then engage ALLEN: a subject bound to this home's lineage, adopted once.
    r.start().unwrap();
    let lineage = home_lineage(&r);
    r.kill_daemon();
    let mut f = support::fx("r7");
    f.cortex = lineage;
    let chain = support::chain(&f, 2);
    let subj = support::write_head(&r.root.join("subject.bin"), chain.last().unwrap());
    r.env.push(("AIEN_ALLEN_SUBJECT".into(), s(&subj)));
    let agent = aien_allen::hex(&f.agent);
    r.start_with(&[("AIEN_ALLEN_ADOPT", &agent)])
        .unwrap_or_else(|d| panic!("ALLEN adopt: {}", d.log));
    let model_a = r.model.clone();
    assert_ne!(model_a, "none", "the real checkpoint loaded with a digest");
    let pin_bytes = std::fs::read(pin_path(&r.compose())).expect("pin written");
    let (rep, grant) = r.prepare("r7", PATH, CONTENT);
    assert_eq!(r.execute(&rep, grant).json["state"], "DONE");
    let g1 = generate(&r);
    let rec1 = record_text(&r, g1);
    assert_eq!(rec1["model_sha256"], json!(model_a));
    let done = r.file(PATH).unwrap();

    // kill -9, restart with model B.
    r.kill_daemon();
    model_env(&mut r, &mb);
    r.start()
        .unwrap_or_else(|d| panic!("restart on model B: {}", d.log));
    assert_ne!(
        r.model, model_a,
        "the restart loaded a different file digest"
    );
    let g2 = generate(&r);
    let rec2 = record_text(&r, g2);
    assert_eq!(rec2["model_sha256"], json!(r.model));
    assert_ne!(
        rec1["model_sha256"], rec2["model_sha256"],
        "generation records show the change"
    );
    // Earlier committed results remain; identity unchanged.
    assert_eq!(r.states(), vec!["DONE"]);
    let now = r.file(PATH).unwrap();
    assert_eq!((done.0.clone(), done.1, done.2), (now.0, now.1, now.2));
    assert_eq!(
        pin_bytes,
        std::fs::read(pin_path(&r.compose())).unwrap(),
        "ALLEN pin unchanged"
    );
    let log2 = std::fs::read_to_string(r.root.join("daemon-3.log")).unwrap();
    assert!(
        log2.contains(&format!("ALLEN: engaged agent={agent} head_sequence=2")),
        "ALLEN identity unchanged: {log2}"
    );
    r.detail = format!(
        "model {} -> {}; ALLEN agent {} unchanged",
        &model_a[..12],
        &r.model[..12],
        &agent[..12]
    );
}

/// R8: a compose home pinned to another agent is refused; nothing executes.
#[test]
#[ignore = "needs AIEN_BIN: compose-linked CPU build with fault-hold"]
fn r8_foreign_identity_is_refused() {
    let mut r = Rig::new("R8");
    r.start().unwrap();
    let lineage = home_lineage(&r);
    r.kill_daemon();
    // Agent A adopts the home.
    let mut fa = support::fx("agent-a");
    fa.cortex = lineage;
    let subj_a = support::write_head(
        &r.root.join("a.bin"),
        support::chain(&fa, 2).last().unwrap(),
    );
    let agent_a = aien_allen::hex(&fa.agent);
    r.start_with(&[
        ("AIEN_ALLEN_SUBJECT", &s(&subj_a)),
        ("AIEN_ALLEN_ADOPT", &agent_a),
    ])
    .unwrap_or_else(|d| panic!("adopt: {}", d.log));
    let pin = std::fs::read(pin_path(&r.compose())).unwrap();
    r.kill_daemon();
    // Agent B (another logical agent, same lineage claim) starts over A's home.
    let mut fb = support::fx("agent-b");
    fb.cortex = lineage;
    let subj_b = support::write_head(
        &r.root.join("b.bin"),
        support::chain(&fb, 2).last().unwrap(),
    );
    let d = match r.start_with(&[("AIEN_ALLEN_SUBJECT", &s(&subj_b))]) {
        Err(d) => d,
        Ok(()) => {
            // The home opens lazily: the first compose call must be fatal.
            let _ = r.cli(&["recall"]);
            std::thread::sleep(Duration::from_millis(500));
            let child = r.daemon.take().unwrap();
            let log = r.root.join("daemon-3.log");
            r.down_of(child, &log)
        }
    };
    assert_eq!(
        d.code,
        Some(aien_allen::EXIT_REFUSED),
        "signal {:?}: {}",
        d.signal,
        d.log
    );
    assert!(d.log.contains("FATAL ALLEN refused"), "{}", d.log);
    let refusal = d
        .log
        .lines()
        .find(|l| l.contains("FATAL ALLEN refused"))
        .unwrap()
        .to_string();
    assert_eq!(
        pin,
        std::fs::read(pin_path(&r.compose())).unwrap(),
        "not rebound"
    );
    assert!(r.file(PATH).is_none(), "nothing executed");
    // The rightful agent still starts, and its ledger holds no effect.
    r.start_with(&[("AIEN_ALLEN_SUBJECT", &s(&subj_a))])
        .unwrap_or_else(|d| panic!("agent A restart: {}", d.log));
    assert!(r.intents().is_empty());
    r.detail = refusal.chars().take(200).collect();
}

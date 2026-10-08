//! Real-process recovery matrix for the ordinary compose -> grant -> execute -> ack
//! path (arch#162). Every case starts the real `aien daemon` and the real
//! `aien compose ...` CLI as OS processes, each with its OWN fresh
//! AIEN_RUNTIME_STATE_DIR, AIEN_COMPOSE_DIR (the desk key lives under it),
//! AIEN_PROVENANCE_DIR, socket and workspace. Crashes are SIGKILL (kill -9) of
//! the real process at a named fault-hold point of `aien compose execute`
//! (before_intent, after_intent, after_write), then SIGKILL of the daemon,
//! then a restart over the same state. Nothing is a clean shutdown or abort().
//!
//! How the grant is obtained: a real SmolLM2-1.7B proposal on the CPU reference
//! backend (`aien compose propose`, one committed one-file change per case), then
//! the ordinary `aien compose authorize`, so the daemon mints the grant from its
//! own compose-commit record. That model daemon is SIGKILLed; the case then runs
//! on a model-less daemon over the same state. Two modes: AIEN_MATRIX_MAC unset
//! (daemon ignores desk proofs) and AIEN_MATRIX_MAC=1 (daemon started with
//! AIEN_COMPOSE_AUTHORIZE_REQUIRES_DESK=1, desk key in the case compose dir, every
//! authorize carries a desk MAC under a fresh nonce). Receipts record
//! "Authorize MAC: on|off" from the daemon log. AIEN_PROPOSER_MODEL overrides the
//! SmolLM2 directory. Rows M1 and M2 are about the MAC and always run with it on.
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
use aien_runtime::approved_auth::{desk_key_path, AuthorizeBinding, DeskKey};
use aien_runtime::client::AienRuntimeClient;
use aien_runtime::control::{ChatTurn, ControlCommand, ControlResponse};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const AUTHORIZE_DESK_ENV: &str = "AIEN_COMPOSE_AUTHORIZE_REQUIRES_DESK";

/// A committed proposal the real model made: the S3 report file and what it names.
#[derive(Clone)]
struct Prop {
    report: PathBuf,
    path: String,
    content_sha256: String,
    cx_promotion: u64,
    proposal_sha256: String,
}

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
    mac: bool,
    mac_line: String,
    props: Vec<Prop>,
}

impl Rig {
    fn new(case: &'static str) -> Rig {
        Rig::with_mac(case, std::env::var("AIEN_MATRIX_MAC").as_deref() == Ok("1"))
    }

    /// `mac` forces the authorize-MAC mode (rows that are about the MAC itself).
    fn with_mac(case: &'static str, mac: bool) -> Rig {
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
        let mut r = Rig {
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
            mac,
            mac_line: String::new(),
            props: Vec::new(),
        };
        if mac {
            r.env.push((AUTHORIZE_DESK_ENV.into(), "1".into()));
        }
        r
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
            AUTHORIZE_DESK_ENV,
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
                self.mac_line = text
                    .lines()
                    .find(|l| l.starts_with("Authorize MAC:"))
                    .unwrap_or("Authorize MAC: no line")
                    .to_string();
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

    /// One model-daemon session (SmolLM2 on the CPU reference backend, never the
    /// GPU): one real `aien compose propose` per file name. The daemon is then
    /// SIGKILLed; the case continues on a model-less daemon over the same state.
    /// A proposal that does not commit is retried (the model is sampled), up to 3 times.
    fn proposals(&mut self, files: &[&str]) {
        let m = std::env::var("AIEN_PROPOSER_MODEL").unwrap_or_else(|_| {
            format!(
                "{}/models/SmolLM2-1.7B-Instruct-31b70e2e869a",
                std::env::var("HOME").unwrap()
            )
        });
        let m = PathBuf::from(m);
        let saved = self.env.clone();
        self.env
            .push(("AIEN_MODEL_PATH".into(), s(&m.join("model.safetensors"))));
        self.env
            .push(("AIEN_TOKENIZER_PATH".into(), s(&m.join("tokenizer.json"))));
        self.env
            .push(("AIEN_COMPOSE_EDIT_BUDGET_MS".into(), "590000".into()));
        self.env
            .push(("AIEN_COMPOSE_DOC_BUDGET_MS".into(), "590000".into()));
        self.start()
            .unwrap_or_else(|d| panic!("proposer daemon: {}", d.log));
        let mut out = Vec::new();
        for (i, file) in files.iter().enumerate() {
            let goal = format!(
                "Create the file {file} with a short plain-text note that says the project keeps every change inside its workspace."
            );
            let mut got = None;
            for attempt in 0..3 {
                let o = self.cli(&["propose", "--goal", &goal, "--workspace", &s(&self.ws())]);
                let rep = &o.json["report"];
                if o.code == 0 && rep["committed"] == json!(true) {
                    let f = self.root.join(format!("S3-{}-{i}.json", self.case));
                    std::fs::write(&f, o.json.to_string()).unwrap();
                    got = Some(Prop {
                        report: f,
                        path: rep["proposal_path"].as_str().unwrap().to_string(),
                        content_sha256: rep["proposal_content_sha256"]
                            .as_str()
                            .unwrap()
                            .to_string(),
                        cx_promotion: rep["cx_promotion"].as_u64().unwrap(),
                        proposal_sha256: rep["proposal_sha256"].as_str().unwrap().to_string(),
                    });
                    break;
                }
                eprintln!("propose attempt {attempt} did not commit: {}", o.all());
            }
            out.push(got.expect("the model proposed a committable change in 3 attempts"));
        }
        self.kill_daemon();
        self.env = saved;
        self.start()
            .unwrap_or_else(|d| panic!("model-less restart: {}", d.log));
        self.props = out;
    }

    /// Authorize proposal `i`; (its S3 report file, the daemon-minted grant id).
    fn prepare(&self, i: usize) -> (PathBuf, u64) {
        let p = &self.props[i];
        (p.report.clone(), self.authorize(p))
    }

    fn path(&self, i: usize) -> String {
        self.props[i].path.clone()
    }

    /// Are `bytes` exactly the content the model proposed for `i`?
    fn is_content(&self, i: usize, bytes: &[u8]) -> bool {
        sha(bytes) == self.props[i].content_sha256
    }

    /// `aien compose authorize` for `p`; with the MAC mode on it carries a desk
    /// MAC under a fresh nonce. Returns the daemon-minted grant id.
    fn authorize(&self, p: &Prop) -> u64 {
        let o = self.authorize_raw(p, None);
        assert_eq!(o.code, 0, "authorize: {}", o.all());
        o.json["authorization"]["id"].as_u64().expect("grant id")
    }

    fn authorize_raw(&self, p: &Prop, proof: Option<(&str, &str)>) -> Out {
        let (rep, ws) = (s(&p.report), s(&self.ws()));
        let mut a = vec![
            "authorize",
            "--report",
            &rep,
            "--workspace",
            &ws,
            "--approver",
            "drake",
        ];
        if let Some((nonce, mac)) = proof {
            a.extend(["--desk-nonce", nonce, "--desk-mac", mac]);
        } else if self.mac {
            a.extend(["--desk", "1"]);
        }
        self.cli(&a)
    }

    /// A desk MAC for `p` under the desk key now in the compose dir, for `nonce`.
    fn desk_mac(&self, p: &Prop, nonce: &str) -> String {
        DeskKey::load(&desk_key_path(&self.compose()))
            .unwrap()
            .sign_authorize(&AuthorizeBinding {
                cx_promotion: p.cx_promotion,
                proposal_sha256: p.proposal_sha256.clone(),
                path: p.path.clone(),
                content_sha256: p.content_sha256.clone(),
                workspace: s(&self.ws()),
                approver: "drake".into(),
                constraints: vec![],
                nonce: nonce.into(),
                desk_key_id: DeskKey::load(&desk_key_path(&self.compose()))
                    .unwrap()
                    .id()
                    .to_string(),
            })
    }

    /// Replace the desk key file with a new key (rotation while nothing runs).
    fn rotate_desk_key(&self) {
        let p = desk_key_path(&self.compose());
        std::fs::remove_file(&p).unwrap();
        DeskKey::create(&p).unwrap();
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
            "bin_sha256": bin_sha, "backend": self.backend, "authorize_mac": self.mac_line, "model_sha256": self.model,
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
    r.proposals(&["r1.md"]);
    let (rep, grant) = r.prepare(0);
    r.execute_killed_at(&rep, grant, "before_intent");
    r.crash_and_restart();
    assert!(r.file(&r.path(0)).is_none(), "no file written");
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
    assert!(r.is_content(0, &r.file(&r.path(0)).unwrap().0));
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
    r.proposals(&["r2.md"]);
    let (rep, grant) = r.prepare(0);
    r.execute_killed_at(&rep, grant, "after_intent");
    r.crash_and_restart();
    assert!(r.file(&r.path(0)).is_none(), "the write never happened");
    let st = r.states();
    assert_eq!(st.len(), 1, "{st:?}");
    assert_eq!(
        st[0], "NOT_DONE",
        "an intent whose write never happened settles NOT_DONE, got {st:?}"
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
    assert!(r.file(&r.path(0)).is_none());
    assert_eq!(r.intents().len(), 1, "no second intent");
    r.detail = format!("SIGKILL at after_intent; restart settled {}", st[0]);
}

/// R3: killed after the write, before the ack.
#[test]
#[ignore = "needs AIEN_BIN: compose-linked CPU build with fault-hold"]
fn r3_crash_after_write_before_ack() {
    let mut r = Rig::new("R3");
    r.proposals(&["r3.md"]);
    let (rep, grant) = r.prepare(0);
    r.execute_killed_at(&rep, grant, "after_write");
    let before = r.file(&r.path(0)).expect("the write happened before the crash");
    assert!(r.is_content(0, &before.0));
    r.crash_and_restart();
    // The world matches content_sha256 inside the workspace: DONE.
    assert_eq!(r.states(), vec!["DONE"]);
    let again = r.execute(&rep, grant);
    assert_ne!(again.code, 0, "{}", again.all());
    names_error(&again, &["AlreadySpent"]);
    let after = r.file(&r.path(0)).unwrap();
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
    r.proposals(&["r3b.md"]);
    let (rep, grant) = r.prepare(0);
    r.execute_killed_at(&rep, grant, "after_write");
    r.kill_daemon();
    std::fs::write(r.ws().join(r.path(0)), "someone else wrote this\n").unwrap();
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
        std::fs::read_to_string(r.ws().join(r.path(0))).unwrap(),
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
    r.proposals(&["r4.md"]);
    let (rep, grant) = r.prepare(0);
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
        r.is_content(0, &r.file(&r.path(0)).unwrap().0),
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
    r.proposals(&["r5.md"]);
    assert!(r.backend.contains("CPU-reference"), "{}", r.backend);
    assert!(r.backend.contains("explicit fallback"), "{}", r.backend);
    let log = (1..=r.nd)
        .map(|n| std::fs::read_to_string(r.root.join(format!("daemon-{n}.log"))).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !log.contains("Omega CTA budget:") && !log.contains("Omega marker spin:"),
        "no GPU engine lines in a CPU build: {log}"
    );
    let (rep, grant) = r.prepare(0);
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
    r.proposals(&["r6a.md", "r6b.md", "r6c.md"]);
    // (a) minted by daemon A, spent, kill -9, daemon B: AlreadySpent, no second write.
    let (rep, grant) = r.prepare(0);
    assert_eq!(r.execute(&rep, grant).json["state"], "DONE");
    let first = r.file(&r.path(0)).unwrap();
    r.crash_and_restart();
    let again = r.execute(&rep, grant);
    assert_ne!(again.code, 0, "{}", again.all());
    names_error(&again, &["AlreadySpent"]);
    let second = r.file(&r.path(0)).unwrap();
    assert_eq!((first.1, first.2, first.3), (second.1, second.2, second.3));
    assert_eq!(r.intents().len(), 1);

    // (b) a revoked grant stays unusable, also across a kill -9 restart.
    let (rep_b, grant_b) = r.prepare(1);
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
    assert!(r.file(&r.path(1)).is_none());

    // (c) a grant older than an operator stop stays stale after resume.
    let (rep_c, grant_c) = r.prepare(2);
    assert_eq!(r.cli(&["stop", "--approver", "drake"]).code, 0);
    assert_eq!(r.cli(&["resume", "--approver", "drake"]).code, 0);
    let o = r.execute(&rep_c, grant_c);
    assert_ne!(o.code, 0, "{}", o.all());
    names_error(&o, &["Stale"]);
    assert!(r.file(&r.path(2)).is_none());

    // (d) authorizing the already-spent proposal again. With the desk MAC on, an
    // identical (nonce, MAC) pair is a replay and mints nothing; either way no
    // second effect may ever run for the same file.
    let p0 = r.props[0].clone();
    let au_note;
    if r.mac {
        let nonce = "r6d-nonce";
        let mac = r.desk_mac(&p0, nonce);
        let first = r.authorize_raw(&p0, Some((nonce, &mac)));
        let replay = r.authorize_raw(&p0, Some((nonce, &mac)));
        assert_ne!(replay.code, 0, "{}", replay.all());
        names_error(&replay, &["Replayed"]);
        au_note = format!(
            "first authorize on a spent proposal: code {} {}; identical replay refused",
            first.code,
            first.all().chars().take(120).collect::<String>()
        );
        if first.code == 0 {
            let g2 = first.json["authorization"]["id"].as_u64().expect("grant id");
            let o = r.execute(&rep, g2);
            assert_ne!(o.json["state"], "DONE", "a second effect: {}", o.all());
        }
    } else {
        let au = r.authorize_raw(&p0, None);
        au_note = format!(
            "authorize on a spent proposal: code {} {}",
            au.code,
            au.all().chars().take(160).collect::<String>()
        );
        if au.code == 0 {
            let g2 = au.json["authorization"]["id"].as_u64().expect("grant id");
            let o = r.execute(&rep, g2);
            assert_ne!(o.json["state"], "DONE", "a second effect: {}", o.all());
        }
    }
    let o = r.execute(&rep, grant);
    assert_ne!(o.code, 0);
    assert_eq!(r.intents().len(), 1, "still exactly one effect");
    r.detail = format!("a-c PASS; {au_note}");
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
    r.proposals(&["r7.md"]);
    r.kill_daemon();
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
    let (rep, grant) = r.prepare(0);
    assert_eq!(r.execute(&rep, grant).json["state"], "DONE");
    let g1 = generate(&r);
    let rec1 = record_text(&r, g1);
    assert_eq!(rec1["model_sha256"], json!(model_a));
    let done = r.file(&r.path(0)).unwrap();

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
    let now = r.file(&r.path(0)).unwrap();
    assert_eq!((done.0.clone(), done.1, done.2), (now.0, now.1, now.2));
    assert_eq!(
        pin_bytes,
        std::fs::read(pin_path(&r.compose())).unwrap(),
        "ALLEN pin unchanged"
    );
    let log2 = std::fs::read_to_string(r.root.join(format!("daemon-{}.log", r.nd))).unwrap();
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
    r.proposals(&["r8.md"]);
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
    // A real grant minted by A, spent by nobody yet.
    let (rep, grant) = r.prepare(0);
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
            let log = r.root.join(format!("daemon-{}.log", r.nd));
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
    assert!(r.file(&r.path(0)).is_none(), "nothing executed");
    // Under B: a real grant is not spendable (no daemon serves B), and a fresh
    // desk-style request has nothing to talk to.
    let o = r.execute(&rep, grant);
    assert_ne!(o.code, 0, "{}", o.all());
    assert_ne!(o.json["state"], "DONE", "{}", o.all());
    assert!(r.file(&r.path(0)).is_none(), "B executed A's grant");
    // Positive control: the same grant is real, A executes it exactly once.
    r.start_with(&[("AIEN_ALLEN_SUBJECT", &s(&subj_a))])
        .unwrap_or_else(|d| panic!("agent A restart: {}", d.log));
    assert!(r.intents().is_empty(), "no effect was opened under B");
    let ok = r.execute(&rep, grant);
    assert_eq!(ok.json["state"], "DONE", "{}", ok.all());
    assert_eq!(r.intents().len(), 1);
    r.detail = refusal.chars().take(200).collect();
}

/// M1: an authorize MAC replayed after a SIGKILL restart is `Replayed`, with no
/// second grant. The desk MAC is on in both matrix modes (the row is about it).
#[test]
#[ignore = "needs AIEN_BIN: compose-linked CPU build with fault-hold"]
fn m1_mac_replay_across_sigkill_restart() {
    let mut r = Rig::with_mac("M1", true);
    r.proposals(&["m1.md"]);
    let p = r.props[0].clone();
    let nonce = "m1-nonce";
    let mac = r.desk_mac(&p, nonce);
    let ok = r.authorize_raw(&p, Some((nonce, &mac)));
    assert_eq!(ok.code, 0, "{}", ok.all());
    let grant = ok.json["authorization"]["id"].as_u64().expect("grant id");
    // Real-process crash mid-effect, then SIGKILL of the daemon and a restart.
    r.execute_killed_at(&p.report, grant, "after_write");
    r.crash_and_restart();
    assert_eq!(r.states(), vec!["DONE"]);
    let replay = r.authorize_raw(&p, Some((nonce, &mac)));
    assert_ne!(replay.code, 0, "{}", replay.all());
    names_error(&replay, &["Replayed"]);
    // No second grant: the only grant is spent, the next id does not exist, and
    // the ledger still holds the one effect.
    let again = r.execute(&p.report, grant);
    assert_ne!(again.code, 0, "{}", again.all());
    names_error(&again, &["AlreadySpent"]);
    let next = r.execute(&p.report, grant + 1);
    assert_ne!(next.code, 0, "{}", next.all());
    assert_ne!(next.json["state"], "DONE", "{}", next.all());
    assert_eq!(r.intents().len(), 1);
    // A second replay after a second kill -9 restart is still refused.
    r.crash_and_restart();
    let replay2 = r.authorize_raw(&p, Some((nonce, &mac)));
    names_error(&replay2, &["Replayed"]);
    r.detail = "SIGKILL at after_write; restart DONE; same nonce+MAC twice (across 2 restarts) -> Replayed; grant+1 does not exist".into();
}

/// M2: the desk key is rotated while an intent is in flight (SIGKILL at
/// fault_hold). The old key's MAC is refused afterwards, and the ambiguous
/// outcome (the file changed while nothing ran) stays UNRESOLVED, never DONE.
#[test]
#[ignore = "needs AIEN_BIN: compose-linked CPU build with fault-hold"]
fn m2_rotated_desk_key_during_inflight_intent() {
    let mut r = Rig::with_mac("M2", true);
    r.proposals(&["m2.md"]);
    let p = r.props[0].clone();
    let (n1, n2) = ("m2-nonce-1", "m2-nonce-2");
    let mac1 = r.desk_mac(&p, n1);
    let old_key_mac2 = r.desk_mac(&p, n2); // signed by the key that is about to go
    let ok = r.authorize_raw(&p, Some((n1, &mac1)));
    assert_eq!(ok.code, 0, "{}", ok.all());
    let grant = ok.json["authorization"]["id"].as_u64().expect("grant id");
    r.execute_killed_at(&p.report, grant, "after_write");
    r.kill_daemon();
    // Ambiguity: someone else changes the file; the desk key is rotated.
    std::fs::write(r.ws().join(r.path(0)), "someone else wrote this\n").unwrap();
    r.rotate_desk_key();
    r.start().unwrap_or_else(|d| panic!("restart: {}", d.log));
    assert_eq!(r.states(), vec!["UNRESOLVED"], "never DONE for changed bytes");
    // The old MACs are refused (a fresh nonce under the old key; the used one).
    let stale = r.authorize_raw(&p, Some((n2, &old_key_mac2)));
    assert_ne!(stale.code, 0, "{}", stale.all());
    names_error(&stale, &["DeskMacInvalid"]);
    let used = r.authorize_raw(&p, Some((n1, &mac1)));
    assert_ne!(used.code, 0, "{}", used.all());
    names_error(&used, &["DeskMacInvalid", "Replayed"]);
    let used_why = used.all().chars().take(100).collect::<String>();
    // Control: the rotated key's MAC is accepted by the MAC check itself (it is
    // then refused further on because the proposal is already spent/unresolved,
    // which is not a MAC error).
    let fresh = r.desk_mac(&p, "m2-nonce-3");
    let ctl = r.authorize_raw(&p, Some(("m2-nonce-3", &fresh)));
    let ctl_text = ctl.all();
    assert!(
        !ctl_text.contains("DeskMacInvalid") && !ctl_text.contains("DeskMacRequired"),
        "rotated key rejected by the MAC check: {ctl_text}"
    );
    if ctl.code == 0 {
        let g2 = ctl.json["authorization"]["id"].as_u64().expect("grant id");
        let o = r.execute(&p.report, g2);
        assert_ne!(o.json["state"], "DONE", "{}", o.all());
    }
    let rc = r.cli(&["reconcile"]);
    assert!(
        rc.json["reconcile"]["outcomes"]
            .as_array()
            .map(|a| a.iter().all(|o| o["state"] != "DONE"))
            .unwrap_or(true),
        "{}",
        rc.json
    );
    assert_eq!(r.states(), vec!["UNRESOLVED"], "never DONE");
    assert_eq!(
        std::fs::read_to_string(r.ws().join(r.path(0))).unwrap(),
        "someone else wrote this\n"
    );
    r.detail = format!(
        "SIGKILL at after_write; file changed; desk key rotated; old-key MAC DeskMacInvalid; used MAC: {used_why}; UNRESOLVED kept; control (new key) passes MAC check: code {}",
        ctl.code
    );
}

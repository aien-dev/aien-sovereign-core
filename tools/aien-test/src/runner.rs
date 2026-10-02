//! Serial, host-pool runner for one gate (ADR 0028, Slice A).
//!
//! Order: parse manifest (BAD_MANIFEST means no receipt), pin the commit,
//! record whether the tree is clean, decide refusals, run, measure what
//! changed, derive the verdict, write blobs and an immutable receipt.
//!
//! Slice A choices where the ADR is silent (recorded in the PR):
//! - A dirty tree is refused unless the operator passes `--allow-dirty` (the
//!   manifest grammar has no key for it and P4 forbids adding one here). A
//!   run allowed this way is recorded with `tree_clean_before = false`.
//! - "Tree changed after the run" means `git status --porcelain` differs from
//!   the pre-run output, so it also catches a run that dirties an already
//!   allowed-dirty tree further.
//! - `pins` is always empty and `dependencies` is always empty.
//! - `gb10_present` is a heuristic: `/dev/nvidiactl` exists (UNVERIFIED).

use crate::evidence::{build_receipt, canonical_digest, sha256_hex, utc_string, Store, SCHEMA};
use crate::manifest::{self, Manifest};
use crate::process::{self, RunSpec};
use crate::verdict::{self, derive, parse_observations, Facts, Verdict};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct Options {
    pub evidence_dir: PathBuf,
    pub allow_dirty: bool,
    pub args_override: Option<Vec<String>>,
    pub kill_grace: Duration,
}

impl Options {
    pub fn new(evidence_dir: PathBuf) -> Options {
        Options {
            evidence_dir,
            allow_dirty: false,
            args_override: None,
            kill_grace: Duration::from_secs(10),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// Manifest invalid: no receipt (P8).
    BadManifest(String),
    /// Could not bind or write evidence (CR-004 style fatal).
    Fatal(String),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::BadManifest(m) => write!(f, "BAD_MANIFEST {m}"),
            RunError::Fatal(m) => write!(f, "FATAL {m}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub verdict: Verdict,
    pub reason: String,
    pub receipt_path: Option<PathBuf>,
    pub receipt_digest: Option<String>,
}

fn git_out(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn repo_root(dir: &Path) -> Result<PathBuf, String> {
    let s = git_out(dir, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(s.trim()))
}

fn head(root: &Path) -> Result<String, String> {
    let s = git_out(root, &["rev-parse", "HEAD"])?.trim().to_string();
    if s.len() == 40
        && s.chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    {
        Ok(s)
    } else {
        Err(format!("HEAD is not a 40-hex commit: {s:?}"))
    }
}

fn porcelain(root: &Path) -> Result<String, String> {
    git_out(root, &["status", "--porcelain"])
}

/// Absolute, lexically normalized path whose deepest existing ancestor is
/// canonicalized, so a not-yet-created evidence dir can still be compared.
fn resolve(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut norm = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::ParentDir => {
                norm.pop();
            }
            Component::CurDir => {}
            other => norm.push(other.as_os_str()),
        }
    }
    let mut existing = norm.clone();
    let mut tail = Vec::new();
    while !existing.exists() {
        match existing.file_name().map(|s| s.to_os_string()) {
            Some(n) => {
                tail.push(n);
                existing.pop();
            }
            None => break,
        }
    }
    let mut base = fs::canonicalize(&existing).unwrap_or(existing);
    for n in tail.iter().rev() {
        base.push(n);
    }
    base
}

const SKIP_DIRS: [&str; 5] = [".git", "target", "build", "vendor", "node_modules"];

/// D1: every `*.gate` file under `root`, skipping the usual build dirs and any
/// directory holding `.aien-test-ignore`. Sorted.
pub fn discover(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if dir.join(".aien-test-ignore").exists() {
            return;
        }
        let rd = match fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => return,
        };
        for e in rd.flatten() {
            let path = e.path();
            let ft = match e.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            let name = e.file_name().to_string_lossy().into_owned();
            if ft.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    walk(&path, out);
                }
            } else if ft.is_file() && name.ends_with(".gate") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}

/// Resolve `GATE_FILE_OR_NAME`: an existing file path, else the unique
/// `.gate` file under the repository whose `gate:` equals the name (D2).
pub fn locate_gate(cwd: &Path, name: &str) -> Result<PathBuf, String> {
    let p = Path::new(name);
    let direct = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    if direct.is_file() {
        return fs::canonicalize(&direct).map_err(|e| format!("cannot resolve {name}: {e}"));
    }
    let root = repo_root(cwd)?;
    let mut found: Vec<PathBuf> = Vec::new();
    let mut unparsable = 0;
    for path in discover(&root) {
        match fs::read(&path) {
            Ok(bytes) => match manifest::parse(&bytes) {
                Ok(m) if m.gate == name => found.push(path),
                Ok(_) => {}
                Err(_) => unparsable += 1,
            },
            Err(_) => unparsable += 1,
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(format!(
            "no gate named {name} ({unparsable} unparsable gate files skipped)"
        )),
        _ => Err(format!(
            "duplicate gate id {name} in: {}",
            found
                .iter()
                .map(|f| f.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn collect_files(root: &Path, rel: &str, out: &mut BTreeSet<String>) {
    let path = root.join(rel.trim_end_matches('/'));
    let md = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(_) => {
            out.insert(rel.trim_end_matches('/').to_string());
            return;
        }
    };
    if md.is_dir() {
        if let Ok(rd) = fs::read_dir(&path) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name == ".git" {
                    continue;
                }
                let child = format!("{}/{}", rel.trim_end_matches('/'), name);
                collect_files(root, &child, out);
            }
        }
    } else if md.is_file() {
        out.insert(rel.trim_end_matches('/').to_string());
    }
}

/// sha256 over the canonical list of (path, file sha256) for the explicit
/// `inputs` plus the D3 convention-implied inputs of `tests/<dir>/<name>.gate`.
/// A listed path that does not exist is recorded with a null digest. Compiler
/// identity is not included in Slice A because Slice A does not build.
pub fn build_inputs_digest(root: &Path, manifest_rel: Option<&Path>, m: &Manifest) -> String {
    let mut paths: BTreeSet<String> = BTreeSet::new();
    for i in &m.inputs {
        collect_files(root, i, &mut paths);
    }
    if let Some(rel) = manifest_rel {
        let dir = rel.parent().unwrap_or(Path::new(""));
        let stem = rel
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Ok(sub) = dir.strip_prefix("tests") {
            let sub = sub.to_string_lossy().into_owned();
            let join = |base: &str| {
                if sub.is_empty() {
                    base.to_string()
                } else {
                    format!("{base}/{sub}")
                }
            };
            for (base, want) in [
                (join("src"), stem.clone()),
                (join("tests"), format!("{stem}_test")),
            ] {
                if let Ok(rd) = fs::read_dir(root.join(&base)) {
                    for e in rd.flatten() {
                        let p = e.path();
                        let fstem = p
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        if p.is_file() && fstem == want {
                            paths.insert(format!("{base}/{}", e.file_name().to_string_lossy()));
                        }
                    }
                }
            }
        }
    }
    let entries: Vec<Value> = paths
        .iter()
        .map(|p| {
            let digest = fs::read(root.join(p)).ok().map(|b| sha256_hex(&b));
            json!({ "path": p, "sha256": digest })
        })
        .collect();
    canonical_digest(&json!({ "inputs": entries })).unwrap_or_default()
}

fn which(name: &str) -> bool {
    match std::env::var_os("PATH") {
        Some(p) => std::env::split_paths(&p).any(|d| d.join(name).is_file()),
        None => false,
    }
}

fn machine() -> Value {
    let kernel = fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let cpu_model = cpuinfo
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|p| p.1.trim().to_string())
        .unwrap_or_default();
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get() as u64)
        .unwrap_or(0);
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mem_kib = meminfo
        .lines()
        .find(|l| l.starts_with("MemTotal:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let gb10 = Path::new("/dev/nvidiactl").exists();
    let qemu = which("qemu-system-aarch64") || which("qemu-system-x86_64");
    json!({
        "arch": std::env::consts::ARCH,
        "kernel_release": kernel,
        "cpu_model": cpu_model,
        "cpu_count": cpus,
        "mem_total_kib": mem_kib,
        "gb10_present": gb10,
        "qemu_available": qemu,
    })
}

fn captured_env() -> Value {
    let mut m = Map::new();
    for k in [
        "AIEN_PROOF_OFF",
        "CC",
        "CFLAGS",
        "CHIPRUN_SELFTEST",
        "LANG",
        "LC_ALL",
        "RUSTFLAGS",
        "TZ",
    ] {
        if let Ok(v) = std::env::var(k) {
            m.insert(k.to_string(), Value::String(v));
        }
    }
    Value::Object(m)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn fatal<E: fmt::Display>(e: E) -> RunError {
    RunError::Fatal(e.to_string())
}

pub fn run_gate(root: &Path, manifest_path: &Path, opts: &Options) -> Result<Outcome, RunError> {
    let bytes = fs::read(manifest_path)
        .map_err(|e| RunError::Fatal(format!("cannot read manifest: {e}")))?;
    let m = manifest::parse(&bytes).map_err(|e| RunError::BadManifest(e.0))?;
    let started = now_secs();
    let t0 = Instant::now();

    let commit = head(root).map_err(RunError::Fatal)?;
    let before = porcelain(root).map_err(RunError::Fatal)?;
    let evidence_inside = resolve(&opts.evidence_dir).starts_with(resolve(root));
    let refusal: Option<String> = if evidence_inside {
        Some("evidence_inside".to_string())
    } else if !before.is_empty() && !opts.allow_dirty {
        Some("dirty_tree".to_string())
    } else {
        None
    };

    let exec_path = root.join(&m.exec);
    let exec_exists = exec_path.is_file();
    let will_run = verdict::pre_run(&m, &refusal, exec_exists).is_none();

    let mut facts = Facts {
        refusal,
        exec_exists,
        ..Facts::default()
    };
    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();
    let mut after = before.clone();
    if will_run {
        let spec = RunSpec {
            program: exec_path.clone(),
            args: opts.args_override.clone().unwrap_or_else(|| m.args.clone()),
            cwd: root.to_path_buf(),
            timeout: Duration::from_millis(m.timeout_ms),
            kill_grace: opts.kill_grace,
        };
        match process::run(&spec) {
            Ok(o) => {
                facts.exit_status = o.exit_code.map(i64::from);
                facts.timed_out = o.timed_out;
                stdout = o.stdout;
                stderr = o.stderr;
            }
            Err(e) => {
                stderr = format!("aien-test: cannot run {}: {e}\n", m.exec).into_bytes();
            }
        }
        after = porcelain(root).map_err(RunError::Fatal)?;
        let head_after = head(root).unwrap_or_default();
        facts.tree_changed_after = after != before;
        facts.head_moved = head_after != commit;
        facts.observations = parse_observations(&stdout);
    }
    let derived = derive(&m, &facts);

    if evidence_inside {
        return Ok(Outcome {
            verdict: derived.verdict,
            reason: derived.reason,
            receipt_path: None,
            receipt_digest: None,
        });
    }

    let store = Store::new(&opts.evidence_dir);
    let stdout_sha = store.put_blob(&stdout).map_err(fatal)?;
    let stderr_sha = store.put_blob(&stderr).map_err(fatal)?;
    let binary_sha = if exec_exists {
        fs::read(&exec_path).ok().map(|b| sha256_hex(&b))
    } else {
        None
    };
    let manifest_rel = manifest_path
        .strip_prefix(root)
        .ok()
        .map(|p| p.to_path_buf());
    let inputs_digest = build_inputs_digest(root, manifest_rel.as_deref(), &m);
    let machine_v = machine();
    let machine_digest = canonical_digest(&machine_v).map_err(fatal)?;

    let observations: Vec<Value> = facts
        .observations
        .iter()
        .map(|o| json!({ "name": o.name, "value": o.value }))
        .collect();
    let rules: Vec<Value> = derived
        .rules
        .iter()
        .map(|(t, h)| json!({ "rule": t, "held": h }))
        .collect();
    let checks: Vec<Value> = m
        .checks
        .iter()
        .map(|c| {
            let passed = will_run && c == "clean_tree_after" && !facts.tree_changed_after;
            json!({ "name": c, "passed": passed })
        })
        .collect();

    let mut core = Map::new();
    core.insert("gate".into(), json!(m.gate));
    core.insert("owner".into(), json!(m.owner));
    core.insert("manifest_digest".into(), json!(m.digest));
    core.insert("commit".into(), json!(commit));
    core.insert("pins".into(), json!([]));
    core.insert("tree_clean_before".into(), json!(before.is_empty()));
    core.insert("tree_clean_after".into(), json!(after.is_empty()));
    core.insert("commit_unchanged_after".into(), json!(!facts.head_moved));
    if let Some(b) = &binary_sha {
        core.insert("binary_sha256".into(), json!(b));
    }
    core.insert("build_inputs_digest".into(), json!(inputs_digest));
    core.insert("env".into(), captured_env());
    core.insert("machine".into(), machine_v);
    core.insert("machine_digest".into(), json!(machine_digest));
    core.insert("exit_status".into(), json!(facts.exit_status));
    core.insert("stdout_sha256".into(), json!(stdout_sha));
    core.insert("stderr_sha256".into(), json!(stderr_sha));
    core.insert("observations".into(), Value::Array(observations));
    core.insert("rules".into(), Value::Array(rules));
    core.insert("derived_verdict".into(), json!(derived.verdict.as_str()));
    core.insert("reason".into(), json!(derived.reason));
    core.insert("mutants".into(), json!([]));
    core.insert("checks".into(), Value::Array(checks));
    core.insert("dependencies".into(), json!([]));

    let cache_key = canonical_digest(&json!({
        "manifest_digest": m.digest,
        "binary_sha256": binary_sha,
        "build_inputs_digest": inputs_digest,
        "machine_digest": machine_digest,
        "dependencies": [],
        "runner_schema": SCHEMA,
        "pins": [],
    }))
    .map_err(fatal)?;

    let volatile = json!({
        "started_utc": utc_string(started),
        "finished_utc": utc_string(now_secs()),
        "duration_ms": t0.elapsed().as_millis() as u64,
        "pool": "host",
        "pool_wait_ms": 0,
        "runner": { "name": "aien-test", "version": env!("CARGO_PKG_VERSION"), "impl": "rust" },
        "cache": { "key": cache_key, "reused": false, "reused_from": null, "mode": "disabled" },
        "timeout_exceeded": facts.timed_out,
    });

    let receipt = build_receipt(Value::Object(core), volatile);
    let (digest, path) = store.put_receipt(&receipt).map_err(fatal)?;
    Ok(Outcome {
        verdict: derived.verdict,
        reason: derived.reason,
        receipt_path: Some(path),
        receipt_digest: Some(digest),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{git, init_repo, write_file, TempDir};
    use std::os::unix::fs::PermissionsExt;

    const OK_SCRIPT: &str = "#!/bin/sh\ntouch \"$1\"\necho \"AIEN-OBS n 3\"\necho PASS\n";

    fn gate_text(
        exec: &str,
        args: &[&str],
        exit: i32,
        observe: &[&str],
        timeout: &str,
        extra: &str,
    ) -> String {
        format!(
            "gate: T-1\nmanifest_version: 1\nowner: t\nrequires:\n  - host\nrun:\n  exec: {exec}\n  args: {}\nexpects:\n  exit: {exit}\n  observe: {}\n  verdict: PASS\ntimeout: {timeout}\n{extra}",
            serde_json::to_string(args).unwrap(),
            serde_json::to_string(observe).unwrap(),
        )
    }

    struct Fx {
        repo: TempDir,
        ev: TempDir,
        scratch: TempDir,
    }

    impl Fx {
        fn marker(&self) -> PathBuf {
            self.scratch.path().join("marker")
        }
        fn manifest(&self) -> PathBuf {
            self.repo.path().join("tests/t.gate")
        }
        fn opts(&self) -> Options {
            let mut o = Options::new(self.ev.path().join("out"));
            o.kill_grace = Duration::from_millis(300);
            o
        }
    }

    /// A repo with `tools/run.sh` = script and a gate whose args are the
    /// marker path.
    fn fixture(script: &str, exit: i32, observe: &[&str], timeout: &str, extra: &str) -> Fx {
        let scratch = TempDir::new("scratch");
        let marker = scratch.path().join("marker");
        let gate = gate_text(
            "tools/run.sh",
            &[marker.to_str().unwrap()],
            exit,
            observe,
            timeout,
            extra,
        );
        let repo = init_repo(
            "repo",
            &[
                ("tools/run.sh", script, true),
                ("tests/t.gate", gate.as_str(), false),
                ("README", "hello\n", false),
            ],
        );
        Fx {
            repo,
            ev: TempDir::new("ev"),
            scratch,
        }
    }

    fn ok_fixture() -> Fx {
        fixture(OK_SCRIPT, 0, &["n == 3"], "20s", "")
    }

    fn read_json(p: &Path) -> Value {
        serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
    }

    fn run(fx: &Fx, opts: &Options) -> Outcome {
        run_gate(fx.repo.path(), &fx.manifest(), opts).unwrap()
    }

    #[test]
    fn pass_writes_a_bound_receipt() {
        let fx = ok_fixture();
        let o = run(&fx, &fx.opts());
        assert_eq!(o.verdict, Verdict::Pass, "reason {}", o.reason);
        assert!(fx.marker().exists(), "script must have run");
        let path = o.receipt_path.unwrap();
        let r = read_json(&path);
        assert_eq!(r["schema"], "aien-test/EvidenceReceiptV1");
        let core = &r["core"];
        let head_now = git_out(fx.repo.path(), &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(core["commit"], head_now.trim());
        assert_eq!(core["gate"], "T-1");
        assert_eq!(core["derived_verdict"], "PASS");
        assert_eq!(core["reason"], "");
        assert_eq!(core["exit_status"], 0);
        assert_eq!(core["tree_clean_before"], true);
        assert_eq!(core["tree_clean_after"], true);
        assert_eq!(core["commit_unchanged_after"], true);
        assert_eq!(core["observations"][0]["name"], "n");
        assert_eq!(core["observations"][0]["value"], 3);
        assert_eq!(core["rules"][0]["held"], true);
        let script_bytes = fs::read(fx.repo.path().join("tools/run.sh")).unwrap();
        assert_eq!(core["binary_sha256"], sha256_hex(&script_bytes));
        let gate_bytes = fs::read(fx.manifest()).unwrap();
        assert_eq!(core["manifest_digest"], sha256_hex(&gate_bytes));
        assert_eq!(r["volatile"]["pool"], "host");
        assert_eq!(r["volatile"]["timeout_exceeded"], false);
        // Receipt is read-only and named by its digest.
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o444
        );
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            format!("{}.json", o.receipt_digest.unwrap())
        );
        // stdout blob holds the script output verbatim.
        let sha = core["stdout_sha256"].as_str().unwrap();
        let blob = fx.ev.path().join("out/blobs").join(format!("{sha}.log"));
        assert_eq!(fs::read(blob).unwrap(), b"AIEN-OBS n 3\nPASS\n");
    }

    #[test]
    fn dirty_tree_is_refused_and_nothing_runs() {
        let fx = ok_fixture();
        write_file(fx.repo.path(), "README", "changed\n", false);
        let o = run(&fx, &fx.opts());
        assert_eq!(o.verdict, Verdict::NotRun);
        assert_eq!(o.reason, "dirty_tree");
        assert!(!fx.marker().exists(), "script must not run on a dirty tree");
        let r = read_json(&o.receipt_path.unwrap());
        assert_eq!(r["core"]["tree_clean_before"], false);
        assert_eq!(r["core"]["exit_status"], Value::Null);
    }

    #[test]
    fn untracked_file_counts_as_dirty() {
        let fx = ok_fixture();
        write_file(fx.repo.path(), "stray.txt", "x", false);
        let o = run(&fx, &fx.opts());
        assert_eq!(
            (o.verdict, o.reason.as_str()),
            (Verdict::NotRun, "dirty_tree")
        );
        assert!(!fx.marker().exists());
    }

    #[test]
    fn allow_dirty_runs_and_records_it() {
        let fx = ok_fixture();
        write_file(fx.repo.path(), "README", "changed\n", false);
        let mut opts = fx.opts();
        opts.allow_dirty = true;
        let o = run(&fx, &opts);
        assert_eq!(o.verdict, Verdict::Pass, "reason {}", o.reason);
        assert!(fx.marker().exists());
        let r = read_json(&o.receipt_path.unwrap());
        assert_eq!(r["core"]["tree_clean_before"], false);
    }

    #[test]
    fn timeout_is_enforced_with_fail() {
        let fx = fixture("#!/bin/sh\nsleep 20\n", 0, &[], "300ms", "");
        let t = Instant::now();
        let o = run(&fx, &fx.opts());
        assert!(t.elapsed() < Duration::from_secs(10));
        assert_eq!((o.verdict, o.reason.as_str()), (Verdict::Fail, "timeout"));
        let r = read_json(&o.receipt_path.unwrap());
        assert_eq!(r["volatile"]["timeout_exceeded"], true);
    }

    #[test]
    fn rule_mismatch_fails_even_if_the_script_prints_pass() {
        let fx = fixture(
            "#!/bin/sh\necho \"AIEN-OBS n 4\"\necho PASS\necho 'AIEN-OBS verdict \"PASS\"'\n",
            0,
            &["n == 3"],
            "20s",
            "",
        );
        let o = run(&fx, &fx.opts());
        assert_eq!((o.verdict, o.reason.as_str()), (Verdict::Fail, "rule:0"));
        let r = read_json(&o.receipt_path.unwrap());
        assert_eq!(r["core"]["rules"][0]["held"], false);
    }

    #[test]
    fn exit_status_must_match() {
        let fx = fixture(
            "#!/bin/sh\necho \"AIEN-OBS n 3\"\nexit 2\n",
            0,
            &["n == 3"],
            "20s",
            "",
        );
        let o = run(&fx, &fx.opts());
        assert_eq!(
            (o.verdict, o.reason.as_str()),
            (Verdict::Fail, "rc_nonzero")
        );
        let r = read_json(&o.receipt_path.unwrap());
        assert_eq!(r["core"]["exit_status"], 2);
    }

    #[test]
    fn nonzero_expected_exit_passes() {
        let fx = fixture("#!/bin/sh\nexit 7\n", 7, &[], "20s", "");
        assert_eq!(run(&fx, &fx.opts()).verdict, Verdict::Pass);
    }

    #[test]
    fn missing_exec_is_missing_implementation() {
        let fx = ok_fixture();
        fs::remove_file(fx.repo.path().join("tools/run.sh")).unwrap();
        git(fx.repo.path(), &["add", "-A"]);
        git(fx.repo.path(), &["commit", "-q", "-m", "rm"]);
        let o = run(&fx, &fx.opts());
        assert_eq!(o.verdict, Verdict::MissingImplementation);
        assert_eq!(o.reason, "NO_RUN_EXEC");
        let r = read_json(&o.receipt_path.unwrap());
        assert!(r["core"].get("binary_sha256").is_none());
    }

    #[test]
    fn evidence_dir_inside_repo_is_refused() {
        let fx = ok_fixture();
        let mut opts = fx.opts();
        opts.evidence_dir = fx.repo.path().join("evidence");
        let o = run(&fx, &opts);
        assert_eq!(
            (o.verdict, o.reason.as_str()),
            (Verdict::NotRun, "evidence_inside")
        );
        assert!(o.receipt_path.is_none());
        assert!(!fx.repo.path().join("evidence").exists());
        assert!(!fx.marker().exists());
    }

    #[test]
    fn tree_dirtied_by_the_run_fails() {
        let fx = fixture("#!/bin/sh\necho more >> README\n", 0, &[], "20s", "");
        let o = run(&fx, &fx.opts());
        assert_eq!(
            (o.verdict, o.reason.as_str()),
            (Verdict::Fail, "dirty_after")
        );
        let r = read_json(&o.receipt_path.unwrap());
        assert_eq!(r["core"]["tree_clean_after"], false);
    }

    #[test]
    fn head_moving_during_the_run_fails() {
        let fx = fixture(
            "#!/bin/sh\ngit -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false commit -q --allow-empty -m moved\n",
            0,
            &[],
            "20s",
            "",
        );
        let o = run(&fx, &fx.opts());
        assert_eq!(
            (o.verdict, o.reason.as_str()),
            (Verdict::Fail, "head_moved")
        );
        let r = read_json(&o.receipt_path.unwrap());
        assert_eq!(r["core"]["commit_unchanged_after"], false);
    }

    #[test]
    fn args_after_double_dash_replace_manifest_args() {
        let fx = ok_fixture();
        let other = fx.scratch.path().join("other");
        let mut opts = fx.opts();
        opts.args_override = Some(vec![other.to_str().unwrap().to_string()]);
        let o = run(&fx, &opts);
        assert_eq!(o.verdict, Verdict::Pass);
        assert!(other.exists());
        assert!(!fx.marker().exists());
    }

    #[test]
    fn bad_manifest_writes_no_receipt() {
        let fx = ok_fixture();
        write_file(fx.repo.path(), "tests/t.gate", "gate: X\nbogus: 1\n", false);
        match run_gate(fx.repo.path(), &fx.manifest(), &fx.opts()) {
            Err(RunError::BadManifest(_)) => {}
            other => panic!("expected BadManifest, got {other:?}"),
        }
        assert!(!fx.ev.path().join("out").exists());
    }

    #[test]
    fn operator_and_other_pool_gates_do_not_run() {
        let fx = ok_fixture();
        let gate = fs::read_to_string(fx.manifest()).unwrap();
        write_file(
            fx.repo.path(),
            "tests/t.gate",
            &gate.replace("  - host\n", "  - host\n  - operator\n"),
            false,
        );
        git(fx.repo.path(), &["add", "-A"]);
        git(fx.repo.path(), &["commit", "-q", "-m", "op"]);
        let o = run(&fx, &fx.opts());
        assert_eq!(o.verdict, Verdict::BlockedOperator);
        assert!(!fx.marker().exists());
        write_file(
            fx.repo.path(),
            "tests/t.gate",
            &gate.replace("  - host\n", "  - gb10\n"),
            false,
        );
        git(fx.repo.path(), &["add", "-A"]);
        git(fx.repo.path(), &["commit", "-q", "-m", "gb10"]);
        let o = run(&fx, &fx.opts());
        assert_eq!(
            (o.verdict, o.reason.as_str()),
            (Verdict::NotRun, "pool_unsupported:gb10")
        );
        assert!(!fx.marker().exists());
    }

    #[test]
    fn unfinished_receipts_are_never_modified_by_a_second_run() {
        let fx = ok_fixture();
        let first = run(&fx, &fx.opts()).receipt_path.unwrap();
        let before = fs::read(&first).unwrap();
        let second = run(&fx, &fx.opts());
        assert_eq!(second.verdict, Verdict::Pass);
        assert_eq!(fs::read(&first).unwrap(), before);
    }

    #[test]
    fn inputs_digest_tracks_input_bytes() {
        let fx = ok_fixture();
        let m = manifest::parse(&fs::read(fx.manifest()).unwrap()).unwrap();
        let mut m2 = m.clone();
        m2.inputs = vec!["README".to_string()];
        let a = build_inputs_digest(fx.repo.path(), None, &m2);
        assert_eq!(a, build_inputs_digest(fx.repo.path(), None, &m2));
        write_file(fx.repo.path(), "README", "different\n", false);
        let b = build_inputs_digest(fx.repo.path(), None, &m2);
        assert_ne!(a, b);
    }

    #[test]
    fn convention_inputs_are_found() {
        let repo = init_repo(
            "conv",
            &[
                ("src/rt/world.c", "int x;", false),
                ("tests/rt/world_test.c", "int t;", false),
                ("tests/rt/other.c", "int o;", false),
            ],
        );
        let m = manifest::parse(gate_text("tools/x.sh", &[], 0, &[], "5s", "").as_bytes()).unwrap();
        let rel = Path::new("tests/rt/world.gate");
        let a = build_inputs_digest(repo.path(), Some(rel), &m);
        write_file(repo.path(), "src/rt/world.c", "int y;", false);
        let b = build_inputs_digest(repo.path(), Some(rel), &m);
        assert_ne!(a, b, "implied src input must feed the digest");
        write_file(repo.path(), "tests/rt/other.c", "int p;", false);
        let c = build_inputs_digest(repo.path(), Some(rel), &m);
        assert_eq!(b, c, "unrelated file must not feed the digest");
    }

    #[test]
    fn locate_by_path_name_and_duplicates() {
        let g1 = gate_text("tools/x.sh", &[], 0, &[], "5s", "").replace("T-1", "ONE");
        let g2 = gate_text("tools/x.sh", &[], 0, &[], "5s", "").replace("T-1", "TWO");
        let repo = init_repo(
            "loc",
            &[
                ("tests/a/one.gate", g1.as_str(), false),
                ("tests/b/two.gate", g2.as_str(), false),
                ("target/skipped.gate", g1.as_str(), false),
                ("tests/bad.gate", "nonsense\n", false),
            ],
        );
        let p = locate_gate(repo.path(), "ONE").unwrap();
        assert!(p.ends_with("tests/a/one.gate"), "{p:?}");
        let p = locate_gate(repo.path(), "tests/b/two.gate").unwrap();
        assert!(p.ends_with("tests/b/two.gate"));
        assert!(locate_gate(repo.path(), "NOPE").is_err());
        write_file(repo.path(), "tests/c/dup.gate", g1.as_str(), false);
        let e = locate_gate(repo.path(), "ONE").unwrap_err();
        assert!(e.contains("duplicate"), "{e}");
    }

    #[test]
    fn ignore_marker_hides_a_directory() {
        let g = gate_text("tools/x.sh", &[], 0, &[], "5s", "");
        let repo = init_repo(
            "ign",
            &[
                ("tests/a.gate", g.as_str(), false),
                ("tests/skip/b.gate", g.as_str(), false),
                ("tests/skip/.aien-test-ignore", "", false),
            ],
        );
        let found = discover(repo.path());
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("tests/a.gate"));
    }

    #[test]
    fn resolve_handles_missing_dirs_and_dotdot() {
        let t = TempDir::new("res");
        let p = resolve(&t.path().join("a/b/../c"));
        assert_eq!(p, t.path().join("a/c"));
    }
}

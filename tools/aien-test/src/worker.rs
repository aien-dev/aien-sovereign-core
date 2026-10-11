//! The trusted worker (ADR 0033 Decision 9; local pipeline contracts C1 to
//! C7) and the state directory every local-CI command shares.
//!
//! State lives under `~/.local/state/aien-local-ci/`:
//!
//! ```text
//! evidence/<repo>/<sha>/   receipts and blobs of that commit's run
//! bundles/<repo>/<sha>/bundle.json   the signed VerificationBundleV1
//! trigger/<repo>.<sha>     a file whose presence asks for a run
//! outbox/<repo>.<sha>.json a GitHub status that could not be posted yet
//! journal.jsonl            one line per finished head
//! worker.heartbeat         what the worker is doing right now
//! ```
//!
//! For one head: fetch into the trusted clone `<clones>/<repo>` (its checkout
//! is never touched), add a detached worktree `<wt-root>/<repo>/<sha>`,
//! `crumb claim` it, plan, run the selected gates with the sandbox and the
//! pools, write receipts, build and sign the bundle with `~/.aien/verify.key`,
//! write it, journal it, `crumb close`, remove the worktree. With `--github`
//! the commit status `AIEN Verification (Spark)` is posted: success only on
//! PASS, failure on FAIL, error otherwise. When GitHub cannot be reached the
//! status goes to the outbox, the journal says `publication: pending`, and the
//! next successful contact flushes it. GitHub being down is never a local
//! failure.

use crate::bundle;
use crate::evidence::{canonical, canonical_digest, sha256_hex, utc_string};
use crate::graph;
use crate::plan::{self, Known, Section};
use crate::process;
use crate::resources::Hardware;
use crate::runner::{self, CacheNote, Options};
use crate::sandbox::Sandbox;
use crate::verdict::Verdict;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const CONTEXT: &str = "AIEN Verification (Spark)";
pub const EXECUTOR_CLASS: &str = "spark";

/// The local-CI state directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateDir {
    pub root: PathBuf,
}

impl StateDir {
    pub fn new(root: &Path) -> StateDir {
        StateDir {
            root: root.to_path_buf(),
        }
    }

    /// `~/.local/state/aien-local-ci`; None without HOME.
    pub fn default_dir() -> Option<StateDir> {
        let home = std::env::var_os("HOME")?;
        Some(StateDir::new(
            &PathBuf::from(home).join(".local/state/aien-local-ci"),
        ))
    }

    /// The directory key of a repository name: its last path segment.
    pub fn repo_key(repository: &str) -> String {
        repository
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(repository)
            .to_string()
    }

    pub fn evidence(&self, repo: &str, sha: &str) -> PathBuf {
        self.root.join("evidence").join(repo).join(sha)
    }

    pub fn bundle_path(&self, repo: &str, sha: &str) -> PathBuf {
        self.root
            .join("bundles")
            .join(repo)
            .join(sha)
            .join("bundle.json")
    }

    pub fn trigger_dir(&self) -> PathBuf {
        self.root.join("trigger")
    }

    pub fn outbox_dir(&self) -> PathBuf {
        self.root.join("outbox")
    }

    pub fn journal(&self) -> PathBuf {
        self.root.join("journal.jsonl")
    }

    pub fn heartbeat(&self) -> PathBuf {
        self.root.join("worker.heartbeat")
    }

    /// Append one JSON line to the journal.
    pub fn journal_append(&self, entry: &Value) -> Result<(), String> {
        fs::create_dir_all(&self.root).map_err(|e| format!("cannot create state dir: {e}"))?;
        let mut f = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(self.journal())
            .map_err(|e| format!("cannot open journal: {e}"))?;
        let mut line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
        line.push('\n');
        f.write_all(line.as_bytes())
            .map_err(|e| format!("cannot write journal: {e}"))
    }

    /// Overwrite the heartbeat file (atomic rename).
    pub fn beat(&self, repo: &str, sha: &str, phase: &str) {
        let v = json!({
            "utc": utc_string(now_secs()),
            "pid": std::process::id(),
            "repo": repo,
            "sha": sha,
            "phase": phase,
        });
        let _ = fs::create_dir_all(&self.root);
        let tmp = self.root.join("worker.heartbeat.tmp");
        if fs::write(&tmp, serde_json::to_string(&v).unwrap_or_default()).is_ok() {
            let _ = fs::rename(&tmp, self.heartbeat());
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// How the worker is set up.
#[derive(Debug, Clone)]
pub struct Config {
    pub state: StateDir,
    pub key: PathBuf,
    /// Where the trusted clones live: `<clones>/<repo>`.
    pub clones: PathBuf,
    pub wt_root: PathBuf,
    pub sandbox: Sandbox,
    pub github: bool,
    pub poll: Duration,
    pub repos: Vec<String>,
    /// The ref the plan is based on (`origin/main`); a head that *is* that
    /// ref is planned against its first parent.
    pub base_ref: String,
    pub operator: bool,
    /// The crumb command (`crumb`); None skips crumb claim/close.
    pub crumb: Option<String>,
    /// The GitHub CLI (`gh`), looked up on PATH.
    pub gh: String,
    pub hardware_identity: String,
}

impl Config {
    pub fn new(state: StateDir, key: &Path, clones: &Path, wt_root: &Path) -> Config {
        Config {
            state,
            key: key.to_path_buf(),
            clones: clones.to_path_buf(),
            wt_root: wt_root.to_path_buf(),
            sandbox: Sandbox::None,
            github: false,
            poll: Duration::from_secs(30),
            repos: Vec::new(),
            base_ref: "origin/main".to_string(),
            operator: false,
            crumb: Some("crumb".to_string()),
            gh: "gh".to_string(),
            hardware_identity: String::new(),
        }
    }
}

/// What one head produced.
#[derive(Debug, Clone)]
pub struct Summary {
    pub repo: String,
    pub sha: String,
    pub repository: String,
    pub verdict: Verdict,
    pub reason: String,
    pub bundle_verdict: String,
    pub bundle_path: PathBuf,
    pub bundle_digest: String,
    pub gates: Vec<(String, String, Verdict)>,
    pub publication: String,
    pub notes: Vec<String>,
}

fn run_cmd(cwd: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(cwd);
    if program == "git" {
        cmd.env("GIT_OPTIONAL_LOCKS", "0");
    }
    let out = process::output_gated(&mut cmd).map_err(|e| format!("cannot run {program}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    run_cmd(cwd, "git", args)
}

/// Fetch the clone and add a detached worktree for `sha`. The clone's own
/// checkout is never touched.
fn checkout(
    cfg: &Config,
    repo: &str,
    sha: &str,
    notes: &mut Vec<String>,
) -> Result<(PathBuf, PathBuf), String> {
    let clone = cfg.clones.join(repo);
    if !clone.join(".git").exists() && !clone.join("HEAD").exists() {
        return Err(format!("no trusted clone at {}", clone.display()));
    }
    let present = |c: &Path| git(c, &["cat-file", "-e", &format!("{sha}^{{commit}}")]).is_ok();
    if !present(&clone) {
        if let Err(e) = git(&clone, &["fetch", "--quiet", "--all"]) {
            notes.push(format!("fetch failed: {e}"));
        }
    } else if let Err(e) = git(&clone, &["fetch", "--quiet", "--all"]) {
        notes.push(format!("fetch failed (commit already present): {e}"));
    }
    if !present(&clone) {
        return Err(format!("commit {sha} is not in {}", clone.display()));
    }
    let wt = cfg.wt_root.join(repo).join(sha);
    if wt.exists() {
        let _ = git(
            &clone,
            &["worktree", "remove", "--force", &wt.display().to_string()],
        );
        let _ = fs::remove_dir_all(&wt);
    }
    fs::create_dir_all(wt.parent().unwrap_or(&cfg.wt_root))
        .map_err(|e| format!("cannot create {}: {e}", cfg.wt_root.display()))?;
    git(
        &clone,
        &[
            "worktree",
            "add",
            "--detach",
            "--quiet",
            &wt.display().to_string(),
            sha,
        ],
    )?;
    Ok((clone, wt))
}

fn remove_worktree(clone: &Path, wt: &Path) {
    let _ = git(
        clone,
        &["worktree", "remove", "--force", &wt.display().to_string()],
    );
    let _ = fs::remove_dir_all(wt);
    let _ = git(clone, &["worktree", "prune"]);
}

fn crumb(cfg: &Config, wt: &Path, args: &[&str], notes: &mut Vec<String>) {
    let Some(c) = &cfg.crumb else {
        return;
    };
    if !wt.join(".crumb").exists() {
        notes.push(format!(
            "crumb {}: no .crumb at the worktree root, skipped",
            args[0]
        ));
        return;
    }
    match run_cmd(wt, c, args) {
        Ok(_) => {}
        Err(e) => notes.push(format!("crumb {}: {e}", args[0])),
    }
}

/// Verify one head end to end. Never panics on GitHub trouble.
pub fn run_once(cfg: &Config, repo: &str, sha: &str) -> Result<Summary, String> {
    let mut notes: Vec<String> = Vec::new();
    cfg.state.beat(repo, sha, "checkout");
    let (clone, wt) = checkout(cfg, repo, sha, &mut notes)?;
    let result = run_in_worktree(cfg, repo, sha, &clone, &wt, &mut notes);
    crumb(
        cfg,
        &wt,
        &[
            "close",
            "aien-worker",
            ".",
            "worker",
            "done",
            "local verification finished",
        ],
        &mut notes,
    );
    remove_worktree(&clone, &wt);
    cfg.state.beat(repo, sha, "idle");
    let mut summary = result?;
    summary.notes.append(&mut notes);
    Ok(summary)
}

fn run_in_worktree(
    cfg: &Config,
    repo: &str,
    sha: &str,
    clone: &Path,
    wt: &Path,
    notes: &mut Vec<String>,
) -> Result<Summary, String> {
    let started = now_secs();
    crumb(
        cfg,
        wt,
        &[
            "claim",
            "aien-worker",
            ".",
            "worker",
            "local verification",
            "7200",
        ],
        notes,
    );
    let repository = plan::repository_name(clone, None);

    // Plan against the base ref; a head that is the base is planned against
    // its first parent so a push to main still verifies what it changed.
    cfg.state.beat(repo, sha, "plan");
    let base_sha = plan::resolve_first(wt, &[&cfg.base_ref, "main", &format!("{sha}~1")])?;
    let base = if base_sha == sha {
        plan::resolve(wt, &format!("{sha}~1")).unwrap_or(base_sha)
    } else {
        base_sha
    };
    let mut req = plan::Request::new(&base, sha);
    req.operator = cfg.operator;
    req.hardware_identity = cfg.hardware_identity.clone();
    let plan = plan::compute(wt, &req)?;

    // Run what the plan selects, through the sandbox and the pools.
    cfg.state.beat(repo, sha, "run");
    let evidence_dir = cfg.state.evidence(repo, sha);
    fs::create_dir_all(&evidence_dir).map_err(|e| format!("cannot create evidence dir: {e}"))?;
    let mut opts = Options::new(evidence_dir.clone());
    opts.sandbox = cfg.sandbox;
    opts.cargo_target_dir = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);
    let mut known: BTreeMap<String, Known> = BTreeMap::new();
    let mut notes_cache: BTreeMap<String, CacheNote> = BTreeMap::new();
    if plan.blocked.is_none() && !plan.entries.is_empty() {
        let g = graph::load(wt).map_err(|e| e.to_string())?;
        let mut sel = std::collections::BTreeSet::new();
        for e in plan
            .entries
            .iter()
            .filter(|e| e.section != Section::Blocked)
        {
            match g.index_of(&e.gate) {
                Some(i) => {
                    sel.insert(i);
                }
                None => notes.push(format!(
                    "gate {} is in the plan but not in the worktree graph",
                    e.gate
                )),
            }
        }
        let campaign = runner::run_graph(wt, &g, &sel, &opts).map_err(|e| e.to_string())?;
        for (id, o) in campaign.results {
            known.insert(
                id.clone(),
                Known {
                    verdict: o.verdict,
                    reason: o.reason.clone(),
                    receipt: o.receipt_digest.clone(),
                },
            );
            notes_cache.insert(id, o.cache);
        }
    }
    let judged = plan::judge(&plan, &known);

    // The bundle.
    cfg.state.beat(repo, sha, "bundle");
    let key = bundle::load_signing_key(&cfg.key)?;
    let vk = key.verifying_key();
    let machine = runner::machine(&Hardware::detect());
    let machine_digest = canonical_digest(&machine).map_err(|e| e.to_string())?;
    let tree = git(wt, &["rev-parse", &format!("{sha}^{{tree}}")])?;
    let mut checks: Vec<Value> = Vec::new();
    let mut entries: Vec<(bool, String, String)> = Vec::new();
    let mut gates: Vec<(String, String, Verdict)> = Vec::new();
    for e in &plan.entries {
        let (eff, _) = judged
            .gates
            .get(&e.gate)
            .cloned()
            .unwrap_or((Verdict::NotRun, String::new()));
        let k = known.get(&e.gate);
        let (status, verdict, extra): (&str, String, Value) =
            match (e.section, k, notes_cache.get(&e.gate)) {
                (Section::Blocked, _, _) => ("blocked", bundle_verdict(eff), json!({})),
                (_, Some(k), Some(CacheNote::Reused { from })) if k.receipt.is_some() => (
                    "cached",
                    bundle_verdict(eff),
                    json!({ "resolution": { "resolved_from": from, "receipt": k.receipt } }),
                ),
                (_, Some(k), _) if k.receipt.is_some() => {
                    ("run", bundle_verdict(eff), json!({ "receipt": k.receipt }))
                }
                _ => (
                    "not_run",
                    bundle_verdict(if eff == Verdict::Pass {
                        Verdict::NotRun
                    } else {
                        eff
                    }),
                    json!({}),
                ),
            };
        let mut entry = json!({
            "gate": e.gate,
            "check_id": e.check_id,
            "exactness": e.exactness.as_str(),
            "required": e.required,
            "status": status,
            "verdict": verdict,
            "backend": e.pool,
        });
        if let (Some(dst), Some(src)) = (entry.as_object_mut(), extra.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
        checks.push(entry);
        entries.push((e.required, status.to_string(), verdict.clone()));
        gates.push((e.gate.clone(), status.to_string(), eff));
    }
    let bundle_verdict = bundle::compute_verdict(&entries).to_string();
    let unsigned = json!({
        "schema": bundle::SCHEMA,
        "repository": repository,
        "commit": sha,
        "tree_digest": sha256_hex(tree.trim().as_bytes()),
        "graph_digest": plan.graph_digest,
        "receipt_id_scheme": "sha256-canonical-json",
        "checks": checks,
        "verdict": bundle_verdict,
        "executor": { "id": bundle::fingerprint(&vk), "class": EXECUTOR_CLASS, "machine_digest": machine_digest },
        "started_utc": utc_string(started),
        "finished_utc": utc_string(now_secs()),
        // The planner's verdict beside the bundle algebra: a bundle with no
        // required checks is PASS by the algebra (nothing required failed)
        // while the plan may say NOT_RUN UNMAPPED_PATH or BLOCKED. Readers
        // that gate on the plan read this; `verify` recomputes it anyway.
        "plan": {
            "verdict": judged.verdict.as_str(),
            "reason": judged.reason,
            "unmapped": plan.unmapped,
            "removed": plan.removed,
            "blocked": plan.blocked,
        },
    });
    let signed = bundle::sign(&unsigned, &key).map_err(|r| format!("cannot sign bundle: {r}"))?;
    let bytes = canonical(&signed).map_err(|e| e.to_string())?;
    let bundle_digest = sha256_hex(&bytes);
    let bundle_path = cfg.state.bundle_path(repo, sha);
    fs::create_dir_all(bundle_path.parent().unwrap_or(&cfg.state.root))
        .map_err(|e| format!("cannot create bundle dir: {e}"))?;
    fs::write(&bundle_path, &bytes).map_err(|e| format!("cannot write bundle: {e}"))?;

    // Publication, then the journal.
    cfg.state.beat(repo, sha, "publish");
    let publication = if cfg.github {
        let status = Status {
            repository: repository.clone(),
            sha: sha.to_string(),
            state: github_state(&bundle_verdict, judged.verdict).to_string(),
            description: format!(
                "{} {}: bundle {}",
                bundle_verdict,
                judged.reason,
                bundle_path.display()
            )
            .chars()
            .take(140)
            .collect(),
        };
        match post_status(&cfg.gh, &status) {
            Ok(()) => "posted".to_string(),
            Err(e) => {
                notes.push(format!("GitHub unreachable, status queued: {e}"));
                queue_status(&cfg.state, repo, &status)?;
                "pending".to_string()
            }
        }
    } else {
        "local".to_string()
    };
    let unmapped = plan.unmapped.clone();
    cfg.state.journal_append(&json!({
        "utc": utc_string(now_secs()),
        "repo": repo,
        "repository": repository,
        "sha": sha,
        "base": plan.base,
        "verdict": judged.verdict.as_str(),
        "reason": judged.reason,
        "bundle_verdict": bundle_verdict,
        "bundle": bundle_path.display().to_string(),
        "bundle_digest": bundle_digest,
        "graph_digest": plan.graph_digest,
        "gates": gates.iter().map(|(g, s, v)| json!({ "gate": g, "status": s, "verdict": v.as_str() })).collect::<Vec<_>>(),
        "unmapped": unmapped,
        "publication": publication,
        "notes": notes,
    }))?;
    Ok(Summary {
        repo: repo.to_string(),
        sha: sha.to_string(),
        repository,
        verdict: judged.verdict,
        reason: judged.reason,
        bundle_verdict,
        bundle_path,
        bundle_digest,
        gates,
        publication,
        notes: Vec::new(),
    })
}

fn bundle_verdict(v: Verdict) -> String {
    v.as_str().to_string()
}

/// success only on PASS, failure on FAIL, error on anything else.
pub fn github_state(bundle_verdict: &str, judged: Verdict) -> &'static str {
    match (bundle_verdict, judged) {
        ("PASS", Verdict::Pass) => "success",
        ("FAIL", _) | (_, Verdict::Fail) => "failure",
        _ => "error",
    }
}

/// A commit status to post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub repository: String,
    pub sha: String,
    pub state: String,
    pub description: String,
}

fn owner_repo(repository: &str) -> Option<String> {
    let parts: Vec<&str> = repository.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 3 {
        Some(format!(
            "{}/{}",
            parts[parts.len() - 2],
            parts[parts.len() - 1]
        ))
    } else if parts.len() == 2 {
        Some(format!("{}/{}", parts[0], parts[1]))
    } else {
        None
    }
}

/// `gh api -X POST repos/O/R/statuses/SHA ...`.
pub fn post_status(gh: &str, s: &Status) -> Result<(), String> {
    let or = owner_repo(&s.repository)
        .ok_or_else(|| format!("cannot derive owner/repo from {}", s.repository))?;
    let mut cmd = Command::new(gh);
    cmd.args([
        "api",
        "-X",
        "POST",
        &format!("repos/{or}/statuses/{}", s.sha),
        "-f",
        &format!("state={}", s.state),
        "-f",
        &format!("context={CONTEXT}"),
        "-f",
        &format!("description={}", s.description),
    ]);
    let out = process::output_gated(&mut cmd).map_err(|e| format!("cannot run {gh}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn queue_status(state: &StateDir, repo: &str, s: &Status) -> Result<(), String> {
    let dir = state.outbox_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create outbox: {e}"))?;
    let v = json!({ "repository": s.repository, "sha": s.sha, "state": s.state, "description": s.description });
    fs::write(
        dir.join(format!("{repo}.{}.json", s.sha)),
        serde_json::to_string(&v).unwrap_or_default(),
    )
    .map_err(|e| format!("cannot write outbox: {e}"))
}

/// Try to post every queued status; a success removes the file. Returns
/// (posted, still pending).
pub fn flush_outbox(cfg: &Config) -> (usize, usize) {
    let (mut posted, mut pending) = (0, 0);
    let Ok(rd) = fs::read_dir(cfg.state.outbox_dir()) else {
        return (0, 0);
    };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    files.sort();
    for f in files {
        let Ok(text) = fs::read_to_string(&f) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let s = Status {
            repository: v["repository"].as_str().unwrap_or("").to_string(),
            sha: v["sha"].as_str().unwrap_or("").to_string(),
            state: v["state"].as_str().unwrap_or("error").to_string(),
            description: v["description"].as_str().unwrap_or("").to_string(),
        };
        match post_status(&cfg.gh, &s) {
            Ok(()) => {
                let _ = fs::remove_file(&f);
                posted += 1;
                let _ = cfg.state.journal_append(&json!({
                    "utc": utc_string(now_secs()),
                    "sha": s.sha,
                    "repository": s.repository,
                    "publication": "posted",
                    "note": "flushed from outbox",
                }));
            }
            Err(_) => pending += 1,
        }
    }
    (posted, pending)
}

/// Pending trigger files as (repo, sha), oldest name first.
pub fn triggers(state: &StateDir) -> Vec<(String, String, PathBuf)> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(state.trigger_dir()) {
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files {
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some((repo, sha)) = name.rsplit_once('.') {
                if sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()) && !repo.is_empty()
                {
                    out.push((repo.to_string(), sha.to_ascii_lowercase(), f.clone()));
                }
            }
        }
    }
    out
}

/// Open PR heads and the main head of `repo` from GitHub (bounded: one call
/// per list). Errors are returned, never fatal to the loop.
pub fn github_heads(gh: &str, repository: &str) -> Result<Vec<String>, String> {
    let or = owner_repo(repository).ok_or("bad repository name")?;
    let mut shas = Vec::new();
    for args in [
        vec![
            "api",
            &format!("repos/{or}/pulls?state=open&per_page=50"),
            "--jq",
            ".[].head.sha",
        ],
        vec!["api", &format!("repos/{or}/commits/main"), "--jq", ".sha"],
    ] {
        let mut cmd = Command::new(gh);
        cmd.args(&args);
        let out = process::output_gated(&mut cmd).map_err(|e| format!("cannot run {gh}: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        for l in String::from_utf8_lossy(&out.stdout).lines() {
            let l = l.trim();
            if l.len() == 40 {
                shas.push(l.to_ascii_lowercase());
            }
        }
    }
    shas.sort();
    shas.dedup();
    Ok(shas)
}

/// The loop: flush the outbox, serve trigger files, poll GitHub when asked,
/// sleep. `iterations` bounds the rounds (None: forever). Returns the number
/// of heads served.
pub fn run_loop(cfg: &Config, iterations: Option<u64>) -> usize {
    let mut served = 0;
    let mut round = 0u64;
    loop {
        round += 1;
        cfg.state.beat("-", "-", "poll");
        let (posted, pending) = flush_outbox(cfg);
        if posted + pending > 0 {
            println!("OUTBOX posted={posted} pending={pending}");
        }
        let mut heads: Vec<(String, String, Option<PathBuf>)> = triggers(&cfg.state)
            .into_iter()
            .map(|(r, s, f)| (r, s, Some(f)))
            .collect();
        if cfg.github {
            for repo in &cfg.repos {
                let repository = plan::repository_name(&cfg.clones.join(repo), None);
                match github_heads(&cfg.gh, &repository) {
                    Ok(shas) => {
                        for s in shas {
                            if !cfg.state.bundle_path(repo, &s).exists() {
                                heads.push((repo.clone(), s, None));
                            }
                        }
                    }
                    Err(e) => println!("GITHUB unreachable for {repo}: {e} (will retry)"),
                }
            }
        }
        for (repo, sha, trigger) in heads {
            match run_once(cfg, &repo, &sha) {
                Ok(s) => {
                    println!(
                        "HEAD {repo} {sha} {} {} bundle {} publication {}",
                        s.verdict.as_str(),
                        s.reason,
                        s.bundle_path.display(),
                        s.publication
                    );
                    served += 1;
                }
                Err(e) => {
                    println!("HEAD {repo} {sha} ERROR {e}");
                    let _ = cfg.state.journal_append(&json!({
                        "utc": utc_string(now_secs()),
                        "repo": repo,
                        "sha": sha,
                        "verdict": "NOT_RUN",
                        "reason": format!("WORKER_ERROR:{e}"),
                        "publication": "none",
                    }));
                }
            }
            if let Some(f) = trigger {
                let _ = fs::remove_file(f);
            }
        }
        if iterations.is_some_and(|n| round >= n) {
            break;
        }
        std::thread::sleep(cfg.poll);
    }
    served
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_layout() {
        let s = StateDir::new(Path::new("/s"));
        assert_eq!(s.evidence("r", "abc"), PathBuf::from("/s/evidence/r/abc"));
        assert_eq!(
            s.bundle_path("r", "abc"),
            PathBuf::from("/s/bundles/r/abc/bundle.json")
        );
        assert_eq!(s.trigger_dir(), PathBuf::from("/s/trigger"));
        assert_eq!(s.journal(), PathBuf::from("/s/journal.jsonl"));
        assert_eq!(
            StateDir::repo_key("github.com/aien-dev/aien-sovereign-core"),
            "aien-sovereign-core"
        );
        assert_eq!(StateDir::repo_key("x"), "x");
    }

    #[test]
    fn github_states() {
        assert_eq!(github_state("PASS", Verdict::Pass), "success");
        assert_eq!(github_state("FAIL", Verdict::Fail), "failure");
        assert_eq!(github_state("NOT_RUN", Verdict::NotRun), "error");
        assert_eq!(github_state("NOT_RUN", Verdict::BlockedOperator), "error");
        // A PASS bundle whose plan-level verdict is not PASS is not success.
        assert_eq!(github_state("PASS", Verdict::NotRun), "error");
        assert_eq!(
            owner_repo("github.com/aien-dev/x").as_deref(),
            Some("aien-dev/x")
        );
        assert_eq!(owner_repo("aien-dev/x").as_deref(), Some("aien-dev/x"));
        assert_eq!(owner_repo("x"), None);
    }

    #[test]
    fn triggers_and_outbox_files() {
        let t = crate::testutil::TempDir::new("worker-state");
        let s = StateDir::new(t.path());
        fs::create_dir_all(s.trigger_dir()).unwrap();
        fs::write(s.trigger_dir().join(format!("repo.{}", "a".repeat(40))), "").unwrap();
        fs::write(s.trigger_dir().join("junk"), "").unwrap();
        fs::write(s.trigger_dir().join("repo.short"), "").unwrap();
        let t3 = triggers(&s);
        assert_eq!(t3.len(), 1);
        assert_eq!(
            (t3[0].0.as_str(), t3[0].1.as_str()),
            ("repo", "a".repeat(40).as_str())
        );
        let st = Status {
            repository: "github.com/o/r".into(),
            sha: "b".repeat(40),
            state: "success".into(),
            description: "d".into(),
        };
        queue_status(&s, "r", &st).unwrap();
        assert!(s
            .outbox_dir()
            .join(format!("r.{}.json", "b".repeat(40)))
            .exists());
        s.journal_append(&json!({"a": 1})).unwrap();
        s.journal_append(&json!({"b": 2})).unwrap();
        let j = fs::read_to_string(s.journal()).unwrap();
        assert_eq!(j.lines().count(), 2);
        s.beat("r", "sha", "plan");
        let hb: Value = serde_json::from_str(&fs::read_to_string(s.heartbeat()).unwrap()).unwrap();
        assert_eq!(hb["phase"], "plan");
    }
}

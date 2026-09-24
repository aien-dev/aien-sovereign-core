//! aien-proof CLI.
//!
//!   aien-proof run --job NAME [--input PATH]... [--gpu] [--resource NAME]... [--agent ID] -- CMD...
//!   aien-proof hold --resource NAME... [--job NAME] [--agent ID] -- CMD...
//!   aien-proof crates [--dir WORKSPACE] [--agent ID] [--gpu-crate NAME]... [--only NAME]... [-- CARGO_TEST_ARGS...]
//!   aien-proof verify
//!   aien-proof status

use aien_proof::board::{mem_available, Board, Job, Outcome};
use aien_proof::{ledger, workspace};
use std::path::PathBuf;
use std::process::{exit, Command};
use std::time::Instant;

const USAGE: &str = "usage:
  aien-proof run --job NAME [--input PATH]... [--gpu] [--resource NAME]... [--agent ID] -- CMD...
  aien-proof hold --resource NAME... [--job NAME] [--agent ID] -- CMD...
  aien-proof crates [--dir WORKSPACE] [--agent ID] [--gpu-crate NAME]... [--only NAME]... [-- CARGO_TEST_ARGS...]
  aien-proof verify
  aien-proof status";

struct Flags {
    values: Vec<(String, String)>,
    switches: Vec<String>,
    rest: Vec<String>,
}

impl Flags {
    fn parse(args: &[String], switches: &[&str]) -> Self {
        let mut f = Flags {
            values: vec![],
            switches: vec![],
            rest: vec![],
        };
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if a == "--" {
                f.rest = args[i + 1..].to_vec();
                break;
            } else if switches.contains(&a.as_str()) {
                f.switches.push(a.clone());
            } else if a.starts_with("--") && i + 1 < args.len() {
                f.values.push((a.clone(), args[i + 1].clone()));
                i += 1;
            } else {
                eprintln!("unexpected argument: {a}\n{USAGE}");
                exit(2);
            }
            i += 1;
        }
        f
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn all(&self, name: &str) -> Vec<String> {
        self.values
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .collect()
    }
}

fn agent(flags: &Flags) -> String {
    flags
        .get("--agent")
        .map(str::to_string)
        .or_else(|| std::env::var("AIEN_AGENT_ID").ok())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "agent".into())
}

/// Compiler identity plus flags that change what gets built.
fn toolchain() -> String {
    let mut t = Command::new("rustc")
        .arg("-vV")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    for var in ["RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_BUILD_TARGET"] {
        t.push_str(&format!(
            "{var}={}\n",
            std::env::var(var).unwrap_or_default()
        ));
    }
    t
}

fn cmd_run(args: &[String]) -> i32 {
    let flags = Flags::parse(args, &["--gpu"]);
    if flags.rest.is_empty() {
        eprintln!("{USAGE}");
        return 2;
    }
    let job = Job {
        name: flags
            .get("--job")
            .map(str::to_string)
            .unwrap_or_else(|| flags.rest.join(" ")),
        agent: agent(&flags),
        base: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        inputs: flags
            .all("--input")
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        cmd: flags.rest.clone(),
        gpu: flags.switches.iter().any(|s| s == "--gpu"),
        resources: flags.all("--resource"),
        toolchain: toolchain(),
    };
    match Board::from_env().run(&job) {
        Ok(outcome) => outcome.record().exit_code,
        Err(e) => {
            eprintln!("[aien-proof] {}: {e}", job.name);
            1
        }
    }
}

fn cmd_crates(args: &[String]) -> i32 {
    let flags = Flags::parse(args, &[]);
    let here = flags
        .get("--dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let (root, members) = match workspace::members(&here) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[aien-proof] cargo metadata failed: {e}");
            return 1;
        }
    };
    let only = flags.all("--only");
    let gpu_crates = flags.all("--gpu-crate");
    let board = Board::from_env();
    let (agent, toolchain) = (agent(&flags), toolchain());
    let started = Instant::now();
    let (mut stamped, mut joined, mut ran, mut failed) = (0, 0, 0, Vec::new());
    let selected: Vec<_> = members
        .iter()
        .filter(|m| only.is_empty() || only.contains(&m.name))
        .collect();

    for m in &selected {
        let mut cmd = vec![
            "cargo".to_string(),
            "test".into(),
            "-p".into(),
            m.name.clone(),
        ];
        cmd.extend(flags.rest.iter().cloned());
        let job = Job {
            name: format!("cargo-test:{}", m.name),
            agent: agent.clone(),
            base: root.clone(),
            inputs: m.inputs.clone(),
            cmd,
            gpu: m.gpu || gpu_crates.contains(&m.name),
            resources: vec![],
            toolchain: toolchain.clone(),
        };
        match board.run(&job) {
            Ok(outcome) => {
                let rec = outcome.record();
                eprintln!(
                    "[aien-proof] {:<32} {:<8} {:<4} {:>8} ms",
                    m.name,
                    outcome.label(),
                    if outcome.passed() { "pass" } else { "FAIL" },
                    rec.duration_ms
                );
                match outcome {
                    Outcome::Stamped(_) => stamped += 1,
                    Outcome::Joined(_) => joined += 1,
                    Outcome::Ran(_) => ran += 1,
                }
                if !outcome.passed() {
                    failed.push(m.name.clone());
                }
            }
            Err(e) => {
                eprintln!("[aien-proof] {}: {e}", m.name);
                failed.push(m.name.clone());
            }
        }
    }

    eprintln!(
        "[aien-proof] {} crates in {:.1}s: {ran} ran, {stamped} stamped, {joined} joined, {} failed",
        selected.len(),
        started.elapsed().as_secs_f64(),
        failed.len()
    );
    if failed.is_empty() {
        0
    } else {
        eprintln!("[aien-proof] failed: {}", failed.join(", "));
        1
    }
}

fn cmd_verify() -> i32 {
    let board = Board::from_env();
    match ledger::verify(&board.root.join(ledger::LEDGER_FILE)) {
        Ok((n, head)) => {
            println!("ledger OK: {n} events, head {}", ledger::hex(&head));
            0
        }
        Err(e) => {
            println!("ledger BROKEN: {e}");
            1
        }
    }
}

/// Run a command while holding exclusive keys (for example `machine-1`).
/// Always runs; recorded in the ledger as an `audit` event.
fn cmd_hold(args: &[String]) -> i32 {
    let flags = Flags::parse(args, &[]);
    let resources = flags.all("--resource");
    if flags.rest.is_empty() || resources.is_empty() {
        eprintln!("{USAGE}");
        return 2;
    }
    let job = Job {
        name: flags
            .get("--job")
            .map(str::to_string)
            .unwrap_or_else(|| flags.rest.join(" ")),
        agent: agent(&flags),
        base: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        inputs: vec![],
        cmd: flags.rest.clone(),
        gpu: false,
        resources,
        toolchain: String::new(),
    };
    match Board::from_env().hold(&job) {
        Ok(rec) => {
            eprintln!(
                "[aien-proof] {}: held {} for {} ms, exit {}, ledger #{}",
                rec.job,
                job.resources.join(","),
                rec.duration_ms,
                rec.exit_code,
                rec.ledger_index
            );
            rec.exit_code
        }
        Err(e) => {
            eprintln!("[aien-proof] {}: {e}", job.name);
            1
        }
    }
}

fn count(dir: PathBuf) -> usize {
    std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
}

fn cmd_status() -> i32 {
    let b = Board::from_env();
    println!("board:      {}", b.root.display());
    println!("stamps:     {}", count(b.root.join("stamps")));
    println!("failures:   {}", count(b.root.join("fails")));
    println!("cpu slots:  {}", b.cpu_slots);
    println!("memory gate:{} GiB free required", b.min_free_bytes >> 30);
    if let Some(free) = mem_available() {
        println!("free now:   {} GiB", free >> 30);
    }
    for (name, who) in b.holders() {
        println!("key held:   {name} by {who}");
    }
    cmd_verify()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("run") => cmd_run(&args[1..]),
        Some("crates") => cmd_crates(&args[1..]),
        Some("hold") => cmd_hold(&args[1..]),
        Some("verify") => cmd_verify(),
        Some("status") => cmd_status(),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    exit(code);
}

//! Argument parsing for the `aien-test` commands (ADR 0028 Decision 1 and 8).
//! Hand-written; the surface is small.
//!
//! ```text
//! aien-test run GATE_FILE_OR_NAME [flags] [-- ARGS...]
//! aien-test test ./... | gate:NAME [flags]
//! aien-test why GATE_FILE_OR_NAME [flags]
//! aien-test list [flags]
//! ```
//!
//! `test changed`, `test crate:NAME`, `--host/--qemu/--gb10`, `--mutants`,
//! `--release` and `--no-cache` come in later slices and are refused here.

use crate::resources::{parse_jobs, JobsSpec};
use std::path::PathBuf;

pub const USAGE: &str = "usage:
  aien-test run GATE_FILE_OR_NAME [flags] [-- ARGS...]
  aien-test test ./... | gate:NAME [flags]
  aien-test why GATE_FILE_OR_NAME [flags]
  aien-test list [flags]
flags: --evidence-dir DIR  --allow-dirty  --jobs host=N,qemu=M  --wait-gpu";

/// Which gates `test` runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// `./...`: every gate in the repository.
    All,
    /// `gate:NAME`: that gate and everything it depends on.
    Gate(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run { gate: String },
    Test(Selection),
    Why { gate: String },
    List,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub command: Command,
    pub evidence_dir: Option<PathBuf>,
    pub allow_dirty: bool,
    /// Arguments after `--`; they replace `run.args` (CR-061). `run` only.
    pub args: Option<Vec<String>>,
    /// `--jobs host=N,qemu=M`: the pool sizes that were named.
    pub jobs: JobsSpec,
    /// `--wait-gpu`: wait for a held GPU lock instead of refusing.
    pub wait_gpu: bool,
}

fn parse_selection(s: &str) -> Result<Selection, String> {
    if s == "./..." {
        return Ok(Selection::All);
    }
    match s.strip_prefix("gate:") {
        Some(name) if !name.is_empty() => Ok(Selection::Gate(name.to_string())),
        _ => Err(format!(
            "unsupported test selection {s:?}: only ./... and gate:NAME exist so far"
        )),
    }
}

pub fn parse_args(argv: &[String]) -> Result<Cli, String> {
    let name = argv
        .first()
        .map(|s| s.as_str())
        .ok_or_else(|| "no command".to_string())?;
    if !["run", "test", "why", "list"].contains(&name) {
        return Err(format!("unknown command {name:?}"));
    }
    let mut positional: Option<String> = None;
    let mut evidence_dir: Option<PathBuf> = None;
    let mut allow_dirty = false;
    let mut args: Option<Vec<String>> = None;
    let mut jobs = JobsSpec::default();
    let mut wait_gpu = false;
    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].as_str();
        if a == "--" {
            args = Some(argv[i + 1..].to_vec());
            break;
        } else if a == "--evidence-dir" {
            i += 1;
            match argv.get(i) {
                Some(v) => evidence_dir = Some(PathBuf::from(v)),
                None => return Err("--evidence-dir needs a value".to_string()),
            }
        } else if a == "--jobs" {
            i += 1;
            match argv.get(i) {
                Some(v) => jobs = parse_jobs(v)?,
                None => return Err("--jobs needs a value".to_string()),
            }
        } else if a == "--allow-dirty" {
            allow_dirty = true;
        } else if a == "--wait-gpu" {
            wait_gpu = true;
        } else if a.starts_with('-') {
            return Err(format!("unknown argument {a:?}"));
        } else if positional.is_none() {
            positional = Some(a.to_string());
        } else {
            return Err(format!("unexpected argument {a:?}"));
        }
        i += 1;
    }
    if args.is_some() && name != "run" {
        return Err("-- ARGS only apply to the run command".to_string());
    }
    let command = match name {
        "run" => Command::Run {
            gate: positional.ok_or("missing GATE_FILE_OR_NAME")?,
        },
        "why" => Command::Why {
            gate: positional.ok_or("missing GATE_FILE_OR_NAME")?,
        },
        "test" => {
            let selection = positional.ok_or("missing ./... or gate:NAME")?;
            Command::Test(parse_selection(&selection)?)
        }
        _ => {
            if let Some(p) = positional {
                return Err(format!("unexpected argument {p:?}"));
            }
            Command::List
        }
    };
    Ok(Cli {
        command,
        evidence_dir,
        allow_dirty,
        args,
        jobs,
        wait_gpu,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn run_gate(c: &Cli) -> &str {
        match &c.command {
            Command::Run { gate } => gate,
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[test]
    fn minimal() {
        let c = parse_args(&v(&["run", "G"])).unwrap();
        assert_eq!(run_gate(&c), "G");
        assert!(!c.allow_dirty);
        assert!(!c.wait_gpu);
        assert_eq!(c.args, None);
        assert_eq!(c.evidence_dir, None);
        assert_eq!(c.jobs, JobsSpec::default());
    }

    #[test]
    fn full() {
        let c = parse_args(&v(&[
            "run",
            "--allow-dirty",
            "G",
            "--evidence-dir",
            "/e",
            "--",
            "-x",
            "y",
        ]))
        .unwrap();
        assert_eq!(run_gate(&c), "G");
        assert!(c.allow_dirty);
        assert_eq!(c.evidence_dir, Some(PathBuf::from("/e")));
        assert_eq!(c.args, Some(v(&["-x", "y"])));
    }

    #[test]
    fn empty_override_args_are_kept() {
        let c = parse_args(&v(&["run", "G", "--"])).unwrap();
        assert_eq!(c.args, Some(Vec::new()));
    }

    #[test]
    fn test_selections() {
        let c = parse_args(&v(&["test", "./..."])).unwrap();
        assert_eq!(c.command, Command::Test(Selection::All));
        let c = parse_args(&v(&["test", "gate:G-1"])).unwrap();
        assert_eq!(c.command, Command::Test(Selection::Gate("G-1".to_string())));
    }

    #[test]
    fn unsupported_selections_are_refused() {
        for bad in ["changed", "crate:x", "gate:", "G", "./", "."] {
            assert!(parse_args(&v(&["test", bad])).is_err(), "{bad}");
        }
        assert!(parse_args(&v(&["test"])).is_err());
    }

    #[test]
    fn jobs_and_wait_gpu() {
        let c = parse_args(&v(&[
            "test",
            "./...",
            "--jobs",
            "host=3,qemu=2",
            "--wait-gpu",
        ]))
        .unwrap();
        assert_eq!(c.jobs.host, Some(3));
        assert_eq!(c.jobs.qemu, Some(2));
        assert!(c.wait_gpu);
        let c = parse_args(&v(&["test", "./...", "--jobs", "qemu=1"])).unwrap();
        assert_eq!(c.jobs.host, None);
        assert_eq!(c.jobs.qemu, Some(1));
    }

    #[test]
    fn bad_jobs_are_refused() {
        for bad in ["host=0", "gb10=2", "host", "host=x", "disk=1", ""] {
            assert!(
                parse_args(&v(&["test", "./...", "--jobs", bad])).is_err(),
                "{bad}"
            );
        }
        assert!(parse_args(&v(&["test", "./...", "--jobs"])).is_err());
    }

    #[test]
    fn why_and_list() {
        let c = parse_args(&v(&["why", "G-1"])).unwrap();
        assert_eq!(
            c.command,
            Command::Why {
                gate: "G-1".to_string()
            }
        );
        let c = parse_args(&v(&["list"])).unwrap();
        assert_eq!(c.command, Command::List);
        assert!(parse_args(&v(&["why"])).is_err());
        assert!(parse_args(&v(&["list", "extra"])).is_err());
    }

    #[test]
    fn override_args_only_for_run() {
        assert!(parse_args(&v(&["test", "./...", "--", "x"])).is_err());
        assert!(parse_args(&v(&["why", "G", "--", "x"])).is_err());
    }

    #[test]
    fn errors() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&v(&["test", "G"])).is_err());
        assert!(parse_args(&v(&["bogus", "G"])).is_err());
        assert!(parse_args(&v(&["run"])).is_err());
        assert!(parse_args(&v(&["run", "G", "H"])).is_err());
        assert!(parse_args(&v(&["run", "G", "--bogus"])).is_err());
        assert!(parse_args(&v(&["run", "G", "--evidence-dir"])).is_err());
    }
}

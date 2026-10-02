//! Argument parsing for `aien-test run GATE_FILE_OR_NAME [flags] [-- args]`.
//! Hand-written; the surface is small (ADR 0028 Decision 1 and 8).

use std::path::PathBuf;

pub const USAGE: &str =
    "usage: aien-test run GATE_FILE_OR_NAME [--evidence-dir DIR] [--allow-dirty] [-- ARGS...]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub gate: String,
    pub evidence_dir: Option<PathBuf>,
    pub allow_dirty: bool,
    /// Arguments after `--`; they replace `run.args` (CR-061).
    pub args: Option<Vec<String>>,
}

pub fn parse_args(argv: &[String]) -> Result<Cli, String> {
    match argv.first().map(|s| s.as_str()) {
        Some("run") => {}
        Some(other) => return Err(format!("unknown command {other:?}")),
        None => return Err("no command".to_string()),
    }
    let mut gate: Option<String> = None;
    let mut evidence_dir: Option<PathBuf> = None;
    let mut allow_dirty = false;
    let mut args: Option<Vec<String>> = None;
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
        } else if a == "--allow-dirty" {
            allow_dirty = true;
        } else if a.starts_with('-') {
            return Err(format!("unknown argument {a:?}"));
        } else if gate.is_none() {
            gate = Some(a.to_string());
        } else {
            return Err(format!("unexpected argument {a:?}"));
        }
        i += 1;
    }
    match gate {
        Some(gate) => Ok(Cli {
            gate,
            evidence_dir,
            allow_dirty,
            args,
        }),
        None => Err("missing GATE_FILE_OR_NAME".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn minimal() {
        let c = parse_args(&v(&["run", "G"])).unwrap();
        assert_eq!(c.gate, "G");
        assert!(!c.allow_dirty);
        assert_eq!(c.args, None);
        assert_eq!(c.evidence_dir, None);
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
    fn errors() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&v(&["test", "G"])).is_err());
        assert!(parse_args(&v(&["run"])).is_err());
        assert!(parse_args(&v(&["run", "G", "H"])).is_err());
        assert!(parse_args(&v(&["run", "G", "--bogus"])).is_err());
        assert!(parse_args(&v(&["run", "G", "--evidence-dir"])).is_err());
    }
}

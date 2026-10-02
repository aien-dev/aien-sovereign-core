//! `aien-test run GATE_FILE_OR_NAME`: run one gate, print the verdict, exit
//! non-zero unless PASS (0 PASS, 1 FAIL or BAD_MANIFEST, 2 NOT_RUN or usage,
//! 3 BLOCKED_HARDWARE, 4 BLOCKED_OPERATOR, 5 MISSING_IMPLEMENTATION).

use aien_test::cli::{parse_args, USAGE};
use aien_test::runner::{self, Options, RunError};
use aien_test::verdict::Verdict;
use std::path::PathBuf;

fn main() {
    std::process::exit(real_main());
}

fn real_main() -> i32 {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let cli = match parse_args(&argv) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("aien-test: {e}");
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("aien-test: cannot read current directory: {e}");
            return 2;
        }
    };
    let manifest_path = match runner::locate_gate(&cwd, &cli.gate) {
        Ok(p) => p,
        Err(e) => {
            println!("AIEN_TEST: BAD_MANIFEST {e}");
            return 1;
        }
    };
    let parent = manifest_path.parent().unwrap_or(&cwd).to_path_buf();
    let root = match runner::repo_root(&parent) {
        Ok(r) => r,
        Err(e) => {
            println!("AIEN_TEST: FAIL {e}");
            return 1;
        }
    };
    let evidence_dir = match cli.evidence_dir {
        Some(d) => d,
        None => match std::env::var_os("HOME") {
            Some(h) => {
                let name = root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "repo".to_string());
                PathBuf::from(h)
                    .join("workspace")
                    .join("evidence-out")
                    .join(name)
            }
            None => {
                eprintln!("aien-test: HOME is not set and no --evidence-dir given");
                return 2;
            }
        },
    };
    let mut opts = Options::new(evidence_dir);
    opts.allow_dirty = cli.allow_dirty;
    opts.args_override = cli.args;
    match runner::run_gate(&root, &manifest_path, &opts) {
        Ok(o) => {
            if o.verdict == Verdict::NotRun && !o.reason.is_empty() {
                println!("REFUSED: {}", o.reason);
            }
            if let Some(p) = &o.receipt_path {
                println!("RECEIPT {}", p.display());
            }
            if o.reason.is_empty() {
                println!("AIEN_TEST: {}", o.verdict.as_str());
            } else {
                println!("AIEN_TEST: {} {}", o.verdict.as_str(), o.reason);
            }
            o.verdict.exit_code()
        }
        Err(RunError::BadManifest(m)) => {
            println!("AIEN_TEST: BAD_MANIFEST {m}");
            1
        }
        Err(RunError::Fatal(m)) => {
            println!("AIEN_TEST: FAIL {m}");
            1
        }
    }
}

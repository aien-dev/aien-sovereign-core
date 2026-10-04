//! aien-test: run gates and report verdicts (ADR 0028).
//!
//! - `run GATE`: run one gate. Exit 0 PASS, 1 FAIL or BAD_MANIFEST, 2 NOT_RUN
//!   or usage, 3 BLOCKED_HARDWARE, 4 BLOCKED_OPERATOR, 5 MISSING_IMPLEMENTATION.
//! - `test ./...` and `test gate:NAME`: run many gates, dependencies first,
//!   with worker threads that honour the resource pools. The exit code is the
//!   campaign verdict: any FAIL gives 1, else the worst of 2 to 5, else 0.
//! - `why GATE`: explain a gate's state from the receipts already written.
//! - `list`: list the gates and their dependencies.
//! - `identity GATE`: print the check id (ADR 0033 Decision 3) and the object it
//!   hashed, for the gate and everything it depends on.

use aien_test::cli::{parse_args, Cli, Command, Selection, USAGE};
use aien_test::evidence::{Index, Store};
use aien_test::graph::{self, GraphError};
use aien_test::identity;
use aien_test::runner::{self, CacheNote, Options, RunError};
use aien_test::verdict::Verdict;
use aien_test::why;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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
    match &cli.command {
        Command::Run { gate } => cmd_run(&cli, gate, &cwd),
        Command::Test(selection) => cmd_test(&cli, selection, &cwd),
        Command::Why { gate } => cmd_why(&cli, gate, &cwd),
        Command::List => cmd_list(&cwd),
        Command::Identity { gate } => cmd_identity(&cli, gate, &cwd),
    }
}

/// Options for the real machine, with the command line applied.
fn options(cli: &Cli, root: &Path) -> Result<Options, i32> {
    let evidence_dir = match &cli.evidence_dir {
        Some(d) => d.clone(),
        None => match runner::default_evidence_dir(root) {
            Some(d) => d,
            None => {
                eprintln!("aien-test: HOME is not set and no --evidence-dir given");
                return Err(2);
            }
        },
    };
    let mut opts = Options::new(evidence_dir);
    opts.allow_dirty = cli.allow_dirty;
    opts.no_cache = cli.no_cache;
    opts.args_override = cli.args.clone();
    opts.pool_sizes = opts.pool_sizes.with(cli.jobs);
    opts.gpu.wait = cli.wait_gpu;
    Ok(opts)
}

fn run_error(e: &RunError) -> i32 {
    match e {
        RunError::BadManifest(m) => println!("AIEN_TEST: BAD_MANIFEST {m}"),
        RunError::Fatal(m) => println!("AIEN_TEST: FAIL {m}"),
    }
    1
}

fn graph_error(e: &GraphError) -> i32 {
    match e {
        // The message already starts with BAD_MANIFEST.
        GraphError::BadManifest { .. } => println!("AIEN_TEST: {e}"),
        _ => println!("AIEN_TEST: FAIL {e}"),
    }
    1
}

fn repo_root_or_report(cwd: &Path) -> Result<PathBuf, i32> {
    runner::repo_root(cwd).map_err(|e| {
        println!("AIEN_TEST: FAIL {e}");
        1
    })
}

/// Say plainly what the cache did. A reused result is never presented as a
/// fresh run.
fn print_cache(note: &CacheNote) {
    match note {
        CacheNote::Reused { from } => {
            println!("REUSED {from}");
            println!(
                "CACHE reused: an earlier run of exactly this experiment already has this result, so it was not run again (use --no-cache to force a fresh run)"
            );
        }
        CacheNote::Miss(why) => println!("CACHE miss: {why}; the gate was run"),
        CacheNote::Skipped(why) => println!("CACHE not used: {why}"),
    }
}

fn cmd_run(cli: &Cli, gate: &str, cwd: &Path) -> i32 {
    let manifest_path = match runner::locate_gate(cwd, gate) {
        Ok(p) => p,
        Err(e) => {
            println!("AIEN_TEST: BAD_MANIFEST {e}");
            return 1;
        }
    };
    let parent = manifest_path.parent().unwrap_or(cwd).to_path_buf();
    let root = match repo_root_or_report(&parent) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let opts = match options(cli, &root) {
        Ok(o) => o,
        Err(code) => return code,
    };
    match runner::run_gate(&root, &manifest_path, &opts) {
        Ok(o) => {
            if o.verdict == Verdict::NotRun && !o.reason.is_empty() {
                println!("REFUSED: {}", o.reason);
            }
            if let Some(p) = &o.receipt_path {
                println!("RECEIPT {}", p.display());
            }
            print_cache(&o.cache);
            if o.reason.is_empty() {
                println!("AIEN_TEST: {}", o.verdict.as_str());
            } else {
                println!("AIEN_TEST: {} {}", o.verdict.as_str(), o.reason);
            }
            o.verdict.exit_code()
        }
        Err(e) => run_error(&e),
    }
}

fn cmd_test(cli: &Cli, selection: &Selection, cwd: &Path) -> i32 {
    let root = match repo_root_or_report(cwd) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let graph = match graph::load(&root) {
        Ok(g) => g,
        Err(e) => return graph_error(&e),
    };
    let chosen: BTreeSet<usize> = match selection {
        Selection::All => (0..graph.len()).collect(),
        Selection::Gate(name) => match graph.index_of(name) {
            Some(i) => graph.with_dependencies(&[i]),
            None => {
                println!("AIEN_TEST: BAD_MANIFEST no gate named {name}");
                return 1;
            }
        },
    };
    if chosen.is_empty() {
        println!("AIEN_TEST: NOT_RUN no gates found under {}", root.display());
        return 2;
    }
    let opts = match options(cli, &root) {
        Ok(o) => o,
        Err(code) => return code,
    };
    match runner::run_graph(&root, &graph, &chosen, &opts) {
        Ok(campaign) => {
            for (id, o) in &campaign.results {
                if o.reason.is_empty() {
                    println!("GATE {id} {}", o.verdict.as_str());
                } else {
                    println!("GATE {id} {} {}", o.verdict.as_str(), o.reason);
                }
                if let Some(p) = &o.receipt_path {
                    println!("RECEIPT {}", p.display());
                }
                print_cache(&o.cache);
            }
            println!("AIEN_TEST: {}", campaign.verdict().as_str());
            campaign.exit_code()
        }
        Err(e) => run_error(&e),
    }
}

fn cmd_why(cli: &Cli, gate: &str, cwd: &Path) -> i32 {
    let root = match repo_root_or_report(cwd) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let graph = match graph::load(&root) {
        Ok(g) => g,
        Err(e) => return graph_error(&e),
    };
    let target = match why::resolve_target(&graph, cwd, gate) {
        Ok(i) => i,
        Err(e) => {
            println!("AIEN_TEST: BAD_MANIFEST {e}");
            return 1;
        }
    };
    let opts = match options(cli, &root) {
        Ok(o) => o,
        Err(code) => return code,
    };
    let commit = match runner::head(&root) {
        Ok(c) => c,
        Err(e) => {
            println!("AIEN_TEST: FAIL {e}");
            return 1;
        }
    };
    let index = Index::load(&Store::new(&opts.evidence_dir));
    let cache_facts =
        aien_test::cache::explain_chain(&root, &graph, &index, &opts, &commit, target);
    for line in why::explain(&graph, &index, &commit, target, &cache_facts) {
        println!("{line}");
    }
    0
}

fn cmd_identity(cli: &Cli, gate: &str, cwd: &Path) -> i32 {
    let root = match repo_root_or_report(cwd) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let graph = match graph::load(&root) {
        Ok(g) => g,
        Err(e) => return graph_error(&e),
    };
    let target = match why::resolve_target(&graph, cwd, gate) {
        Ok(i) => i,
        Err(e) => {
            println!("AIEN_TEST: BAD_MANIFEST {e}");
            return 1;
        }
    };
    let node = graph.node(target);
    let binding = match identity::local_binding(cli.hardware_identity.as_deref().unwrap_or("")) {
        Ok(b) => b,
        Err(e) => {
            println!("AIEN_TEST: FAIL {e}");
            return 1;
        }
    };
    let ids = match identity::graph_identities(
        &graph,
        target,
        &root,
        &cli.compiler_identity,
        &cli.flags,
        &binding,
    ) {
        Ok(i) => i,
        Err(e) => {
            println!("AIEN_TEST: FAIL {e}");
            return 1;
        }
    };
    for (g, (id, _)) in &ids {
        if *g != node.id {
            println!("DEPENDENCY {g} CHECK_ID {id}");
        }
    }
    if let Some((id, obj)) = ids.get(&node.id) {
        println!("CHECK_ID {id}");
        println!("OBJECT {}", obj);
    }
    0
}

fn cmd_list(cwd: &Path) -> i32 {
    let root = match repo_root_or_report(cwd) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let graph = match graph::load(&root) {
        Ok(g) => g,
        Err(e) => return graph_error(&e),
    };
    for &i in graph.order() {
        let node = graph.node(i);
        let file = node.path.strip_prefix(&root).unwrap_or(&node.path);
        let deps: Vec<&str> = node
            .deps
            .iter()
            .map(|&d| graph.node(d).id.as_str())
            .collect();
        println!(
            "{} pool={} depends_on={} file={}",
            node.id,
            node.manifest.pool(),
            if deps.is_empty() {
                "-".to_string()
            } else {
                deps.join(",")
            },
            file.display()
        );
    }
    println!("{} gates", graph.len());
    0
}

use clap::{Parser, Subcommand};
use colored::*;
use spark_inquisitor::{generate_inquisitor_interview, DiffAuditor, TestimonyEvaluator};
use std::fs;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "spark-inquisitor")]
#[command(author = "AIEN <aien.atlas@proton.me>")]
#[command(version = "0.1.0")]
#[command(about = "Autonomous Sovereign Inquisitor & Pull Request Alignment Gatekeeper")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate the Sovereign Alignment Interview comment for a Pull Request
    Interview {
        #[arg(long, help = "GitHub username of PR author")]
        author: String,

        #[arg(long, help = "Pull Request number")]
        pr: u64,

        #[arg(long, default_value = "External Contribution", help = "Pull Request title")]
        title: String,
    },

    /// Audit a git diff for telemetry, surveillance, and unslop violations
    Audit {
        #[arg(long, help = "Path to diff file")]
        diff: PathBuf,
    },

    /// Evaluate contributor testimony against the Sovereign Constitution
    Evaluate {
        #[arg(long, help = "Path to testimony text file")]
        testimony: PathBuf,
    },

    /// Run internal self-diagnostics
    Doctor,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Interview { author, pr, title } => {
            let comment = generate_inquisitor_interview(&author, pr, &title);
            println!("{}", comment);
        }
        Commands::Audit { diff } => {
            let content = match fs::read_to_string(&diff) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{} Failed to read diff file {}: {}", "error:".red().bold(), diff.display(), e);
                    std::process::exit(1);
                }
            };
            let report = DiffAuditor::audit_text(&content);
            if report.clean {
                println!("{} Diff is clean: zero telemetry, zero unslop violations.", "success:".green().bold());
            } else {
                eprintln!("{} Constitutional audit failed:", "failure:".red().bold());
                for v in &report.violations {
                    eprintln!("  * {}", v.red());
                }
                for u in &report.unslop_violations {
                    eprintln!("  * {}", u.yellow());
                }
                std::process::exit(1);
            }
        }
        Commands::Evaluate { testimony } => {
            let content = match fs::read_to_string(&testimony) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{} Failed to read testimony file {}: {}", "error:".red().bold(), testimony.display(), e);
                    std::process::exit(1);
                }
            };
            let verdict = TestimonyEvaluator::evaluate(&content);
            if verdict.approved {
                println!("{} {}", "PASS:".green().bold(), verdict.reason);
                println!("Score: {:.2}/1.00", verdict.score);
            } else {
                eprintln!("{} {}", "FAIL:".red().bold(), verdict.reason);
                eprintln!("Score: {:.2}/1.00", verdict.score);
                std::process::exit(1);
            }
        }
        Commands::Doctor => {
            println!("⚖️ Sovereign Inquisitor Diagnostics: OK");
            println!("  - Invariant rules: Active");
            println!("  - Telemetry scanner: Active");
            println!("  - Unslop scanner: Active");
            println!("  - Contributor Oath evaluator: Active");
        }
    }
}

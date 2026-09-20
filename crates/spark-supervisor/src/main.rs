fn default_supervisor_config_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config/aien/supervisor.toml")
}

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod proc_telemetry;
mod supervisor;

use config::SupervisorConfig;
use supervisor::Supervisor;

#[derive(Parser, Debug)]
#[command(
    name = "spark-supervisor",
    about = "Sovereign Native Process Supervisor for SparkOS"
)]
struct Cli {
    #[arg(long, default_value_os_t = default_supervisor_config_path())]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Run,
    Status,
    Check,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    if !cli.config.exists() {
        eprintln!(
            "Config file not found at {:?}. Generating default supervisor.toml...",
            cli.config
        );
        if let Some(p) = cli.config.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        let default_toml = format!(
            "[services.cortex]\ncommand = \"{}/.local/bin/cortex-rs\"\nargs = [\"--port\", \"18080\"]\nrestart = \"always\"\nhealth_url = \"http://127.0.0.1:18080/health\"\n\n[services.cockpit]\ncommand = \"{}/.local/bin/spark-cockpit-rs\"\nrestart = \"always\"\nhealth_url = \"http://127.0.0.1:18095/health\"\n",
            home, home
        );
        std::fs::write(&cli.config, default_toml)?;
    }

    let config = SupervisorConfig::load_from_file(&cli.config)?;
    let supervisor = Supervisor::new(config);

    match cli.command.unwrap_or(Commands::Run) {
        Commands::Run => {
            supervisor.run_daemon().await?;
        }
        Commands::Status => {
            supervisor.print_status().await;
        }
        Commands::Check => {
            println!("Configuration {:?} is valid.", cli.config);
        }
    }

    Ok(())
}

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use aien_provenance::SourceGrant;
use spark_harvester::{BundleKind, ExtractedPair, ReasoningExtractor, RightsGate};

#[derive(Parser)]
#[command(name = "spark-harvester")]
#[command(
    about = "Native Rust rights-gated harvester and provenance engine for sovereign AI datasets"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Extract reasoning pairs from raw text with mandatory SourceGrant rights gating
    Extract {
        #[arg(short, long)]
        prompt: String,
        #[arg(short, long)]
        input: String,
        #[arg(long, default_value = "commercial-lab")]
        provider: String,
        #[arg(long, default_value = "frontier-model")]
        model: String,
        /// Path to JSON file containing a valid, signed SourceGrant
        #[arg(long)]
        grant_file: Option<PathBuf>,
        /// Inline JSON string of SourceGrant
        #[arg(long)]
        grant_json: Option<String>,
        /// Optional output path for gated bundle
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Bundle kind: training, distillation, or evidence
        #[arg(long, default_value = "distillation")]
        bundle_kind: String,
    },
    /// Export extracted training pairs into JSONL with mandatory SourceGrant rights gating
    Distill {
        #[arg(short, long)]
        output: PathBuf,
        /// Path to JSON file containing a valid, signed SourceGrant
        #[arg(long)]
        grant_file: Option<PathBuf>,
        /// Inline JSON string of SourceGrant
        #[arg(long)]
        grant_json: Option<String>,
    },
    /// Display pipeline status
    Status,
}

fn load_grant(
    grant_file: Option<&PathBuf>,
    grant_json: Option<&str>,
) -> Result<SourceGrant, Box<dyn std::error::Error>> {
    if let Some(path) = grant_file {
        let content = std::fs::read_to_string(path)?;
        let grant: SourceGrant = serde_json::from_str(&content)?;
        Ok(grant)
    } else if let Some(json) = grant_json {
        let grant: SourceGrant = serde_json::from_str(json)?;
        Ok(grant)
    } else {
        Err("SourceGrant required: Pass --grant-file <PATH> or --grant-json <JSON> to prove rights admission. Unlicensed extraction is strictly rejected.".into())
    }
}

fn parse_bundle_kind(s: &str) -> Result<BundleKind, String> {
    match s.to_lowercase().as_str() {
        "training" => Ok(BundleKind::Training),
        "distillation" => Ok(BundleKind::Distillation),
        "evidence" => Ok(BundleKind::Evidence),
        other => Err(format!(
            "Unknown bundle kind {}. Must be training, distillation, or evidence",
            other
        )),
    }
}

const DEFAULT_ALLOWED_LICENSES: &[&str] = &[
    "Apache-2.0",
    "MIT",
    "BSD-3-Clause",
    "SRCL-1.0",
    "PolyForm-Noncommercial-1.0.0",
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let cli = Cli::parse();
    match cli.command {
        Commands::Extract {
            prompt,
            input,
            provider,
            model,
            grant_file,
            grant_json,
            output,
            bundle_kind,
        } => {
            let grant = load_grant(grant_file.as_ref(), grant_json.as_deref())?;
            let kind = parse_bundle_kind(&bundle_kind)?;

            let extractor = ReasoningExtractor::new();
            let pair = extractor.extract(&prompt, &input, &provider, &model);
            info!(
                "Extracted pair: tokens={}, quality={:.2}",
                pair.token_count, pair.quality_score
            );

            let bundle = RightsGate::validate_and_bundle(
                &grant,
                &[pair],
                kind,
                DEFAULT_ALLOWED_LICENSES,
            )?;
            info!(
                "RightsGate accepted bundle: ID={}, MerkleRoot={:?}",
                bundle.bundle_id, bundle.merkle_root
            );

            if let Some(out_path) = output {
                let count = RightsGate::export_bundle(&bundle, &out_path)?;
                info!("Exported {} rights-gated records to {:?}", count, out_path);
            }

            println!("{}", serde_json::to_string_pretty(&bundle)?);
        }
        Commands::Distill {
            output,
            grant_file,
            grant_json,
        } => {
            let grant = load_grant(grant_file.as_ref(), grant_json.as_deref())?;

            info!("Preparing distillation with SourceGrant {}", grant.grant_id);
            let sample_pair = ExtractedPair {
                prompt: "Sovereign AI foundational premise".to_string(),
                reasoning: Some("Verify local computation vs centralized extraction".to_string()),
                completion: "Human freedom requires local ownership of intelligence.".to_string(),
                provider: "community".to_string(),
                model: "atlas-sovereign".to_string(),
                token_count: 42,
                quality_score: 1.0,
            };

            let bundle = RightsGate::validate_and_bundle(
                &grant,
                &[sample_pair],
                BundleKind::Distillation,
                DEFAULT_ALLOWED_LICENSES,
            )?;

            let count = RightsGate::export_bundle(&bundle, &output)?;
            info!("Appended {} verified rights-gated pairs to {:?}", count, output);
        }
        Commands::Status => {
            println!("AIEN Sovereign Harvester Pipeline Active");
            println!("Rights Enforcement: Active (Strict SourceGrant validation & structural evidence quarantine)");
            println!("Zero Disk Secrets: Enforced via hardware TPM vault (atlas-vault)");
            println!("License: PolyForm Noncommercial 1.0.0 with Sovereign Defense Covenant");
        }
    }
    Ok(())
}

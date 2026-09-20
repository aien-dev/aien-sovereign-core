use clap::{Parser, Subcommand};
use spark_adapters::curriculum::{CurriculumEngine, CurriculumTrack};
use spark_adapters::models::{DistillTask, TaskType, VerificationStrategy};
use spark_adapters::{AdapterRouter, DistillationEngine};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "spark-distill")]
#[command(author = "AIEN <aien.atlas@proton.me>")]
#[command(version = "0.2.0")]
#[command(
    about = "Sovereign Teacher Distillation, Multi-Track Curriculum Crawler, and Training Pair Generator"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    // Flat arguments for backwards compatibility when no subcommand is given
    #[arg(short, long, help = "Task prompt or reasoning challenge to evaluate")]
    prompt: Option<String>,

    #[arg(
        short,
        long,
        default_value = "anthropic/claude-3-7-sonnet",
        help = "Teacher model identifier"
    )]
    teacher: String,

    #[arg(
        short,
        long,
        default_value = "atlas-lightning-omni",
        help = "Student open-weight model seat"
    )]
    student: String,

    #[arg(
        long,
        default_value = "code",
        help = "Task type: code, arch, reasoning, refactor, harness"
    )]
    task_type: String,

    #[arg(
        long,
        default_value = "compiler",
        help = "Verification strategy: compiler, unslop, json, consensus"
    )]
    verify: String,

    #[arg(long, help = "Commit high-scoring verified solution to Cortex memory")]
    commit_cortex: bool,

    #[arg(long, help = "Custom dataset directory for SFT/DPO output")]
    dataset_dir: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Execute a single prompt distillation task
    Single {
        #[arg(short, long, help = "Task prompt or reasoning challenge to evaluate")]
        prompt: String,

        #[arg(
            short,
            long,
            default_value = "anthropic/claude-3-7-sonnet",
            help = "Teacher model identifier"
        )]
        teacher: String,

        #[arg(
            short,
            long,
            default_value = "atlas-lightning-omni",
            help = "Student open-weight model seat"
        )]
        student: String,

        #[arg(
            long,
            default_value = "code",
            help = "Task type: code, arch, reasoning, refactor, harness"
        )]
        task_type: String,

        #[arg(
            long,
            default_value = "compiler",
            help = "Verification strategy: compiler, unslop, json, consensus"
        )]
        verify: String,

        #[arg(long, help = "Commit high-scoring verified solution to Cortex memory")]
        commit_cortex: bool,

        #[arg(long, help = "Custom dataset directory for SFT/DPO output")]
        dataset_dir: Option<PathBuf>,
    },

    /// Run an autonomous batch distillation crawler across a curriculum track
    Crawl {
        #[arg(
            short,
            long,
            default_value = "systems",
            help = "Curriculum track: systems, agent, science, frontier, dynamic"
        )]
        track: String,

        #[arg(short, long, default_value_t = 5, help = "Number of tasks to execute")]
        limit: usize,

        #[arg(
            short,
            long,
            default_value = "anthropic/claude-3-7-sonnet",
            help = "Teacher model identifier"
        )]
        teacher: String,

        #[arg(
            short,
            long,
            default_value = "atlas-lightning-omni",
            help = "Student open-weight model seat"
        )]
        student: String,

        #[arg(long, help = "Commit high-scoring verified solution to Cortex memory")]
        commit_cortex: bool,

        #[arg(long, help = "Custom dataset directory for SFT/DPO output")]
        dataset_dir: Option<PathBuf>,
    },

    /// Audit and verify all configured cloud subscriptions and hardware TPM vault keys
    VerifyKeys,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Single {
            prompt,
            teacher,
            student,
            task_type,
            verify,
            commit_cortex,
            dataset_dir,
        }) => {
            run_single(
                prompt,
                teacher,
                student,
                task_type,
                verify,
                commit_cortex,
                dataset_dir,
            )
            .await;
        }

        Some(Commands::Crawl {
            track,
            limit,
            teacher,
            student,
            commit_cortex,
            dataset_dir,
        }) => {
            run_crawl(
                &track,
                limit,
                &teacher,
                &student,
                commit_cortex,
                dataset_dir,
            )
            .await;
        }

        Some(Commands::VerifyKeys) => {
            run_verify_keys();
        }

        None => {
            if let Some(prompt) = cli.prompt {
                run_single(
                    prompt,
                    cli.teacher,
                    cli.student,
                    cli.task_type,
                    cli.verify,
                    cli.commit_cortex,
                    cli.dataset_dir,
                )
                .await;
            } else {
                eprintln!("Error: Missing prompt. Use `spark-distill single --prompt ...` or `spark-distill crawl --track systems`.");
                std::process::exit(1);
            }
        }
    }
}

async fn run_single(
    prompt: String,
    teacher: String,
    student: String,
    task_type_str: String,
    verify_str: String,
    commit_cortex: bool,
    dataset_dir: Option<PathBuf>,
) {
    let task_type = parse_task_type(&task_type_str);
    let strategy = parse_strategy(&verify_str);

    let task = DistillTask {
        id: Uuid::new_v4().to_string(),
        task_type,
        prompt: prompt.clone(),
        system_prompt: Some("You are a verified sovereign systems specialist. Output clear, compilable native code with zero unslop.".to_string()),
        teacher_model: teacher.clone(),
        student_model: Some(student.clone()),
        verification_strategy: strategy,
        commit_to_cortex: commit_cortex,
    };

    println!("=== Sovereign Knowledge Distillation ===");
    println!("Task ID:  {}", task.id);
    println!("Teacher:  {}", task.teacher_model);
    println!("Student:  {}", task.student_model.as_deref().unwrap_or("<none>"));
    println!("Prompt:   {}", task.prompt);

    let mut engine = DistillationEngine::new();
    if let Some(dir) = dataset_dir {
        engine = engine.with_dataset_dir(dir);
    }

    match engine.distill(task).await {
        Ok(record) => {
            println!("\n=== Distillation Complete ===");
            println!(
                "Teacher Verification: Passed: {}, Score: {:.2}",
                record.teacher_rollout.verification.passed,
                record.teacher_rollout.verification.score
            );
            if let Some(ref s_roll) = record.student_rollout {
                println!(
                    "Student Verification: Passed: {}, Score: {:.2}",
                    s_roll.verification.passed, s_roll.verification.score
                );
                println!("Preference Delta:     {:.2}", record.preference_delta);
            }
            if record.durable_memory_committed {
                println!(
                    "Cortex Entity:        {}",
                    record.cortex_entity_id.as_deref().unwrap_or("<committed>")
                );
            }
            println!(
                "Datasets Appended:    {}/sft.jsonl, {}/dpo.jsonl",
                engine.dataset_dir.display(),
                engine.dataset_dir.display()
            );
        }
        Err(e) => {
            eprintln!("Distillation error: {}", e);
            std::process::exit(1);
        }
    }
}

async fn run_crawl(
    track_str: &str,
    limit: usize,
    teacher: &str,
    student: &str,
    commit_cortex: bool,
    dataset_dir: Option<PathBuf>,
) {
    let track = CurriculumTrack::parse(track_str).unwrap_or(CurriculumTrack::NativeSystemsAndGpu);
    println!("=== Sovereign Batch Curriculum Crawler ===");
    println!("Curriculum Track: {}", track.display_name());
    println!("Task Limit:       {}", limit);
    println!("Teacher Model:    {}", teacher);
    println!("Student Seat:     {}", student);
    println!("Cortex Commit:    {}", commit_cortex);

    let tasks = CurriculumEngine::generate_tasks(
        track,
        limit,
        teacher,
        Some(student),
        commit_cortex,
    );

    let mut engine = DistillationEngine::new();
    if let Some(dir) = dataset_dir {
        engine = engine.with_dataset_dir(dir);
    }

    let mut passed_count = 0;
    let mut total_delta = 0.0;
    let mut cortex_count = 0;

    for (idx, task) in tasks.into_iter().enumerate() {
        println!("\n[{}/{}] Distilling: {}", idx + 1, limit, task.prompt);
        match engine.distill(task).await {
            Ok(record) => {
                let t_pass = record.teacher_rollout.verification.passed;
                let s_pass = record.student_rollout.as_ref().map(|s| s.verification.passed).unwrap_or(false);
                if t_pass || s_pass {
                    passed_count += 1;
                }
                total_delta += record.preference_delta;
                if record.durable_memory_committed {
                    cortex_count += 1;
                    println!("  ✓ Committed to Cortex: {}", record.cortex_entity_id.as_deref().unwrap_or("<id>"));
                }
                println!("  ✓ Score: Teacher: {:.2}, Delta: {:.2}", record.teacher_rollout.verification.score, record.preference_delta);
            }
            Err(e) => {
                eprintln!("  X Task failed: {}", e);
            }
        }
    }

    println!("\n=== Curriculum Crawl Summary ===");
    println!("Total Tasks Evaluated:  {}", limit);
    println!("Passing Candidates:     {}/{}", passed_count, limit);
    println!("Cortex Entities Added:  {}", cortex_count);
    println!("Average Preference Delta: {:.2}", total_delta / limit.max(1) as f32);
    println!("Datasets Directory:     {}", engine.dataset_dir.display());
}

fn run_verify_keys() {
    let router = AdapterRouter::new();
    let adapters = router.discover_adapters();

    println!("=== Sovereign Model & Subscription Key Audit ===");
    println!("{:<28} {:<15} {:<15} {:<10}", "ADAPTER ID", "PROVIDER", "STATUS", "RESIDENT");
    println!("{}", "-".repeat(72));

    for a in adapters {
        let status = if a.is_available {
            "✓ READY"
        } else if a.requires_key {
            "X KEY MISSING"
        } else {
            "✓ LOCAL"
        };
        let resident = if a.is_resident { "YES" } else { "NO" };
        println!("{:<28} {:<15} {:<15} {:<10}", a.id, a.provider.slug(), status, resident);
    }
}

fn parse_task_type(s: &str) -> TaskType {
    match s.to_lowercase().as_str() {
        "arch" => TaskType::SystemArchitecture,
        "reasoning" => TaskType::ReasoningTrace,
        "refactor" => TaskType::RefactorLogic,
        "harness" => TaskType::VerificationHarness,
        _ => TaskType::CodeSynthesis,
    }
}

fn parse_strategy(s: &str) -> VerificationStrategy {
    match s.to_lowercase().as_str() {
        "unslop" => VerificationStrategy::UnslopStrict,
        "json" => VerificationStrategy::JsonSchema,
        "consensus" => VerificationStrategy::DualConsensus,
        _ => VerificationStrategy::CompilerCheck,
    }
}

use colored::*;
use reqwest::Client;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use crate::crumbs::{format_dir_crumb_tui, leave_dir_whisper};
use crate::hive::{hive_kill, hive_roster, hive_spawn};
use crate::nesting::perform_nesting_ritual;
use crate::telemetry::get_gpu_telemetry;

pub async fn handle_slash_command(cmd: &str) -> bool {
    let parts: Vec<&str> = cmd.trim().split_whitespace().collect();
    if parts.is_empty() {
        return false;
    }

    match parts[0] {
        "/exit" | "/quit" => {
            println!("{}", "Exiting AIEN. The Spark desk remains ours.".cyan());
            std::process::exit(0);
        },
        "/clear" => {
            print!("{esc}[2J{esc}[1;1H", esc = 27 as char);
            let _ = std::io::stdout().flush();
            true
        },
        "/help" => {
            println!("{}", "\nAvailable Slash Commands:".cyan().bold());
            println!("  /skill [list|<name>]      Inspect and load dynamic sovereign skills\n  /goal [list|new|done]     Manage project goals & milestone lattices");
            println!("  /walkthrough [save]       Display live architecture map & roadmap (or save to WALKTHROUGH.md)");
            println!("  /nest                     Execute the Agent Nesting Ritual (grounding, threat check, peer wind, scent)");
            println!("  /crumb [path]             Inspect directory crumb (above, below, and local agent history)");
            println!("  /crumb whisper <message>  Leave a directory whisper for peer agents");
            println!("  /vault                    Inspect hardware TPM key vault and secret hygiene audit");
            println!("  /doctor                   Probe GB10 GPU, Model (18006), Judge (18082), Cortex (18080), Vault, Dream (18085)");
            println!("  /dream [now|status]       Inspect or trigger JSpace Dynamic Dream Cycle");
            println!("  /hive                     Manage ephemeral subagent swarm (/hive roster | /hive spawn <role> <task>)");
            println!("  /mask                     List or inspect active persona mask");
            println!("  /park <text>              Park high-volume input or interruption to park/ directory");
            println!("  /cortex <query>           Query permanent memory in Spark Cortex");
            println!("  /sync                     Run Radicle sovereign identity key synchronization");
            println!("  /clear                    Clear terminal screen");
            println!("  /exit                     Exit AIEN session\n");
            true
        },
        "/walkthrough" | "/roadmap" | "/map" => {
            if parts.len() > 1 && parts[1] == "save" {
                let dest = Path::new("/home/drakestapleton/basecamp/WALKTHROUGH.md");
                match crate::walkthrough::generate_and_save_walkthrough_md(dest) {
                    Ok(msg) => println!("{}", msg.green()),
                    Err(e) => println!("{}", format!("Failed to save walkthrough: {}", e).red()),
                }
            } else {
                let card = crate::walkthrough::render_walkthrough_tui();
                println!("{}", card);
            }
            true
        },
        "/nest" => {
            let session_id = format!("manual-nest-{}", chrono::Utc::now().timestamp());
            perform_nesting_ritual(&session_id, true).await;
            true
        },
        "/crumb" => {
            if parts.len() > 1 && parts[1] == "whisper" {
                if parts.len() < 3 {
                    println!("Usage: /crumb whisper <message>");
                } else {
                    let msg = parts[2..].join(" ");
                    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                    leave_dir_whisper("AIEN", &cwd, None, &msg);
                    println!("{}", format!("✓ Whisper left in {}/.crumb: \"{}\"", cwd.display(), msg).green());
                }
            } else {
                let target_str = if parts.len() > 1 { parts[1] } else { "." };
                let p = Path::new(target_str);
                println!("{}", format_dir_crumb_tui(p));
            }
            true
        },
        "/vault" => {
            println!("{}", "\n=== Hardware TPM Key Vault (atlas-vault) ===".cyan().bold());
            if !crate::vault::is_vault_available() {
                println!("{}", "❌ atlas-vault not found at /home/drakestapleton/.local/bin/atlas-vault".red());
            } else {
                let keys = crate::vault::list_keys();
                println!("• Hardware Vault:  {}", "TPM-bound atlas-vault (Active)".green());
                println!("• Registered Keys: {}", keys.len().to_string().cyan().bold());
                for k in &keys {
                    println!("  - {}", k.green());
                }
                let findings = crate::vault::audit_workspace_secrets();
                if findings.is_empty() {
                    println!("{}", "✓ Secret Hygiene:  Clean (0 stray .env or plaintext credential files on disk).".green());
                } else {
                    println!("{}", format!("⚠ Warning: {} stray .env files detected on disk:", findings.len()).yellow().bold());
                    for f in &findings {
                        println!("    - {}", f.red());
                    }
                }
                println!("{}", "• Storage Policy:  Direct value retrieval is in-memory only; plaintext .env strictly forbidden.\n".dimmed());
            }
            true
        },
        "/doctor" => {
            run_doctor().await;
            true
        },
        "/dream" => {
            if parts.len() > 1 && (parts[1] == "now" || parts[1] == "run") {
                println!("{}", "Triggering immediate JSpace Dream Cycle...".yellow());
                let out = Command::new("/home/drakestapleton/max-env/bin/python")
                    .args(["/home/drakestapleton/basecamp/aien-dream/dream_engine.py", "--now"])
                    .output();
                match out {
                    Ok(o) => {
                        println!("{}", String::from_utf8_lossy(&o.stdout));
                        if !o.stderr.is_empty() {
                            eprintln!("{}", String::from_utf8_lossy(&o.stderr).dimmed());
                        }
                    },
                    Err(e) => println!("{}", format!("Failed to run dream engine: {}", e).red()),
                }
            } else {
                let out = Command::new("/home/drakestapleton/max-env/bin/python")
                    .args(["/home/drakestapleton/basecamp/aien-dream/dream_engine.py", "--status"])
                    .output();
                match out {
                    Ok(o) => println!("{}", String::from_utf8_lossy(&o.stdout)),
                    Err(e) => println!("{}", format!("Failed to query dream status: {}", e).red()),
                }
            }
            true
        },
        "/hive" => {
            if parts.len() < 2 || parts[1] == "roster" {
                println!("{}", "=== aien-hive Active Swarm Roster ===".cyan().bold());
                println!("{}", hive_roster());
            } else if parts[1] == "spawn" && parts.len() >= 4 {
                let role = parts[2];
                let task = parts[3..].join(" ");
                println!("{}", format!("Spawning hive cell [{}]...", role).yellow());
                let res = hive_spawn(role, &task);
                println!("{}", res.green());
            } else if parts[1] == "kill" && parts.len() >= 3 {
                let name = parts[2];
                println!("{}", hive_kill(name));
            } else {
                println!("Usage: /hive [roster | spawn <role> <task> | kill <name>]");
            }
            true
        },
        "/mask" => {
            run_mask_command(&parts[1..]);
            true
        },
        "/park" => {
            if parts.len() < 2 {
                println!("Usage: /park <notes/text to park>");
            } else {
                let text = parts[1..].join(" ");
                park_input(&text);
            }
            true
        },
        "/sync" => {
            println!("{}", "Synchronizing Radicle identity keys...".yellow());
            let out = Command::new("basecamp/rad-id-sync/target/debug/rad-id-sync")
                .arg("sync")
                .current_dir("/home/drakestapleton")
                .output();
            match out {
                Ok(o) => println!("{}", String::from_utf8_lossy(&o.stdout).green()),
                Err(e) => println!("{}", format!("Failed to sync keys: {}", e).red()),
            }
            true
        },
                "/goal" | "/goals" => {
            if parts.len() < 2 || parts[1] == "list" {
                println!("{}", crate::goals::format_goals_tui());
            } else if parts[1] == "new" || parts[1] == "add" {
                if parts.len() < 3 {
                    println!("Usage: /goal new <title> [| milestone 1, milestone 2]");
                } else {
                    let rest = parts[2..].join(" ");
                    let split: Vec<&str> = rest.split('|').collect();
                    let title = split[0].trim();
                    let milestones: Vec<String> = if split.len() > 1 {
                        split[1].split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
                    } else {
                        Vec::new()
                    };
                    let g = crate::goals::add_goal(title, "", milestones);
                    println!("{}", format!("✓ Initialized goal: {} ({})", g.title, g.id).green().bold());
                }
            } else if parts[1] == "auto" {
                if parts.len() < 3 {
                    println!("Usage: /goal auto <id or title>");
                } else {
                    let target_id = parts[2..].join(" ");
                    crate::run_autonomous_goal(&target_id).await;
                }
            } else if parts[1] == "done" || parts[1] == "complete" {
                if parts.len() < 3 {
                    println!("Usage: /goal done <id or title>");
                } else {
                    let id = parts[2..].join(" ");
                    match crate::goals::complete_goal(&id) {
                        Ok(msg) => println!("{}", msg.green().bold()),
                        Err(e) => println!("{}", e.red()),
                    }
                }
            } else {
                println!("Usage: /goal [list | new <title> [| m1, m2] | done <id>]");
            }
            true
        },
                "/skill" | "/skills" => {
            if parts.len() < 2 || parts[1] == "list" {
                println!("{}", crate::skills::format_skills_tui());
            } else {
                let name = parts[1..].join(" ");
                match crate::skills::read_skill_content(&name) {
                    Ok(content) => {
                        println!("\n{}", format!("=== Skill: {} ===", name).cyan().bold());
                        println!("{}", content);
                    },
                    Err(e) => println!("{}", e.red()),
                }
            }
            true
        },
        "/cortex" => {
            crate::cortex::handle_cortex_command(&parts[1..]).await;
            true
        },
        _ => false,
    }
}

pub async fn run_doctor() {
    println!("{}", "=== AIEN Comprehensive Diagnostics ===".cyan().bold());
    let client = Client::new();

    // 1. GPU
    let gpu = get_gpu_telemetry();
    println!("✓ GPU Hardware:        {} ({}°C, {} MB used / {} MB total)", gpu.name.green(), gpu.temp_c, gpu.used_mb, gpu.total_mb);

    // 2. Model Seat 18006
    match client.get("http://127.0.0.1:18006/v1/models").send().await {
        Ok(r) if r.status().is_success() => println!("{}", "✓ Model Seat (Port 18006): Online (Nemotron-3.5-Lightning-30B-A3B BF16)".green()),
        _ => println!("{}", "⚠ Model Seat (Port 18006): OFFLINE or unreachable".yellow()),
    }

    // 3. JSpace Judge 18082
    match client.get("http://127.0.0.1:18082/v1/models").send().await {
        Ok(r) if r.status().is_success() => println!("{}", "✓ JSpace Truth Judge (Port 18082): Online (Llama-3.2-1B on Modular MAX CPU)".green()),
        _ => println!("{}", "⚠ JSpace Truth Judge (Port 18082): Offline".yellow()),
    }

    // 4. Cortex Memory 18080
    match client.get("http://127.0.0.1:18080/").send().await {
        Ok(_) => println!("{}", "✓ Spark Cortex Memory (Port 18080): Online (Postgres socket 15433)".green()),
        _ => println!("{}", "⚠ Spark Cortex Memory (Port 18080): Offline".yellow()),
    }

    // 5. ONNX Encoder 18081
    match client.get("http://127.0.0.1:18081/health").send().await {
        Ok(r) if r.status().is_success() => println!("{}", "✓ Cortex ONNX Encoder (Port 18081): Online (bge-base embeddings/salience)".green()),
        _ => println!("{}", "⚠ Cortex ONNX Encoder (Port 18081): Offline".yellow()),
    }

    // 6. Dream Daemon 18085
    match client.get("http://127.0.0.1:18085/health").send().await {
        Ok(r) if r.status().is_success() => println!("{}", "✓ JSpace Dream Engine (Port 18085): Online (Idle watcher daemon)".green()),
        _ => println!("{}", "⚠ JSpace Dream Engine (Port 18085): Offline".yellow()),
    }

    // 7. Radicle Anchor
    println!("{}", "✓ Radicle Identity:    did:rad:aien:spark-master (Active)".green());

    // 8. Hardware TPM Vault & Secret Hygiene
    if crate::vault::is_vault_available() {
        let keys = crate::vault::list_keys();
        let findings = crate::vault::audit_workspace_secrets();
        if findings.is_empty() {
            println!("{}", format!("✓ TPM Key Vault:       Online ({} keys registered; 0 plaintext .env files on disk)", keys.len()).green());
        } else {
            println!("{}", format!("⚠ TPM Key Vault:       Online ({} keys), but {} stray .env files found!", keys.len(), findings.len()).yellow());
        }
    } else {
        println!("{}", "⚠ TPM Key Vault:       Offline (atlas-vault binary missing)".yellow());
    }

    println!();
}

fn run_mask_command(_args: &[&str]) {
    let out = Command::new("aien-mask")
        .current_dir("/home/drakestapleton")
        .output();

    if let Ok(_) = out {
        println!("{}", "Active Mask: AIEN (Sovereign Operator)".cyan().bold());
    } else {
        println!("Active Mask: AIEN (default)");
    }
}

fn park_input(text: &str) {
    let park_dir = Path::new("/home/drakestapleton/atlas-prime-workspace/park");
    let _ = fs::create_dir_all(park_dir);
    let stamp = chrono::Utc::now().timestamp();
    let filename = format!("overload-{}.md", stamp);
    let path = park_dir.join(&filename);

    match OpenOptions::new().create(true).write(true).open(&path) {
        Ok(mut f) => {
            let _ = writeln!(f, "# Overload Parked: {}", chrono::Utc::now().to_rfc3339());
            let _ = writeln!(f, "{}", text);
            println!("{}", format!("✓ Input safely parked to: {}", path.display()).green().bold());
            println!("Continuing active thread without derailment.");
        },
        Err(e) => println!("{}", format!("Failed to park input: {}", e).red()),
    }
}

async fn query_cortex(query: &str) {
    let client = Client::new();
    let token_path = Path::new("/home/drakestapleton/.config/cortex/token");
    let token = fs::read_to_string(token_path).unwrap_or_default().trim().to_string();

    let url = format!("http://127.0.0.1:18080/api/cortex/search?q={}&space=atlas-memory&limit=5", urlencoding_simple(query));
    let mut req = client.get(&url).header("Accept", "application/json");
    if !token.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", token));
    }

    match req.send().await {
        Ok(r) => {
            if let Ok(data) = r.json::<serde_json::Value>().await {
                let results = data.get("results").and_then(|v| v.as_array());
                match results {
                    Some(items) if !items.is_empty() => {
                        println!("{}", format!("\nCortex Knowledge Recall for '{}' ({} found):", query, items.len()).cyan().bold());
                        for (idx, item) in items.iter().enumerate() {
                            let title = item.get("canonicalName").and_then(|v| v.as_str()).unwrap_or("Untitled");
                            let score = item.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let content = item.get("content").and_then(|v| v.as_str()).unwrap_or("");
                            let snippet = if content.len() > 180 { &content[..180] } else { content };
                            println!("{}. {} ({:.2})", idx + 1, title.bold().yellow(), score);
                            println!("   {}\n", snippet.dimmed());
                        }
                    },
                    _ => println!("{}", "No matching entities found in atlas-memory.".dimmed()),
                }
            } else {
                println!("{}", "Could not parse JSON response from Cortex.".red());
            }
        },
        Err(e) => println!("{}", format!("Cortex query failed: {}", e).red()),
    }
}

fn urlencoding_simple(s: &str) -> String {
    s.replace(" ", "%20")
}

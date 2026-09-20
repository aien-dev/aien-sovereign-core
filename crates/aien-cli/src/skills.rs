use colored::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: String,
    pub has_scripts: bool,
}

fn extract_frontmatter_field(content: &str, field: &str) -> Option<String> {
    let mut in_frontmatter = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "---" {
            if in_frontmatter {
                break;
            } else {
                in_frontmatter = true;
                continue;
            }
        }
        if in_frontmatter {
            if let Some(rest) = trimmed.strip_prefix(&format!("{}:", field)) {
                return Some(
                    rest.trim()
                        .trim_matches('"')
                        .trim_matches('\x27')
                        .to_string(),
                );
            }
        }
    }
    None
}

fn scan_dir_for_skills(dir: &Path, skills: &mut Vec<Skill>, seen_names: &mut HashSet<String>) {
    if !dir.is_dir() {
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let skill_md = path.join("SKILL.md");
                if skill_md.is_file() {
                    let content = fs::read_to_string(&skill_md).unwrap_or_default();
                    let name = extract_frontmatter_field(&content, "name").unwrap_or_else(|| {
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string()
                    });
                    let norm_name = name.to_lowercase();
                    if seen_names.insert(norm_name) {
                        let desc = extract_frontmatter_field(&content, "description")
                            .unwrap_or_else(|| "No description provided.".to_string());
                        let has_scripts = path.join("scripts").is_dir();
                        skills.push(Skill {
                            name,
                            description: desc,
                            path: skill_md.display().to_string(),
                            has_scripts,
                        });
                    }
                } else {
                    // Check one level deeper (e.g. modular-skills/benchmark-model)
                    if let Ok(sub_entries) = fs::read_dir(&path) {
                        for sub_entry in sub_entries.flatten() {
                            let sub_path = sub_entry.path();
                            if sub_path.is_dir() {
                                let sub_skill_md = sub_path.join("SKILL.md");
                                if sub_skill_md.is_file() {
                                    let content =
                                        fs::read_to_string(&sub_skill_md).unwrap_or_default();
                                    let name = extract_frontmatter_field(&content, "name")
                                        .unwrap_or_else(|| {
                                            sub_path
                                                .file_name()
                                                .unwrap_or_default()
                                                .to_string_lossy()
                                                .to_string()
                                        });
                                    let norm_name = name.to_lowercase();
                                    if seen_names.insert(norm_name) {
                                        let desc =
                                            extract_frontmatter_field(&content, "description")
                                                .unwrap_or_else(|| {
                                                    "No description provided.".to_string()
                                                });
                                        let has_scripts = sub_path.join("scripts").is_dir();
                                        skills.push(Skill {
                                            name,
                                            description: desc,
                                            path: sub_skill_md.display().to_string(),
                                            has_scripts,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn discover_skills() -> Vec<Skill> {
    let mut skills = Vec::new();
    let mut seen_names = HashSet::new();
    let platform = crate::platform::PlatformContext::detect();

    // Priority 1: Current workspace .agents/skills and skills
    let cwd = std::env::current_dir().unwrap_or_else(|_| platform.home_dir.clone());
    scan_dir_for_skills(&cwd.join(".agents/skills"), &mut skills, &mut seen_names);
    scan_dir_for_skills(&cwd.join("skills"), &mut skills, &mut seen_names);

    // Priority 2: Home skills (~/skills)
    let home_skills = platform.home_dir.join("skills");
    scan_dir_for_skills(&home_skills, &mut skills, &mut seen_names);

    // Priority 3: Home .agents/skills (~/.agents/skills)
    let home_agents_skills = platform.home_dir.join(".agents/skills");
    scan_dir_for_skills(&home_agents_skills, &mut skills, &mut seen_names);

    // Priority 4: Gemini global config skills (~/.gemini/config/skills)
    let gemini_skills = platform.home_dir.join(".gemini/config/skills");
    scan_dir_for_skills(&gemini_skills, &mut skills, &mut seen_names);

    skills
}

pub fn format_skills_progressive_summary() -> String {
    let skills = discover_skills();
    if skills.is_empty() {
        return String::new();
    }

    let mut out = String::from("\nAVAILABLE AGENT SKILLS (Progressive Disclosure):\n");
    out.push_str("To activate and inspect full instructions for any skill, use the 'skill' tool with {\"action\": \"read\", \"name\": \"<skill_name>\"}.\n");
    for s in skills {
        out.push_str(&format!("- {}: {}\n", s.name, s.description));
    }
    out
}

pub fn format_skills_tui() -> String {
    let skills = discover_skills();
    let mut out = String::new();
    out.push_str(&format!(
        "{}\n",
        "=== AIEN Antigravity Skill Registry ===".cyan().bold()
    ));

    if skills.is_empty() {
        out.push_str(&format!(
            "{}\n",
            "No skills currently discovered. Create a folder with SKILL.md in ~/skills/ or .agents/skills/ to register one.".dimmed()
        ));
        return out;
    }

    for (i, s) in skills.iter().enumerate() {
        let scripts_badge = if s.has_scripts {
            "[scripts]".yellow()
        } else {
            "".normal()
        };
        out.push_str(&format!(
            "{}. {} {} ({})\n",
            i + 1,
            s.name.bold().green(),
            scripts_badge,
            s.path.dimmed()
        ));
        out.push_str(&format!("   {}\n\n", s.description.dimmed()));
    }

    out
}

pub fn read_skill_content(name: &str) -> Result<String, String> {
    let skills = discover_skills();
    let norm = name.trim().to_lowercase();

    for s in skills {
        if s.name.to_lowercase() == norm {
            return fs::read_to_string(&s.path).map_err(|e| e.to_string());
        }
    }
    Err(format!(
        "Skill '{}' not found in discovered skill paths",
        name
    ))
}

pub fn skills_dispatch_tool(args: &Value) -> Value {
    let action = args.get("action").and_then(Value::as_str).unwrap_or("list");

    match action {
        "list" => {
            let skills = discover_skills();
            json!({
                "status": "ok",
                "total": skills.len(),
                "skills": skills
            })
        }
        "read" | "load" => {
            let name = args.get("name").and_then(Value::as_str).unwrap_or("");
            match read_skill_content(name) {
                Ok(content) => json!({"status": "ok", "name": name, "content": content}),
                Err(e) => json!({"status": "error", "error": e}),
            }
        }
        "optimize" => {
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("atlas-skillopt");
            optimize_skill(name)
        }
        _ => json!({"error": format!("Unknown skill action '{}'", action)}),
    }
}

pub fn optimize_skill(name: &str) -> Value {
    let platform = crate::platform::PlatformContext::detect();
    let script = platform.home_dir.join("atlas-skillopt-stage.sh");
    if !script.exists() {
        return json!({"status": "error", "error": "atlas-skillopt-stage.sh not found"});
    }
    let output = std::process::Command::new("bash").arg(&script).output();
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            json!({
                "status": if out.status.success() { "ok" } else { "failed" },
                "target_skill": name,
                "stdout": stdout,
                "stderr": stderr
            })
        }
        Err(e) => json!({"status": "error", "error": e.to_string()}),
    }
}

pub fn run_optimize_cli(name: &str) {
    println!(
        "{}",
        format!(
            "Starting Microsoft SkillOpt self-improvement loop for {}...",
            name
        )
        .cyan()
        .bold()
    );
    let res = optimize_skill(name);
    println!("{}", serde_json::to_string_pretty(&res).unwrap_or_default());
}

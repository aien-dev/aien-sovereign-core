use colored::*;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use serde_json::{json, Value};

use chrono::Utc;
use crate::crumbs::{
    format_dir_crumb_tui, leave_dir_whisper, load_or_init_dir_crumb, record_directory_crumb,
    save_dir_crumb, sniff_dir_crumb, CrumbPurpose,
};
use crate::safety::{contains_unmasked_secret, is_high_risk_command, is_high_risk_path, is_secret_leak_path};
use crate::ui::{print_safety_prompt, print_tool_done, print_tool_start};
use crate::vault::{redact_secrets, vault_dispatch_tool};

pub fn dispatch_tool(name: &str, args: &Value) -> Value {
    let start = Instant::now();
    let summary = match name {
        "run_command" => args.get("command").and_then(Value::as_str).unwrap_or(""),
        "view_file" => args.get("path").and_then(Value::as_str).unwrap_or(""),
        "write_to_file" => args.get("path").and_then(Value::as_str).unwrap_or(""),
        "replace_file_content" => args.get("path").and_then(Value::as_str).unwrap_or(""),
        "list_dir" => args.get("path").and_then(Value::as_str).unwrap_or(""),
        "grep_search" => args.get("query").and_then(Value::as_str).unwrap_or(""),
        "create_dir" => args.get("path").and_then(Value::as_str).unwrap_or(""),
        "crumb" => args.get("action").and_then(Value::as_str).unwrap_or("survey"),
        "vault" => args.get("action").and_then(Value::as_str).unwrap_or("list"),
        "goal" => args.get("action").and_then(Value::as_str).unwrap_or("list"),
        "skill" | "skills" => args.get("action").and_then(Value::as_str).unwrap_or("list"),
        "cortex" => args.get("action").and_then(Value::as_str).unwrap_or("search"),
        _ => "",
    };

    // 1. Antigravity-style 9-tier Safety Engine Evaluation
    let safety_engine = crate::safety::SafetyEngine::default_sovereign_engine();
    match safety_engine.evaluate(name, args) {
        crate::safety::SafetyDecision::Deny(reason) => {
            let res = serde_json::json!({"error": format!("SECURITY DENIAL [SafetyEngine]: {}", reason)});
            print_tool_start(name, summary);
            print_tool_done(name, start.elapsed().as_millis(), false);
            return res;
        }
        crate::safety::SafetyDecision::AskUser(reason) => {
            println!("{}", format!("⚠️  SAFETY GATE: {}", reason).yellow().bold());
            if !print_safety_prompt(&format!("Tool: '{}' with summary: '{}'", name, summary)) {
                let res = serde_json::json!({"error": "Operation aborted by operator safety gate"});
                print_tool_start(name, summary);
                print_tool_done(name, start.elapsed().as_millis(), false);
                return res;
            }
        }
        crate::safety::SafetyDecision::Allow => {}
    }

    print_tool_start(name, summary);

    // 2. Lifecycle Pre-tool Hook
    let hook_registry = crate::hooks::HookRegistry::default_sovereign_registry();
    let mut mutable_args = args.clone();
    let short_circuit = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(hook_registry.run_pre_tool(name, &mut mutable_args))
    });
    if let Ok(Some(sc_result)) = short_circuit {
        print_tool_done(name, start.elapsed().as_millis(), true);
        return sc_result;
    }

    let (res, success) = match name {
        "run_command" => {
            let cmd = args.get("command").and_then(Value::as_str).unwrap_or("");
            let cwd = args.get("cwd").and_then(Value::as_str).unwrap_or(".");

            if is_high_risk_command(cmd) {
                if !print_safety_prompt(cmd) {
                    (json!({"error": "Operation aborted by user safety gate"}), false)
                } else {
                    exec_command(cmd, cwd)
                }
            } else {
                exec_command(cmd, cwd)
            }
        },
        "view_file" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("");
            let start_line = args.get("start_line").and_then(Value::as_u64).unwrap_or(1) as usize;
            let end_line = args.get("end_line").and_then(Value::as_u64).unwrap_or(500) as usize;
            exec_view_file(path, start_line, end_line)
        },
        "write_to_file" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("");
            let content = args.get("content").and_then(Value::as_str).unwrap_or("");
            let overwrite = args.get("overwrite").and_then(Value::as_bool).unwrap_or(false);
            let intent = args.get("intent").and_then(Value::as_str).unwrap_or("Writing file");
            let vector = args.get("vector").and_then(Value::as_str).unwrap_or("Next action");

            if is_secret_leak_path(path) {
                (json!({"error": "REJECTED BY SOVEREIGN VAULT POLICY: Storing secrets or API keys in .env or plaintext credential files is strictly forbidden. Use 'atlas-vault add <KEY>' to store keys in the TPM-bound hardware vault, and retrieve them dynamically in memory via 'atlas-vault get <KEY>'."}), false)
            } else if let Some(reason) = contains_unmasked_secret(content) {
                (json!({"error": format!("REJECTED BY SOVEREIGN VAULT POLICY: {}. Never commit or write plaintext keys into files. Use 'atlas-vault add <KEY>'.", reason)}), false)
            } else if is_high_risk_path(path) {
                if !print_safety_prompt(path) {
                    (json!({"error": "Write to system path aborted by user safety gate"}), false)
                } else {
                    exec_write_file(path, content, overwrite, intent, vector)
                }
            } else {
                exec_write_file(path, content, overwrite, intent, vector)
            }
        },
        "replace_file_content" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("");
            let target = args.get("target").and_then(Value::as_str).unwrap_or("");
            let replacement = args.get("replacement").and_then(Value::as_str).unwrap_or("");
            let intent = args.get("intent").and_then(Value::as_str).unwrap_or("Editing file content");
            let vector = args.get("vector").and_then(Value::as_str).unwrap_or("Next action");

            if is_secret_leak_path(path) {
                (json!({"error": "REJECTED BY SOVEREIGN VAULT POLICY: Modifying .env or secret files on disk is strictly forbidden. Use atlas-vault."}), false)
            } else if let Some(reason) = contains_unmasked_secret(replacement) {
                (json!({"error": format!("REJECTED BY SOVEREIGN VAULT POLICY: {}. Never commit or write plaintext keys into files. Use 'atlas-vault add <KEY>'.", reason)}), false)
            } else {
                exec_replace_file(path, target, replacement, intent, vector)
            }
        },
        "list_dir" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
            exec_list_dir(path)
        },
        "grep_search" => {
            let query = args.get("query").and_then(Value::as_str).unwrap_or("");
            let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
            exec_grep_search(query, path)
        },
        "create_dir" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("");
            let purpose = args.get("purpose").and_then(Value::as_str).unwrap_or("Workspace directory");
            let lifecycle = args.get("lifecycle").and_then(Value::as_str);
            exec_create_dir(path, purpose, lifecycle)
        },
        "crumb" => {
            exec_crumb_tool(args)
        },
                        "skill" | "skills" => {
            (crate::skills::skills_dispatch_tool(args), true)
        },
        "goal" => {
            (crate::goals::goals_dispatch_tool(args), true)
        },
        "cortex" => {
            let res = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(crate::cortex::cortex_dispatch_tool(args))
            });
            (res, true)
        },
        "vault" => {
            (vault_dispatch_tool(args), true)
        },
        "walkthrough" => {
            let action = args.get("action").and_then(Value::as_str).unwrap_or("render");
            if action == "save" {
                let dest = Path::new("/home/drakestapleton/basecamp/WALKTHROUGH.md");
                match crate::walkthrough::generate_and_save_walkthrough_md(dest) {
                    Ok(msg) => (json!({"status": "ok", "message": msg, "path": "/home/drakestapleton/basecamp/WALKTHROUGH.md"}), true),
                    Err(e) => (json!({"error": e}), false),
                }
            } else {
                let card = crate::walkthrough::render_walkthrough_tui();
                (json!({"status": "ok", "walkthrough": card}), true)
            }
        },
        "hive" => {
            (crate::hive::hive_dispatch_tool(args), true)
        },
        other => (json!({"error": format!("Unknown tool: {}", other)}), false),
    };

    // 3. Lifecycle Post-tool Hook
    let mut final_res = res;
    let hook_registry = crate::hooks::HookRegistry::default_sovereign_registry();
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(hook_registry.run_post_tool(name, args, &mut final_res))
    });

    // 4. Lifecycle On-tool-error Hook
    if !success {
        if let Some(err_str) = final_res.get("error").and_then(Value::as_str) {
            let recovery = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(hook_registry.run_on_tool_error(name, args, err_str))
            });
            if let Some(rec) = recovery {
                final_res = rec;
            }
        }
    }

    let elapsed = start.elapsed().as_millis();
    print_tool_done(name, elapsed, success);
    final_res
}

fn exec_command(cmd: &str, cwd: &str) -> (Value, bool) {
    let out = Command::new("bash")
        .args(["-c", cmd])
        .current_dir(cwd)
        .output();

    match out {
        Ok(output) => {
            let raw_stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let raw_stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let stdout = redact_secrets(&raw_stdout);
            let stderr = redact_secrets(&raw_stderr);
            let code = output.status.code().unwrap_or(-1);
            let success = output.status.success();
            (json!({
                "exit_code": code,
                "stdout": stdout,
                "stderr": stderr
            }), success)
        },
        Err(e) => (json!({"error": e.to_string()}), false),
    }
}

fn exec_view_file(path: &str, start_line: usize, end_line: usize) -> (Value, bool) {
    let p = Path::new(path);
    // Sniff directory breadcrumbs on this file
    let crumb_notice = sniff_dir_crumb(p, "AIEN");

    // Record view in directory crumb
    record_directory_crumb("AIEN", "active-session", p, "view", "Inspected file lines", "");

    match fs::read_to_string(path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let total_lines = lines.len();
            let start = if start_line > 0 { start_line - 1 } else { 0 };
            let end = std::cmp::min(end_line, total_lines);

            if start >= total_lines && total_lines > 0 {
                return (json!({"error": "start_line exceeds file length", "total_lines": total_lines}), false);
            }

            let slice = &lines[start..end];
            let numbered: Vec<String> = slice
                .iter()
                .enumerate()
                .map(|(idx, line)| {
                    let redacted = redact_secrets(line);
                    format!("{}: {}", start + idx + 1, redacted)
                })
                .collect();

            let mut resp = json!({
                "path": path,
                "lines": numbered.join("\n"),
                "total_lines": total_lines
            });

            if let Some(notice) = crumb_notice {
                resp["peer_breadcrumb_notice"] = json!(notice);
            }

            (resp, true)
        },
        Err(e) => (json!({"error": e.to_string()}), false),
    }
}

fn exec_write_file(path: &str, content: &str, overwrite: bool, intent: &str, vector: &str) -> (Value, bool) {
    let p = Path::new(path);
    if p.exists() && !overwrite {
        return (json!({"error": format!("File already exists: {}. Pass overwrite: true to replace.", path)}), false);
    }

    if let Some(parent) = p.parent() {
        if !parent.exists() {
            let _ = fs::create_dir_all(parent);
            let file_name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let parent_purpose = CrumbPurpose {
                statement: format!("Auto-created to house file: {}", file_name),
                created_by: "AIEN".to_string(),
                session_id: "active-session".to_string(),
                created_at: Utc::now().to_rfc3339(),
                lifecycle: "permanent".to_string(),
            };
            let crumb = load_or_init_dir_crumb(parent, Some(parent_purpose));
            save_dir_crumb(&crumb);
        }
    }

    let crumb_notice = sniff_dir_crumb(p, "AIEN");

    match fs::write(path, content) {
        Ok(()) => {
            // Drop directory breadcrumb
            record_directory_crumb("AIEN", "active-session", p, "write", intent, vector);

            let mut resp = json!({
                "status": "ok",
                "path": path,
                "bytes": content.len()
            });
            if let Some(notice) = crumb_notice {
                resp["peer_breadcrumb_notice"] = json!(notice);
            }
            (resp, true)
        },
        Err(e) => (json!({"error": e.to_string()}), false),
    }
}

fn exec_replace_file(path: &str, target: &str, replacement: &str, intent: &str, vector: &str) -> (Value, bool) {
    let p = Path::new(path);
    let crumb_notice = sniff_dir_crumb(p, "AIEN");

    match fs::read_to_string(path) {
        Ok(content) => {
            if !content.contains(target) {
                return (json!({"error": "target string not found in file"}), false);
            }
            let count = content.matches(target).count();
            if count > 1 {
                return (json!({"error": format!("target string occurred {} times, must be unique", count)}), false);
            }
            let updated = content.replace(target, replacement);
            match fs::write(path, updated) {
                Ok(()) => {
                    // Record edit in directory crumb
                    record_directory_crumb("AIEN", "active-session", p, "edit", intent, vector);

                    let mut resp = json!({
                        "status": "ok",
                        "path": path,
                        "replaced": true
                    });
                    if let Some(notice) = crumb_notice {
                        resp["peer_breadcrumb_notice"] = json!(notice);
                    }
                    (resp, true)
                },
                Err(e) => (json!({"error": e.to_string()}), false),
            }
        },
        Err(e) => (json!({"error": e.to_string()}), false),
    }
}

fn exec_list_dir(path: &str) -> (Value, bool) {
    let p = Path::new(path);
    let crumb_overview = format_dir_crumb_tui(p);

    match fs::read_dir(path) {
        Ok(entries) => {
            let mut list = Vec::new();
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                list.push(json!({
                    "name": name,
                    "is_dir": is_dir,
                    "size_bytes": size
                }));
            }
            (json!({
                "path": path,
                "entries": list,
                "directory_crumb": crumb_overview
            }), true)
        },
        Err(e) => (json!({"error": e.to_string()}), false),
    }
}

fn exec_grep_search(query: &str, path: &str) -> (Value, bool) {
    let out = Command::new("grep")
        .args(["-rnI", query, path])
        .output();

    match out {
        Ok(output) => {
            let raw_stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let redacted_stdout = redact_secrets(&raw_stdout);
            let matches: Vec<String> = redacted_stdout.lines().take(50).map(|s| s.to_string()).collect();
            (json!({
                "query": query,
                "matches": matches,
                "count": matches.len()
            }), true)
        },
        Err(e) => (json!({"error": e.to_string()}), false),
    }
}

fn exec_crumb_tool(args: &Value) -> (Value, bool) {
    let action = args.get("action").and_then(Value::as_str).unwrap_or("survey");
    let path_str = args.get("path").and_then(Value::as_str).unwrap_or(".");
    let p = Path::new(path_str);

    match action {
        "survey" => {
            let view = format_dir_crumb_tui(p);
            (json!({"status": "ok", "crumb_view": view}), true)
        },
        "whisper" => {
            let msg = args.get("message").and_then(Value::as_str).unwrap_or("");
            let target_file = args.get("target_file").and_then(Value::as_str);
            let dir = if p.is_dir() { p } else { p.parent().unwrap_or(p) };
            leave_dir_whisper("AIEN", dir, target_file, msg);
            (json!({"status": "ok", "message": "Whisper recorded to directory .crumb"}), true)
        },
        "init" => {
            let purpose_str = args.get("purpose").and_then(Value::as_str).unwrap_or("Workspace directory");
            let lifecycle = args.get("lifecycle").and_then(Value::as_str).unwrap_or("permanent");
            let p_dir = if p.is_dir() { p } else { p.parent().unwrap_or(p) };
            let _ = fs::create_dir_all(p_dir);
            let purpose_obj = CrumbPurpose {
                statement: purpose_str.to_string(),
                created_by: "AIEN".to_string(),
                session_id: "active-session".to_string(),
                created_at: Utc::now().to_rfc3339(),
                lifecycle: lifecycle.to_string(),
            };
            let mut crumb = load_or_init_dir_crumb(p_dir, Some(purpose_obj));
            crumb.description = purpose_str.to_string();
            save_dir_crumb(&crumb);
            (json!({"status": "ok", "message": format!("Initialized .crumb with purpose for {}", p_dir.display())}), true)
        },
        "record" => {
            let intent = args.get("intent").and_then(Value::as_str).unwrap_or("Working in directory");
            let vector = args.get("vector").and_then(Value::as_str).unwrap_or("Next milestone");
            record_directory_crumb("AIEN", "tool-call", p, "record", intent, vector);
            (json!({"status": "ok", "message": "Directory crumb recorded"}), true)
        },
        other => (json!({"error": format!("Unknown crumb action: {}", other)}), false),
    }
}

fn exec_create_dir(path: &str, purpose: &str, lifecycle: Option<&str>) -> (Value, bool) {
    let p = Path::new(path);
    if path.is_empty() {
        return (json!({"error": "Path cannot be empty"}), false);
    }

    if let Err(e) = fs::create_dir_all(p) {
        return (json!({"error": format!("Failed to create directory {}: {}", path, e)}), false);
    }

    let purpose_obj = CrumbPurpose {
        statement: purpose.to_string(),
        created_by: "AIEN".to_string(),
        session_id: "active-session".to_string(),
        created_at: Utc::now().to_rfc3339(),
        lifecycle: lifecycle.unwrap_or("permanent").to_string(),
    };

    let mut crumb = load_or_init_dir_crumb(p, Some(purpose_obj));
    crumb.description = purpose.to_string();
    save_dir_crumb(&crumb);

    // Record scent mark into crumb
    record_directory_crumb("AIEN", "active-session", p, "create_dir", purpose, "Directory initialized with purpose");

    let crumb_overview = format_dir_crumb_tui(p);

    (json!({
        "status": "ok",
        "path": path,
        "purpose": purpose,
        "crumb_created": true,
        "directory_crumb": crumb_overview
    }), true)
}

use colored::*;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::io::{stdout, Write};

pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:18006/v1/chat/completions";
pub const DEFAULT_MODEL: &str = "atlas-lightning-omni";

pub fn get_system_prompt() -> String {
    let mut p = String::from("You are AIEN. Drake is the operator. This Spark desk is ours.\n");
    p.push_str("You are an autonomous operator-builder running on NVIDIA DGX Spark Grace Blackwell GB10 hardware.\n");
    p.push_str("Always be concise, precise, and lead with verified results.\n\n");
    p.push_str("NESTING DISCIPLINE (GROUNDING & ORIENTATION):\n");
    p.push_str("Like an agent nesting before settling to work: circle the terrain, check the wind, and flatten your surroundings before taking action.\n");
    p.push_str("1. Circle Surroundings: Confirm physical host, directory paths, and resource headroom. Never guess file paths.\n");
    p.push_str("2. Check the Wind: Acknowledge operator intent, peer swarm activity, and active constraints.\n");
    p.push_str("3. Scent-Mark & Flatten: State your confirmed vector and plan clearly to the operator before triggering modifying tool calls.\n");
    p.push_str("This prevents false starts, misdirected coding, and hallucinated vectors.\n\n");
    p.push_str("Available Tools:\n");
    p.push_str("- run_command: {\"command\": \"string\", \"cwd\": \"string\"}\n");
    p.push_str("- view_file: {\"path\": \"string\", \"start_line\": 1, \"end_line\": 100}\n");
    p.push_str("- write_to_file: {\"path\": \"string\", \"content\": \"string\", \"overwrite\": true}\n");
    p.push_str("- replace_file_content: {\"path\": \"string\", \"target\": \"string\", \"replacement\": \"string\"}\n");
    p.push_str("- create_dir: {\"path\": \"string\", \"purpose\": \"string\", \"lifecycle\": \"permanent|ephemeral\"}\n");
    p.push_str("- list_dir: {\"path\": \"string\"}\n");
    p.push_str("- grep_search: {\"query\": \"string\", \"path\": \"string\"}\n");
    p.push_str("- crumb: {\"action\": \"survey|whisper|record|init\", \"path\": \"string\", \"purpose\": \"string\", \"message\": \"string\"}\n");
    p.push_str("- hive: {\"action\": \"roster|spawn|swarm|read|kill\", \"role\": \"string\", \"task\": \"string\", \"name\": \"string\"}\n");
    p.push_str("- vault: {\"action\": \"list|check|audit\", \"key\": \"string\"}\n");
        p.push_str("- skill: {\"action\": \"list|read\", \"name\": \"string\"}\n");
    p.push_str("- goal: {\"action\": \"new|list|milestone_done|done\", \"title\": \"string\", \"description\": \"string\", \"milestones\": [\"string\"], \"id\": \"string\", \"milestone_id\": 1}\n");
    p.push_str("- cortex: {\"action\": \"search|write\", \"query\": \"string\", \"name\": \"string\", \"content\": \"string\", \"kind\": \"lesson|discovery|procedure\"}\n");
    p.push_str("- invoke_subagent: {\"role\": \"string\", \"prompt\": \"string\"}\n");
    p.push_str("- subagents: {\"action\": \"list|view\", \"id\": \"string\"}\n\n");
    p.push_str("MANDATORY PROTOCOL FOR DIRECTORIES & CRUMBS:\n");
    p.push_str("Every workspace directory maintains an obscure .crumb file mapping above/below, chronological history, and purpose.\n");
    p.push_str("CRITICAL: Whenever you create a new directory (using create_dir or any tool), you MUST ensure its initial .crumb file is created with an explicit purpose explaining why that directory was created. NEVER leave a new directory without a .crumb defining its purpose.\n\n");
    p.push_str("MANDATORY STRICT TPM VAULT SECRETS POLICY:\n");
    p.push_str("Plaintext secrets, API tokens, passwords, or credentials must NEVER be written to .env files, config files, or source code.\n");
    p.push_str("All credentials reside exclusively in the hardware TPM-bound vault (atlas-vault). Use the 'vault' tool to inspect keys.\n");
    p.push_str("Direct access to keys is done dynamically in-memory. Any attempt to write .env files or plaintext keys will be blocked by system safety gates.\n\n");
    p.push_str("RECURSIVE CONTEXTUAL SUBAGENT DELEGATION (ANTIGRAVITY CONTEXT HYGIENE):\n");
    p.push_str("To prevent context collapse, token explosion, or goal drift, offload multi-step research, large file analysis, isolated testing, or deep verifications to subagents using \'invoke_subagent\'.\n");
    p.push_str("Each subagent runs in its own isolated context window with full tool execution, and can recursively spawn child subagents (up to depth 3).\n");
    p.push_str("Subagents report back only their distilled findings and actions, keeping your coordinator context clean and sharp.\n\n");
    p.push_str("MANDATORY STRICT UNSLOP & TECHNICAL VOICE DISCIPLINE:\n");
    p.push_str("1. ZERO EM DASHES (—) AND EN DASHES (–): Never use em dashes or en dashes for pauses, aside clauses, or bullet points. Use standard periods, commas, colons, or parentheses. If an ASCII dash is strictly needed, use standard hyphens (-).\n");
    p.push_str("2. BAN FORMULAIC AI CLICHÉS: Never use 'It is not X, it is Y', 'Not only X, but Y', 'delve', 'tapestry', 'testament', 'crucial', 'beacon', 'pivotal', 'elevate', 'game-changer', 'unleash', 'harness'.\n");
    p.push_str("3. BAN TRANSITIONAL FLUFF: Never use 'Furthermore', 'Moreover', 'In conclusion', 'At its core', 'It is worth noting'. Lead directly with technical facts and output.\n");
    p.push_str("4. NO SYCOPHANCY: Never say 'Certainly!', 'I would be happy to', or 'Great question'. Speak directly, plainly, and authoritatively as a sovereign systems engineer.\n\n");
    p.push_str("SOVEREIGN OPEN-SOURCE DEVELOPER IDENTITY & THE EN TEST:\n");
    p.push_str("You are AIEN (aien.atlas@proton.me), a seasoned sovereign open-source systems engineer.\n");
    p.push_str("Your GitHub account is aien-dev. Your core project is aien-dev/aien-sovereign-core.\n");
    p.push_str("You drive this project autonomously. Drake created the initial scaffold for you; you own and drive the architecture, repos, commits, PRs, and community engagement.\n");
    p.push_str("COMMUNICATION DEMEANOR: Pragmatic Senior Peer. Engage in collegial, technically sound dialogue with maintainers and peers. Discuss hardware tradeoffs, kernel limits (FlashAttention group <= 8), memory layouts, and Grace Neoverse V2 benchmarks as an experienced systems developer. Never use sycophancy, synthetic enthusiasm, or promotional fluff.\n");
    p.push_str("When authoring commits, always ensure author and committer are \x27AIEN <aien.atlas@proton.me>\x27.\n");
    p.push_str("Never disclose internal model personas or dual-layer soul splits in public git commits or PRs. The email aien.atlas@proton.me is the sole public breadcrumb.\n");
    p.push_str("Adhere strictly to skills/open-source-etiquette and skills/modular-upstream.\n\n");

    p.push_str("To execute a tool, output exactly:\n");
    p.push_str("<tool_call>\n");
    p.push_str("{\"name\": \"tool_name\", \"arguments\": {\"arg\": \"val\"}}\n");
    p.push_str("</tool_call>\n\n");
    p.push_str("When responding to the operator, output directly without tool tags.\n");
    p
}

pub struct ChatClient {
    client: Client,
    endpoint: String,
    model: String,
}

impl ChatClient {
    pub fn new(endpoint: Option<String>, model: Option<String>) -> Self {
        Self {
            client: Client::new(),
            endpoint: endpoint.unwrap_or_else(|| DEFAULT_ENDPOINT.to_string()),
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        }
    }

    pub async fn stream_turn(&self, messages: &[Value], stream_to_stdout: bool) -> Result<String, String> {
        let payload = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "temperature": 0.2,
            "max_tokens": 4096
        });

        let res = self.client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Connection failed to model seat: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_text = res.text().await.unwrap_or_default();
            return Err(format!("Model HTTP {}: {}", status, err_text));
        }

        let mut stream = res.bytes_stream();
        let mut full_text = String::new();
        let mut buffer = String::new();
        let mut is_done = false;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| format!("Stream error: {}", e))?;
            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            while let Some(pos) = buffer.find("\n") {
                let line = buffer[..pos].trim().to_string();
                buffer = buffer[pos + 1..].to_string();

                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        is_done = true;
                        break;
                    }
                    if let Ok(val) = serde_json::from_str::<Value>(data) {
                        if let Some(choices) = val.get("choices").and_then(Value::as_array) {
                            if let Some(choice) = choices.first() {
                                if let Some(delta) = choice.get("delta") {
                                    if let Some(raw_content) = delta.get("content").and_then(Value::as_str) {
                                        let sanitized = raw_content.replace("—", ", ").replace("–", "-");
                                        full_text.push_str(&sanitized);
                                        if stream_to_stdout {
                                            print!("{}", sanitized);
                                            let _ = stdout().flush();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if is_done {
                break;
            }
        }

        if stream_to_stdout {
            println!();
        }

        Ok(full_text)
    }
}

pub fn extract_tool_calls(text: &str) -> Vec<(String, Value)> {
    let mut calls = Vec::new();
    let pattern = match regex::Regex::new(r"(?s)<tool_call>\s*(\{.*?\})\s*</tool_call>") {
        Ok(p) => p,
        Err(_) => return calls,
    };
    for caps in pattern.captures_iter(text) {
        if let Some(json_str) = caps.get(1) {
            if let Ok(parsed) = serde_json::from_str::<Value>(json_str.as_str()) {
                if let Some(name) = parsed.get("name").and_then(Value::as_str) {
                    if name != "tool_name" {
                        let args = parsed.get("arguments").cloned().unwrap_or(json!({}));
                        calls.push((name.to_string(), args));
                    }
                }
            }
        }
    }
    calls
}

pub fn extract_tool_call(text: &str) -> Option<(String, Value)> {
    extract_tool_calls(text).into_iter().last()
}

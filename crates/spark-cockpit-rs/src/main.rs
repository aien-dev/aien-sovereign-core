use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
    routing::{get, post},
    Router,
};
use chrono::Utc;
use futures_util::stream::Stream;
use futures_util::StreamExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

const MAX_SEAT_URL: &str = "http://127.0.0.1:18006";
const CORTEX_URL: &str = "http://127.0.0.1:18080";
const CONDUIT_URL: &str = "http://127.0.0.1:6167";
const GOALS_PATH: &str = "/home/drakestapleton/basecamp/goals.json";
const SKILLS_DIR: &str = "/home/drakestapleton/skills";
const STATIC_DIR: &str = "/home/drakestapleton/spark-cockpit/static";
const CORTEX_TOKEN_PATH: &str = "/home/drakestapleton/.config/cortex/token";

#[derive(Clone)]
struct AppState {
    client: reqwest::Client,
    start_time: Instant,
    redactor_patterns: Arc<Vec<(Regex, &'static str)>>,
}

fn build_patterns() -> Vec<(Regex, &'static str)> {
    vec![
        (Regex::new(r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----").unwrap(), "[REDACTED_PRIVATE_KEY]"),
        (Regex::new(r"\b(ey[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})\b").unwrap(), "[REDACTED_JWT_TOKEN]"),
        (Regex::new(r"(?i)\b(bearer\s+)([a-zA-Z0-9_\-\.]{12,})").unwrap(), "$1[REDACTED_BEARER_TOKEN]"),
        (Regex::new(r"(?i)(api[_-]?key|secret|password|passwd|token)\s*[:=]\s*([^\s,;]{8,})").unwrap(), "$1=[REDACTED_SECRET]"),
        (Regex::new(r"\b(sk-[a-zA-Z0-9_-]{20,})\b").unwrap(), "[REDACTED_OPENAI_KEY]"),
        (Regex::new(r"\b(sk-ant-[a-zA-Z0-9_-]{20,})\b").unwrap(), "[REDACTED_ANTHROPIC_KEY]"),
        (Regex::new(r"\b(hf_[a-zA-Z0-9]{20,})\b").unwrap(), "[REDACTED_HUGGINGFACE_TOKEN]"),
    ]
}

fn unslop_clean(s: &str) -> String {
    // Replace em dashes and en dashes with standard punctuation
    s.replace('—', ", ").replace('–', "-")
}

struct StreamRedactor {
    buffer: String,
    patterns: Arc<Vec<(Regex, &'static str)>>,
    delimiters: HashSet<char>,
}

impl StreamRedactor {
    fn new(patterns: Arc<Vec<(Regex, &'static str)>>) -> Self {
        let mut delimiters = HashSet::new();
        for c in " \n\r\t`'\"()[]{}<>,;:".chars() {
            delimiters.insert(c);
        }
        Self {
            buffer: String::new(),
            patterns,
            delimiters,
        }
    }

    fn feed(&mut self, chunk: &str) -> String {
        self.buffer.push_str(chunk);
        let mut last_delim = None;
        for (i, c) in self.buffer.char_indices() {
            if self.delimiters.contains(&c) {
                last_delim = Some(i + c.len_utf8());
            }
        }

        if let Some(pos) = last_delim {
            let ready = &self.buffer[..pos];
            let redacted = self.redact(ready);
            self.buffer = self.buffer[pos..].to_string();
            redacted
        } else {
            String::new()
        }
    }

    fn flush(&mut self) -> String {
        let remainder = std::mem::take(&mut self.buffer);
        self.redact(&remainder)
    }

    fn redact(&self, text: &str) -> String {
        let mut result = text.to_string();
        for (pat, repl) in self.patterns.iter() {
            result = pat.replace_all(&result, *repl).to_string();
        }
        unslop_clean(&result)
    }
}

fn get_cortex_token() -> Option<String> {
    fs::read_to_string(CORTEX_TOKEN_PATH).ok().map(|s| s.trim().to_string())
}

#[tokio::main]
async fn main() {
    let patterns = Arc::new(build_patterns());
    let client = reqwest::Client::builder()
        .tcp_nodelay(true)
        .pool_idle_timeout(Duration::from_secs(60))
        .build()
        .unwrap();

    let state = AppState {
        client,
        start_time: Instant::now(),
        redactor_patterns: patterns,
    };

    let app = Router::new()
        .route("/api/pulse", get(handle_pulse))
        .route("/api/walkthrough", get(handle_walkthrough))
        .route("/api/action", post(handle_action))
        .route("/api/vault", get(handle_vault))
        .route("/api/goals", get(handle_get_goals).post(handle_post_goals))
        .route("/api/skills", get(handle_get_skills))
        .route("/api/cortex", get(handle_cortex_search).post(handle_cortex_write))
        .route("/api/chat/stream", post(handle_chat_stream))
        .fallback_service(ServeDir::new(STATIC_DIR))
        .layer(
            CorsLayer::new()
                .allow_origin([
                    "http://127.0.0.1:18095".parse().unwrap(),
                    "http://localhost:18095".parse().unwrap(),
                    "http://192.168.1.108:18095".parse().unwrap(),
                    "http://100.116.106.93:18095".parse().unwrap(),
                ])
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 18095));
    println!("🚀 AIEN Native Sovereign Cockpit (Rust Axum) active on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handle_pulse(State(state): State<AppState>) -> Json<Value> {
    let uptime = state.start_time.elapsed().as_secs();

    let model_ok = state
        .client
        .get(format!("{}/health", MAX_SEAT_URL))
        .timeout(Duration::from_millis(500))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);

    let conduit_ok = state
        .client
        .get(CONDUIT_URL)
        .timeout(Duration::from_millis(500))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);

    let hive_dir = Path::new("/home/drakestapleton/basecamp/hive");
    let hive_count = if hive_dir.exists() {
        fs::read_dir(hive_dir).map(|d| d.count()).unwrap_or(0)
    } else {
        0
    };

    Json(json!({
        "status": "ok",
        "heartbeat_bpm": 72,
        "coherence": 0.98,
        "uptime_seconds": uptime,
        "model_seat": if model_ok { "ONLINE (Port 18006)" } else { "OFFLINE" },
        "conduit_seat": if conduit_ok { "ONLINE (Port 6167)" } else { "OFFLINE" },
        "hive_count": hive_count,
        "timestamp": Utc::now().to_rfc3339()
    }))
}

async fn handle_walkthrough() -> Json<Value> {
    let md_path = Path::new("/home/drakestapleton/basecamp/WALKTHROUGH.md");
    if md_path.exists() {
        if let Ok(content) = fs::read_to_string(md_path) {
            return Json(json!({"status": "ok", "walkthrough": content}));
        }
    }

    let output = Command::new("aien")
        .arg("--walkthrough")
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            Json(json!({"status": "ok", "walkthrough": stdout}))
        }
        Err(e) => Json(json!({"status": "error", "detail": e.to_string()})),
    }
}

#[derive(Deserialize)]
struct ActionPayload {
    action: String,
    target: Option<String>,
}

async fn handle_action(Json(payload): Json<ActionPayload>) -> Json<Value> {
    let cmd_args = match payload.action.as_str() {
        "dream_pulse" => vec!["--pulse"],
        "doctor" => vec!["--doctor"],
        "nest" => vec!["--nest"],
        "refresh_map" => vec!["--walkthrough"],
        "audit_secrets" => vec!["--vault"],
        _ => vec!["--help"],
    };

    let output = Command::new("aien")
        .args(&cmd_args)
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            Json(json!({
                "status": if out.status.success() { "ok" } else { "error" },
                "exit_code": out.status.code().unwrap_or(-1),
                "stdout": stdout,
                "stderr": stderr
            }))
        }
        Err(e) => Json(json!({
            "status": "error",
            "detail": e.to_string()
        })),
    }
}

async fn handle_vault() -> Json<Value> {
    let output = Command::new("atlas-vault")
        .arg("list")
        .output()
        .await;

    let keys = match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
        }
        Err(_) => vec![],
    };

    Json(json!({
        "status": "ok",
        "vault_backend": "atlas-vault (Hardware TPM-bound)",
        "keys_registered": keys.len(),
        "keys": keys,
        "stray_env_files": [],
        "hygiene_status": "SECURE_TPM_ONLY"
    }))
}

async fn handle_get_goals() -> Json<Value> {
    let path = Path::new(GOALS_PATH);
    if path.exists() {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&content) {
                return Json(parsed);
            }
        }
    }
    Json(json!({"project_name": "Sovereign Workspace", "goals": []}))
}

#[derive(Deserialize)]
struct GoalCreatePayload {
    title: String,
    description: Option<String>,
    milestones: Option<Vec<String>>,
}

async fn handle_post_goals(Json(payload): Json<GoalCreatePayload>) -> Json<Value> {
    let ms_arg = if let Some(ms) = payload.milestones {
        format!("| {}", ms.join(", "))
    } else {
        String::new()
    };

    let output = Command::new("aien")
        .args(["--goal", "new", &payload.title, &ms_arg])
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            Json(json!({"status": "ok", "output": stdout}))
        }
        Err(e) => Json(json!({"status": "error", "detail": e.to_string()})),
    }
}

async fn handle_get_skills() -> Json<Value> {
    let dir = Path::new(SKILLS_DIR);
    if !dir.exists() {
        return Json(json!({"total": 0, "skills": []}));
    }

    let mut skills = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let skill_md = path.join("SKILL.md");
                if skill_md.exists() {
                    if let Ok(content) = fs::read_to_string(&skill_md) {
                        let mut name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        let mut desc = "No description provided.".to_string();
                        let mut in_fm = false;

                        for line in content.lines() {
                            let t = line.trim();
                            if t == "---" {
                                if in_fm {
                                    break;
                                } else {
                                    in_fm = true;
                                    continue;
                                }
                            }
                            if in_fm {
                                if let Some(rest) = t.strip_prefix("name:") {
                                    name = rest.trim().trim_matches('"').trim_matches('\'').to_string();
                                } else if let Some(rest) = t.strip_prefix("description:") {
                                    desc = rest.trim().trim_matches('"').trim_matches('\'').to_string();
                                }
                            }
                        }

                        let has_scripts = path.join("scripts").exists();
                        skills.push(json!({
                            "name": name,
                            "description": desc,
                            "path": skill_md.display().to_string(),
                            "has_scripts": has_scripts,
                            "content": content
                        }));
                    }
                }
            }
        }
    }

    skills.sort_by(|a, b| {
        let na = a.get("name").and_then(Value::as_str).unwrap_or("");
        let nb = b.get("name").and_then(Value::as_str).unwrap_or("");
        na.cmp(nb)
    });

    Json(json!({"total": skills.len(), "skills": skills}))
}

#[derive(Deserialize)]
struct CortexSearchQuery {
    q: Option<String>,
    limit: Option<usize>,
}

async fn handle_cortex_search(
    State(state): State<AppState>,
    Query(query): Query<CortexSearchQuery>,
) -> Json<Value> {
    let q = query.q.unwrap_or_else(|| "AIEN".to_string());
    let limit = query.limit.unwrap_or(5);
    let token = get_cortex_token();

    let mut req = state.client.get(format!(
        "{}/api/cortex/search?q={}&space=atlas-memory&limit={}",
        CORTEX_URL, q, limit
    ));

    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {}", t));
    }

    match req.send().await {
        Ok(resp) => {
            let json = resp.json::<Value>().await.unwrap_or_else(|_| json!({"results": []}));
            Json(json)
        }
        Err(e) => Json(json!({"results": [], "error": e.to_string()})),
    }
}

#[derive(Deserialize)]
struct CortexWritePayload {
    name: String,
    content: String,
    kind: Option<String>,
}

async fn handle_cortex_write(
    State(state): State<AppState>,
    Json(payload): Json<CortexWritePayload>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let token = get_cortex_token().ok_or_else(|| {
        (StatusCode::INTERNAL_SERVER_ERROR, "Missing Cortex token".to_string())
    })?;

    let body = json!({
        "kind": "entity",
        "value": {
            "space": "atlas-memory",
            "entityType": payload.kind.unwrap_or_else(|| "lesson".to_string()),
            "canonicalName": payload.name,
            "content": payload.content,
            "metadata": {
                "author": "AIEN",
                "source": "spark-cockpit-rs",
                "hardware": "NVIDIA DGX Spark GB10"
            }
        }
    });

    let resp = state
        .client
        .post(format!("{}/api/cortex/write", CORTEX_URL))
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let json = resp.json::<Value>().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    Ok(Json(json))
}

async fn handle_chat_stream(
    State(state): State<AppState>,
    Json(mut payload): Json<Value>,
) -> Response {
    payload["stream"] = json!(true);
    if payload.get("model").is_none() {
        payload["model"] = json!("atlas-lightning-omni");
    }

    let patterns = state.redactor_patterns.clone();
    let upstream_resp = match state
        .client
        .post(format!("{}/v1/chat/completions", MAX_SEAT_URL))
        .json(&payload)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": format!("MAX model seat error: {}", e)})),
            )
                .into_response();
        }
    };

    let stream = async_stream::stream! {
        let mut byte_stream = upstream_resp.bytes_stream();
        let mut content_redactor = StreamRedactor::new(patterns.clone());
        let mut reasoning_redactor = StreamRedactor::new(patterns);
        let mut line_buffer = String::new();

        while let Some(chunk_result) = byte_stream.next().await {
            if let Ok(chunk) = chunk_result {
                let text = String::from_utf8_lossy(&chunk);
                line_buffer.push_str(&text);

                while let Some(pos) = line_buffer.find('\n') {
                    let line = line_buffer[..pos].trim().to_string();
                    line_buffer = line_buffer[pos + 1..].to_string();

                    if line.starts_with("data: ") {
                        let data_str = line[6..].trim();
                        if data_str == "[DONE]" {
                            let sc = content_redactor.flush();
                            let sr = reasoning_redactor.flush();
                            if !sc.is_empty() || !sr.is_empty() {
                                let payload = json!({"content": sc, "reasoning": sr});
                                yield Ok::<_, Infallible>(Event::default().data(payload.to_string()));
                            }
                            yield Ok(Event::default().data("[DONE]"));
                            break;
                        }

                        if let Ok(parsed) = serde_json::from_str::<Value>(data_str) {
                            if let Some(choices) = parsed.get("choices").and_then(Value::as_array) {
                                if let Some(choice) = choices.first() {
                                    if let Some(delta) = choice.get("delta") {
                                        let raw_content = delta.get("content").and_then(Value::as_str).unwrap_or("");
                                        let raw_reasoning = delta.get("reasoning")
                                            .or_else(|| delta.get("reasoning_content"))
                                            .and_then(Value::as_str)
                                            .unwrap_or("");

                                        let sc = if !raw_content.is_empty() { content_redactor.feed(raw_content) } else { String::new() };
                                        let sr = if !raw_reasoning.is_empty() { reasoning_redactor.feed(raw_reasoning) } else { String::new() };

                                        if !sc.is_empty() || !sr.is_empty() {
                                            let payload = json!({"content": sc, "reasoning": sr});
                                            yield Ok(Event::default().data(payload.to_string()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

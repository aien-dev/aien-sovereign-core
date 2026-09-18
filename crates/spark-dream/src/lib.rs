use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_CORTEX_URL: &str = "http://127.0.0.1:18080";
pub const DEFAULT_MODEL_URL: &str = "http://127.0.0.1:18006";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamState {
    pub daemon_running: bool,
    pub current_phase: String,
    pub last_cycle_timestamp: Option<String>,
    pub cycles_completed: u64,
    pub facts_committed: u64,
    pub proposals_staged: u64,
    pub facts_rejected: u64,
    pub hive_pulse: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateItem {
    pub source: String,
    pub text: String,
    pub item_type: String, // operational_fact or soul_proposal
    pub canonical_name: String,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct DreamEngine {
    pub base_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub park_dir: PathBuf,
    pub inbox_dir: PathBuf,
    pub dream_dir: PathBuf,
    pub cortex_url: String,
    pub cortex_token_path: PathBuf,
    pub client: Client,
}

impl DreamEngine {
    pub fn new(base_dir: PathBuf) -> Self {
        let sessions_dir = base_dir.join("sessions");
        let inbox_dir = base_dir.join("inbox");
        let dream_dir = base_dir.join("aien-dream");
        let park_dir = PathBuf::from("/home/drakestapleton/atlas-prime-workspace/park");
        let cortex_token_path = PathBuf::from("/home/drakestapleton/.config/cortex/token");

        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        Self {
            base_dir,
            sessions_dir,
            park_dir,
            inbox_dir,
            dream_dir,
            cortex_url: DEFAULT_CORTEX_URL.to_string(),
            cortex_token_path,
            client,
        }
    }

    pub fn get_cortex_token(&self) -> String {
        fs::read_to_string(&self.cortex_token_path)
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    /// Read GPU telemetry directly from nvidia-smi / /proc without python overhead
    pub async fn get_gpu_utilization(&self) -> u32 {
        let output = tokio::process::Command::new("nvidia-smi")
            .args(["--query-gpu=utilization.gpu", "--format=csv,noheader,nounits"])
            .output()
            .await;

        if let Ok(out) = output {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(val) = s.lines().next().and_then(|l| l.trim().parse::<u32>().ok()) {
                return val;
            }
        }
        0
    }

    /// Check last activity timestamp across active session files
    pub fn get_last_session_activity(&self) -> Option<DateTime<Utc>> {
        let mut latest: Option<DateTime<Utc>> = None;
        if let Ok(entries) = fs::read_dir(&self.sessions_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(mtime) = meta.modified() {
                        let dt: DateTime<Utc> = mtime.into();
                        if latest.map(|l| dt > l).unwrap_or(true) {
                            latest = Some(dt);
                        }
                    }
                }
            }
        }
        latest
    }

    /// Commit verified fact to Cortex Memory (port 18080)
    pub async fn commit_to_cortex(
        &self,
        entity_type: &str,
        canonical_name: &str,
        content: &str,
        metadata: &Value,
    ) -> Result<Value, String> {
        let token = self.get_cortex_token();
        let payload = json!({
            "kind": "entity",
            "value": {
                "canonicalName": canonical_name,
                "entityType": entity_type,
                "content": content,
                "metadata": metadata,
                "space": "atlas-memory"
            }
        });

        let resp = self.client
            .post(format!("{}/api/cortex/write", self.cortex_url))
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Cortex request failed: {}", e))?;

        if resp.status().is_success() {
            let res_json = resp.json::<Value>().await.unwrap_or_else(|_| json!({"status": "ok"}));
            Ok(res_json)
        } else {
            Err(format!("Cortex returned status {}", resp.status()))
        }
    }

    /// Consolidate uncommitted knowledge from parked notes and sessions
    pub async fn execute_dream_cycle(&self) -> Result<DreamState, String> {
        let mut committed = 0;
        let mut staged = 0;
        let mut rejected = 0;

        fs::create_dir_all(&self.inbox_dir).map_err(|e| e.to_string())?;
        fs::create_dir_all(&self.dream_dir).map_err(|e| e.to_string())?;

        // 1. Scan Park notes
        if self.park_dir.exists() {
            if let Ok(entries) = fs::read_dir(&self.park_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().map(|e| e == "md").unwrap_or(false) {
                        let file_stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                        if let Ok(content) = fs::read_to_string(&path) {
                            if content.trim().is_empty() {
                                continue;
                            }

                            // Extract candidate fact
                            let canonical_name = format!("parked-{}", file_stem);
                            let meta = json!({
                                "source": path.to_string_lossy().to_string(),
                                "consolidated_at": Utc::now().to_rfc3339()
                            });

                            // Commit as learned procedure or discovery
                            match self.commit_to_cortex("discovery", &canonical_name, &content, &meta).await {
                                Ok(_) => {
                                    committed += 1;
                                    // Archive parked note
                                    let archive_name = format!("{}.consolidated", path.file_name().unwrap().to_string_lossy());
                                    let _ = fs::rename(&path, path.with_file_name(archive_name));
                                }
                                Err(_) => {
                                    rejected += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. Compute state summary
        let state = DreamState {
            daemon_running: true,
            current_phase: "idle".to_string(),
            last_cycle_timestamp: Some(Utc::now().to_rfc3339()),
            cycles_completed: 1,
            facts_committed: committed,
            proposals_staged: staged,
            facts_rejected: rejected,
            hive_pulse: Some(json!({
                "heartbeat": "synchronous",
                "timestamp": Utc::now().to_rfc3339()
            })),
        };

        // Write state file
        let state_path = self.dream_dir.join("state.json");
        let _ = fs::write(&state_path, serde_json::to_string_pretty(&state).unwrap());

        Ok(state)
    }
}

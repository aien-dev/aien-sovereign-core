use reqwest::Client;
use rusqlite::params;
use serde_json::json;
use tracing::{error, info};
use uuid::Uuid;

use crate::audit::SystemAuditReport;

const CORTEX_URL: &str = "http://127.0.0.1:18080";
fn debugger_cortex_token_path() -> std::path::PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    home.join(".config/cortex/token")
}

fn debugger_hive_db_path() -> std::path::PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    home.join(".config/cortex/hive.db")
}

pub async fn report_incident(client: &Client, report: &SystemAuditReport, failure_summary: &str) {
    info!("[DebuggerReporter] Reporting incident: {}", failure_summary);

    // 1. Commit bug report to Cortex memory
    if let Ok(token) = std::fs::read_to_string(debugger_cortex_token_path()) {
        let payload = json!({
            "space": "atlas-memory",
            "canonicalName": format!("incident-{}", Uuid::new_v4().to_string()[..8].to_string()),
            "entityType": "bug_report",
            "content": format!("Incident detected by spark-debugger: {}
        Report Details: {:?}", failure_summary, report)
        });

        let write_url = format!("{}/api/cortex/write", CORTEX_URL);
        if let Err(e) = client
            .post(&write_url)
            .header("Authorization", format!("Bearer {}", token.trim()))
            .json(&payload)
            .send()
            .await
        {
            error!(
                "[DebuggerReporter] Failed to commit bug report to Cortex: {}",
                e
            );
        } else {
            info!("[DebuggerReporter] Committed bug report to Cortex.");
        }
    }

    // 2. Emit alert comb onto Honeycomb lattice if hive.db exists
    if debugger_hive_db_path().exists() {
        if let Ok(conn) = rusqlite::Connection::open(debugger_hive_db_path()) {
            let comb_id = format!("comb-alert-{}", &Uuid::new_v4().to_string()[..8]);
            let now = chrono::Utc::now().to_rfc3339();
            let content = format!("🚨 AUDIT ALERT: {}", failure_summary);

            // Find free coordinate or default to outer ring
            let res = conn.execute(
                "INSERT INTO hive_combs (id, q, r, author, role, content, intent, created_at)
                 VALUES (?1, -2, 2, ?2, ?3, ?4, ?5, ?6)",
                params![
                    comb_id,
                    "spark-debugger",
                    "watchdog",
                    content,
                    "branch",
                    now
                ],
            );
            if let Ok(_) = res {
                info!("[DebuggerReporter] Emitted alert comb to Honeycomb Wall.");
            }
        }
    }
}

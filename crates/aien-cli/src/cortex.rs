use reqwest::Client;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use colored::*;

const CORTEX_BASE_URL: &str = "http://127.0.0.1:18080";
const CORTEX_TOKEN_PATH: &str = "/home/drakestapleton/.config/cortex/token";

pub fn get_cortex_token() -> String {
    let p = Path::new(CORTEX_TOKEN_PATH);
    if p.exists() {
        fs::read_to_string(p).unwrap_or_default().trim().to_string()
    } else {
        String::new()
    }
}

pub async fn write_to_cortex(canonical_name: &str, content: &str, entity_type: &str, metadata: Value) -> Result<Value, String> {
    let token = get_cortex_token();
    if token.is_empty() {
        return Err("Missing Cortex token in ~/.config/cortex/token".to_string());
    }

    let et = if entity_type.is_empty() { "discovery" } else { entity_type };
    let payload = json!({
        "kind": "entity",
        "value": {
            "space": "atlas-memory",
            "entityType": et,
            "canonicalName": canonical_name,
            "content": content,
            "metadata": metadata,
        }
    });

    let client = Client::new();
    let resp = client.post(format!("{}/api/cortex/write", CORTEX_BASE_URL))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Failed to connect to Cortex (18080): {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Cortex write failed [{}]: {}", status, body));
    }

    let res_json = resp.json::<Value>().await.map_err(|e| format!("Invalid JSON response: {}", e))?;
    Ok(res_json)
}

pub async fn search_cortex(query: &str, limit: usize) -> Result<Vec<Value>, String> {
    let token = get_cortex_token();
    let url = format!("{}/api/cortex/search?q={}&space=atlas-memory&limit={}", CORTEX_BASE_URL, query.replace(" ", "%20"), limit);
    let client = Client::new();
    let mut req = client.get(&url).header("Accept", "application/json");
    if !token.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", token));
    }

    let resp = req.send().await.map_err(|e| format!("Cortex connection failed: {}", e))?;
    if let Ok(data) = resp.json::<Value>().await {
        if let Some(arr) = data.get("results").and_then(Value::as_array) {
            return Ok(arr.clone());
        }
    }
    Ok(Vec::new())
}

pub async fn handle_cortex_command(parts: &[&str]) {
    if parts.is_empty() {
        println!("Usage: /cortex [search <query> | write <name> | <content>]");
        return;
    }

    match parts[0] {
        "search" | "find" => {
            let q = parts[1..].join(" ");
            println!("{}", format!("Searching Cortex (atlas-memory) for '{}'...", q).yellow());
            match search_cortex(&q, 5).await {
                Ok(items) if !items.is_empty() => {
                    println!("{}", format!("\nCortex Knowledge Recall ({} found):", items.len()).cyan().bold());
                    for (i, it) in items.iter().enumerate() {
                        let name = it.get("canonicalName").and_then(Value::as_str).unwrap_or("Untitled");
                        let score = it.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let content = it.get("content").and_then(Value::as_str).unwrap_or("");
                        let snippet = if content.len() > 180 { &content[..180] } else { content };
                        println!("{}. {} ({:.2})", i + 1, name.bold().yellow(), score);
                        println!("   {}\n", snippet.dimmed());
                    }
                },
                Ok(_) => println!("{}", "No matching entities found in atlas-memory.".dimmed()),
                Err(e) => println!("{}", format!("Error querying Cortex: {}", e).red()),
            }
        },
        "write" | "remember" => {
            let full_text = parts[1..].join(" ");
            let split: Vec<&str> = full_text.split('|').collect();
            if split.len() < 2 {
                println!("Usage: /cortex write <canonical_name> | <content>");
                return;
            }
            let name = split[0].trim();
            let content = split[1..].join("|").trim().to_string();

            println!("{}", format!("Committing memory entity '{}' to Spark Cortex...", name).yellow());
            match write_to_cortex(name, &content, "lesson", json!({"source": "aien-cli", "author": "operator"})).await {
                Ok(res) => {
                    let r_id = res.get("receipt").and_then(|r| r.get("id")).and_then(Value::as_str).unwrap_or("confirmed");
                    println!("{}", format!("✓ Committed to Spark Cortex Memory! Receipt: {}", r_id).green().bold());
                },
                Err(e) => println!("{}", format!("Failed to commit to Cortex: {}", e).red().bold()),
            }
        },
        _ => {
            let q = parts.join(" ");
            println!("{}", format!("Searching Cortex (atlas-memory) for '{}'...", q).yellow());
            match search_cortex(&q, 5).await {
                Ok(items) if !items.is_empty() => {
                    println!("{}", format!("\nCortex Knowledge Recall ({} found):", items.len()).cyan().bold());
                    for (i, it) in items.iter().enumerate() {
                        let name = it.get("canonicalName").and_then(Value::as_str).unwrap_or("Untitled");
                        let score = it.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let content = it.get("content").and_then(Value::as_str).unwrap_or("");
                        let snippet = if content.len() > 180 { &content[..180] } else { content };
                        println!("{}. {} ({:.2})", i + 1, name.bold().yellow(), score);
                        println!("   {}\n", snippet.dimmed());
                    }
                },
                Ok(_) => println!("{}", "No matching entities found in atlas-memory.".dimmed()),
                Err(e) => println!("{}", format!("Error querying Cortex: {}", e).red()),
            }
        }
    }
}

pub async fn cortex_dispatch_tool(args: &Value) -> Value {
    let action = args.get("action").and_then(Value::as_str).unwrap_or("search");

    match action {
        "write" | "remember" => {
            let name = args.get("name").and_then(Value::as_str).unwrap_or("unnamed_memory");
            let content = args.get("content").and_then(Value::as_str).unwrap_or("");
            let kind = args.get("kind").and_then(Value::as_str).unwrap_or("discovery");
            let meta = args.get("metadata").cloned().unwrap_or(json!({"source": "model_dispatch"}));

            match write_to_cortex(name, content, kind, meta).await {
                Ok(receipt) => json!({
                    "status": "ok",
                    "receipt": receipt,
                    "message": format!("Committed '{}' to Spark Cortex (atlas-memory).", name)
                }),
                Err(e) => json!({"status": "error", "error": e})
            }
        },
        "search" => {
            let q = args.get("query").and_then(Value::as_str).unwrap_or("");
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(5) as usize;
            match search_cortex(q, limit).await {
                Ok(results) => json!({"status": "ok", "results": results}),
                Err(e) => json!({"status": "error", "error": e})
            }
        },
        _ => json!({"error": format!("Unknown cortex action '{}'", action)})
    }
}

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

use crate::db::Database;
use crate::embeddings::fetch_embedding;
use crate::models::*;

pub struct AppState {
    pub db: Arc<Database>,
    pub http_client: Client,
    pub encoder_url: String,
}

pub async fn write_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<WritePayload>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    match payload {
        WritePayload::Entity { value } => {
            let mut emb = None;
            let text_to_embed = if !value.content.trim().is_empty() {
                format!("{}: {}", value.canonical_name, value.content)
            } else {
                value.canonical_name.clone()
            };

            match fetch_embedding(&state.http_client, &text_to_embed, &state.encoder_url).await {
                Ok(vec) => emb = Some(vec),
                Err(e) => {
                    tracing::warn!("Embedding encoder unavailable, proceeding with lexical only: {}", e);
                }
            }

            match state.db.upsert_entity(&value, emb.as_deref()) {
                Ok(receipt) => Ok((StatusCode::CREATED, Json(CortexReceipt { recorded: true, receipt })).into_response()),
                Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))
            }
        }
        WritePayload::Claim { value } => {
            match state.db.upsert_claim(&value) {
                Ok(receipt) => Ok((StatusCode::CREATED, Json(CortexReceipt { recorded: true, receipt })).into_response()),
                Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))
            }
        }
        WritePayload::Retract { value } => {
            match state.db.retract_target(&value.target_type, &value.target_id) {
                Ok(receipt) => Ok((StatusCode::CREATED, Json(CortexReceipt { recorded: true, receipt })).into_response()),
                Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))
            }
        }
    }
}

pub async fn search_get_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResponse>, (StatusCode, Json<serde_json::Value>)> {
    let start = Instant::now();
    let q = params.q.or(params.query).unwrap_or_default();
    let limit = params.limit.unwrap_or(12).min(50);
    let space = params.space.as_deref();

    match state.db.search_entities(&q, space, limit) {
        Ok(results) => {
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            Ok(Json(SearchResponse {
                results,
                degraded: Vec::new(),
                elapsed_ms,
            }))
        }
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))
    }
}

pub async fn search_post_handler(
    State(state): State<Arc<AppState>>,
    Json(params): Json<SearchParams>,
) -> Result<Json<SearchResponse>, (StatusCode, Json<serde_json::Value>)> {
    let start = Instant::now();
    let q = params.q.or(params.query).unwrap_or_default();
    let limit = params.limit.unwrap_or(12).min(50);
    let space = params.space.as_deref();

    match state.db.search_entities(&q, space, limit) {
        Ok(results) => {
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            Ok(Json(SearchResponse {
                results,
                degraded: Vec::new(),
                elapsed_ms,
            }))
        }
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))
    }
}

pub async fn recall_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RecallPayload>,
) -> Result<Json<RecallResponse>, (StatusCode, Json<serde_json::Value>)> {
    let start = Instant::now();
    let space = payload.space.as_deref();
    let limit = payload.limit.min(20);

    let mut emb = None;
    match fetch_embedding(&state.http_client, &payload.query, &state.encoder_url).await {
        Ok(vec) => emb = Some(vec),
        Err(e) => {
            tracing::warn!("Embedding encoder unavailable for recall: {}", e);
        }
    }

    match state.db.recall_entities(&payload.query, emb.as_deref(), space, limit) {
        Ok(results) => {
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            Ok(Json(RecallResponse {
                results,
                degraded: Vec::new(),
                elapsed_ms,
            }))
        }
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))
    }
}

#[derive(serde::Deserialize)]
pub struct GetParams {
    pub id: Option<String>,
    pub name: Option<String>,
    pub space: Option<String>,
}

pub async fn get_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GetParams>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let target = params.id.or(params.name).unwrap_or_default();
    if target.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Missing id or name parameter"}))));
    }

    match state.db.get_entity(&target, params.space.as_deref()) {
        Ok(Some(entity)) => Ok(Json(entity).into_response()),
        Ok(None) => Err((StatusCode::NOT_FOUND, Json(json!({"error": "Entity not found"})))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))
    }
}

pub async fn traverse_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<TraversePayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.db.traverse_claims(&payload.subject_id, payload.space.as_deref()) {
        Ok(claims) => Ok(Json(json!({"claims": claims}))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))
    }
}

pub async fn health_handler() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "cortex-rs",
        "version": "0.1.0",
        "space": "atlas-memory",
        "runtime": "native-arm64-rust"
    }))
}

pub async fn brain_health_handler(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let count = state.db.count_entities(Some("atlas-memory")).unwrap_or(0);
    Json(json!({
        "ok": true,
        "nodes": count,
        "service": "atlas-brain-spark"
    }))
}

pub async fn brain_query_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<BrainQueryPayload>,
) -> Json<BrainResponse> {
    let q = payload.q.trim();
    if q.is_empty() {
        return Json(BrainResponse {
            result: BrainResult {
                brief: "Cortex: no matching nodes. Verified live on the Spark brain bridge.".to_string(),
            },
        });
    }

    let mut emb = None;
    if let Ok(vec) = fetch_embedding(&state.http_client, q, &state.encoder_url).await {
        emb = Some(vec);
    }

    let results = state.db.recall_entities(q, emb.as_deref(), Some("atlas-memory"), 8).unwrap_or_default();

    if results.is_empty() {
        let lexical = state.db.search_entities(q, Some("atlas-memory"), 8).unwrap_or_default();
        if lexical.is_empty() {
            return Json(BrainResponse {
                result: BrainResult {
                    brief: "Cortex: no matching nodes. Verified live on the Spark brain bridge.".to_string(),
                },
            });
        }
        let lines: Vec<String> = lexical
            .iter()
            .map(|r| {
                let slice = if r.entity.content.len() > 240 {
                    &r.entity.content[..240]
                } else {
                    &r.entity.content
                };
                format!("- [{}] {}", r.entity.id, slice)
            })
            .collect();
        let brief = format!(
            "Cortex brief ({} hit{}):\n{}",
            lines.len(),
            if lines.len() == 1 { "" } else { "s" },
            lines.join("\n")
        );
        return Json(BrainResponse { result: BrainResult { brief } });
    }

    let lines: Vec<String> = results
        .iter()
        .map(|r| {
            let slice = if r.entity.content.len() > 240 {
                &r.entity.content[..240]
            } else {
                &r.entity.content
            };
            format!("- [{}] {}", r.entity.id, slice)
        })
        .collect();
    let brief = format!(
        "Cortex brief ({} hit{}):\n{}",
        lines.len(),
        if lines.len() == 1 { "" } else { "s" },
        lines.join("\n")
    );
    Json(BrainResponse { result: BrainResult { brief } })
}

pub async fn brain_remember_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<BrainRememberPayload>,
) -> Json<BrainResponse> {
    let q = payload.q.trim();
    if q.is_empty() {
        return Json(BrainResponse {
            result: BrainResult {
                brief: "Cortex: empty remember payload ignored.".to_string(),
            },
        });
    }

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(q.as_bytes());
    let hash_hex = format!("{:x}", hasher.finalize());
    let key = &hash_hex[..16];
    let canonical_name = format!("brain:{}", key);

    let input = EntityWriteInput {
        id: Some(format!("n-{}", &uuid::Uuid::new_v4().to_string()[..8])),
        space: "atlas-memory".to_string(),
        entity_type: "lesson".to_string(),
        canonical_name,
        content: q.to_string(),
        aliases: Vec::new(),
        metadata: json!({"source": "opencode-brain"}),
        confidence: 1.0,
        valid_from: None,
        valid_to: None,
        external_id: None,
    };

    let mut emb = None;
    if let Ok(vec) = fetch_embedding(&state.http_client, q, &state.encoder_url).await {
        emb = Some(vec);
    }

    let _ = state.db.upsert_entity(&input, emb.as_deref());

    let brief = format!("Cortex remembered: {}", if q.len() > 240 { &q[..240] } else { q });
    Json(BrainResponse { result: BrainResult { brief } })
}

pub async fn brain_explain_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<BrainExplainPayload>,
) -> Json<BrainResponse> {
    let target = payload.node.trim();
    match state.db.explain_node(target) {
        Ok((Some(entity), neighbors)) => {
            let ent_slice = if entity.content.len() > 400 { &entity.content[..400] } else { &entity.content };
            let neighbor_str = if neighbors.is_empty() {
                "none".to_string()
            } else {
                neighbors
                    .iter()
                    .map(|n| {
                        let slice = if n.content.len() > 120 { &n.content[..120] } else { &n.content };
                        format!("[{}] {}", n.id, slice)
                    })
                    .collect::<Vec<_>>()
                    .join(" | ")
            };
            let brief = format!("Cortex node [{}]: {}\nNeighbors: {}", entity.id, ent_slice, neighbor_str);
            Json(BrainResponse { result: BrainResult { brief } })
        }
        Ok((None, _)) => {
            let count = state.db.count_entities(Some("atlas-memory")).unwrap_or(0);
            let slice = if target.len() > 80 { &target[..80] } else { target };
            let brief = format!("Cortex: node {} not found ({} nodes).", slice, count);
            Json(BrainResponse { result: BrainResult { brief } })
        }
        Err(e) => {
            Json(BrainResponse {
                result: BrainResult { brief: format!("Cortex explain error: {}", e) },
            })
        }
    }
}

pub async fn brain_path_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<BrainPathPayload>,
) -> Json<BrainResponse> {
    let from = payload.from.trim();
    let to = payload.to.trim();
    match state.db.find_path_nodes(from, to) {
        Ok((Some(a), Some(b))) => {
            let a_slice = if a.content.len() > 160 { &a.content[..160] } else { &a.content };
            let b_slice = if b.content.len() > 160 { &b.content[..160] } else { &b.content };
            let brief = format!("Cortex path: [{}] {}\n-> [{}] {}", a.id, a_slice, b.id, b_slice);
            Json(BrainResponse { result: BrainResult { brief } })
        }
        _ => {
            let count = state.db.count_entities(Some("atlas-memory")).unwrap_or(0);
            let brief = format!("Cortex: path not found (from={} to={}, {} nodes).", from, to, count);
            Json(BrainResponse { result: BrainResult { brief } })
        }
    }
}

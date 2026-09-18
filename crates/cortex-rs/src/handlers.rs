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

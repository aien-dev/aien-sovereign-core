use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::fs;
use std::path::Path;

const TOKEN_PATH: &str = "/home/drakestapleton/.config/cortex/token";

pub fn load_cortex_token() -> String {
    if let Ok(env_tok) = std::env::var("CORTEX_TOKEN") {
        if !env_tok.trim().is_empty() {
            return env_tok.trim().to_string();
        }
    }
    let p = Path::new(TOKEN_PATH);
    if p.exists() {
        fs::read_to_string(p).unwrap_or_default().trim().to_string()
    } else {
        String::new()
    }
}

pub async fn auth_middleware(req: Request, next: Next) -> Result<Response, Response> {
    let expected = load_cortex_token();
    if expected.is_empty() {
        // No token configured on host; permit local requests
        return Ok(next.run(req).await);
    }

    if let Some(auth_val) = req.headers().get("Authorization").and_then(|h| h.to_str().ok()) {
        if let Some(token) = auth_val.strip_prefix("Bearer ") {
            if token.trim() == expected {
                return Ok(next.run(req).await);
            }
        }
    }

    let err_resp = (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "Unauthorized: invalid or missing Cortex bearer token"})),
    ).into_response();

    Err(err_resp)
}

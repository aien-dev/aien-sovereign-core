use aien_localhttp::Client;
use aien_provenance::{GrantPermissions, SourceGrant};
use serde_json::{json, Value};
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    /// Transport, framing, size or decode failure. The client speaks plain
    /// http to local endpoints only; an https endpoint is refused here.
    #[error("Network request failed: {0}")]
    Http(#[from] aien_localhttp::Error),
    #[error("Vault key resolution failed for {0}: {1}")]
    VaultKey(String, String),
    #[error("API returned error: {0}")]
    Api(String),
    #[error("Rights verification failed: {0}")]
    RightsDenied(String),
}

pub struct HarvesterClient {
    http: Client,
}

impl Default for HarvesterClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HarvesterClient {
    pub fn new() -> Self {
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build(),
        }
    }

    pub fn resolve_vault_key(key_name: &str) -> Result<String, ClientError> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        let vault_candidate = format!("{}/.local/bin/atlas-vault", home);
        let bin = if std::path::Path::new(&vault_candidate).is_file() {
            &vault_candidate
        } else {
            "atlas-vault"
        };
        let output = Command::new(bin).arg("get").arg(key_name).output();

        match output {
            Ok(out) if out.status.success() => {
                let secret = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if secret.is_empty() {
                    Err(ClientError::VaultKey(
                        key_name.to_string(),
                        "empty secret".to_string(),
                    ))
                } else {
                    Ok(secret)
                }
            }
            Ok(out) => Err(ClientError::VaultKey(
                key_name.to_string(),
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            )),
            Err(e) => Err(ClientError::VaultKey(key_name.to_string(), e.to_string())),
        }
    }

    pub(crate) async fn query_openai_compat(
        &self,
        endpoint_url: &str,
        api_key: &str,
        model: &str,
        prompt: &str,
    ) -> Result<String, ClientError> {
        let payload = json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.6
        });

        let res = self
            .http
            .post(endpoint_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(ClientError::Api(format!("{}: {}", status, text)));
        }

        let body: Value = res.json().await?;
        let content = body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(content)
    }

    pub async fn query_with_grant(
        &self,
        endpoint_url: &str,
        api_key: &str,
        model: &str,
        prompt: &str,
        grant: &SourceGrant,
        authority_verifying_key: &p256::ecdsa::VerifyingKey,
    ) -> Result<String, ClientError> {
        let now = chrono::Utc::now().timestamp() as u64;
        grant
            .verify_validity(now)
            .map_err(|e| ClientError::RightsDenied(e.to_string()))?;

        grant
            .verify_signature(authority_verifying_key)
            .map_err(|e| ClientError::RightsDenied(e.to_string()))?;

        grant
            .verify_permission(GrantPermissions::TRAIN | GrantPermissions::DISTILL)
            .or_else(|_| grant.verify_permission(GrantPermissions::EVALUATE))
            .map_err(|e| ClientError::RightsDenied(e.to_string()))?;

        self.query_openai_compat(endpoint_url, api_key, model, prompt)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aien_localhttp::test_server::{response, serve_once};

    #[tokio::test]
    async fn query_openai_compat_sends_the_same_request_and_reads_content() {
        let (base, server) = serve_once(response(
            "200 OK",
            "application/json",
            r#"{"choices":[{"message":{"content":"answer"}}]}"#,
        ))
        .await;
        let got = HarvesterClient::new()
            .query_openai_compat(&format!("{base}/v1/chat/completions"), "key", "m", "q")
            .await
            .unwrap();
        assert_eq!(got, "answer");
        let request = server.await.unwrap();
        assert!(
            request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(request.contains("\r\nAuthorization: Bearer key\r\n"));
        assert_eq!(request.matches("Content-Type: application/json").count(), 1);
        let body: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(
            body,
            json!({
                "model": "m",
                "messages": [{"role": "user", "content": "q"}],
                "temperature": 0.6
            })
        );
    }

    #[tokio::test]
    async fn query_openai_compat_missing_content_is_empty_string() {
        let (base, _server) = serve_once(response("200 OK", "application/json", "{}")).await;
        let got = HarvesterClient::new()
            .query_openai_compat(&base, "k", "m", "q")
            .await
            .unwrap();
        assert_eq!(got, "");
    }

    #[tokio::test]
    async fn query_openai_compat_reports_status_bad_json_and_https() {
        let (base, _server) =
            serve_once(response("429 Too Many Requests", "text/plain", "slow down")).await;
        let err = HarvesterClient::new()
            .query_openai_compat(&base, "k", "m", "q")
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "API returned error: 429 Too Many Requests: slow down"
        );

        let (base, _server) = serve_once(response("200 OK", "application/json", "nope")).await;
        let err = HarvesterClient::new()
            .query_openai_compat(&base, "k", "m", "q")
            .await
            .unwrap_err();
        assert!(matches!(err, ClientError::Http(_)), "{err:?}");

        let err = HarvesterClient::new()
            .query_openai_compat(
                "https://api.example.test/v1/chat/completions",
                "k",
                "m",
                "q",
            )
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                ClientError::Http(aien_localhttp::Error::HttpsRefused { .. })
            ),
            "{err:?}"
        );
    }
}

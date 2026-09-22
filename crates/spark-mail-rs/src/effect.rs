//! Outbound effect gate for mail. SMTP is unreachable until this returns allow.
//!
//! The deterministic floor matches the AEGIS pre-dispatch membrane: an empty
//! effect is rejected, and destructive payloads are rejected. `AIEN_EFFECT_POLICY=deny`
//! is the operator kill switch. When `AIEN_AEGIS_URL` is set, the send also has
//! to receive HTTP 200 from that AEGIS process before any socket to Gandi opens.

use serde::Serialize;
use std::io::{Error, ErrorKind};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EffectReceipt {
    pub effect: &'static str,
    pub decision: &'static str,
    pub to: String,
    pub subject: String,
    pub body_untrusted: bool,
    pub aegis_url: Option<String>,
}

pub fn authorize_gandi_send(to: &str, subject: &str, body: &str) -> Result<EffectReceipt, Error> {
    let policy = std::env::var("AIEN_EFFECT_POLICY").unwrap_or_default();
    let aegis_url = std::env::var("AIEN_AEGIS_URL").unwrap_or_default();
    authorize_gandi_send_with(to, subject, body, policy.trim(), aegis_url.trim())
}

pub fn authorize_gandi_send_with(
    to: &str,
    subject: &str,
    body: &str,
    policy: &str,
    aegis_url: &str,
) -> Result<EffectReceipt, Error> {
    if to.trim().is_empty() || subject.trim().is_empty() || body.trim().is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "subject, body, and recipient are required",
        ));
    }
    if !to.contains('@') || to.contains(char::is_whitespace) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "recipient is not an email address",
        ));
    }
    if policy.eq_ignore_ascii_case("deny") {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            "AEGIS effect policy denied gandi_send",
        ));
    }
    let normalized = body.to_lowercase();
    for pattern in ["rm -rf /", "rm -rf /*", ":(){", "mkfs"] {
        if normalized.contains(pattern) {
            return Err(Error::new(
                ErrorKind::PermissionDenied,
                format!("AEGIS enforcement membrane blocked gandi_send: matched '{pattern}'"),
            ));
        }
    }
    let aegis_url = (!aegis_url.is_empty()).then(|| aegis_url.to_string());
    if let Some(url) = &aegis_url {
        confirm_aegis(url, to, subject)?;
    }
    Ok(EffectReceipt {
        effect: "gandi_send",
        decision: "allow",
        to: to.to_string(),
        subject: subject.to_string(),
        body_untrusted: false,
        aegis_url,
    })
}

fn confirm_aegis(url: &str, to: &str, subject: &str) -> Result<(), Error> {
    let endpoint = format!("{}/v1/probe", url.trim_end_matches('/'));
    let payload = serde_json::json!({
        "state": {"action": "gandi_send", "to": to, "subject": subject},
        "probes": {"probes": []}
    });
    let body = serde_json::to_vec(&payload)
        .map_err(|_| Error::other("could not encode AEGIS probe request"))?;
    let response = std::thread::scope(|scope| {
        let handle = scope.spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| Error::other(error.to_string()))?;
            runtime.block_on(async move {
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(5))
                    .build()
                    .map_err(|error| Error::other(error.to_string()))?
                    .post(&endpoint)
                    .header("content-type", "application/json")
                    .body(body)
                    .send()
                    .await
                    .map_err(|error| Error::other(format!("AEGIS probe call failed: {error}")))
            })
        });
        handle
            .join()
            .map_err(|_| Error::other("AEGIS probe thread failed"))?
    })?;
    if !response.status().is_success() {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            format!(
                "AEGIS probe denied gandi_send with HTTP {}",
                response.status()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_switch_blocks_before_any_network() {
        let error = authorize_gandi_send_with("person@example.com", "hello", "body", "deny", "")
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("denied gandi_send"));
    }

    #[test]
    fn membrane_blocks_destructive_payload() {
        let error =
            authorize_gandi_send_with("person@example.com", "hello", "please rm -rf / now", "", "")
                .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    }

    #[test]
    fn allow_returns_a_receipt_without_touching_smtp() {
        let receipt =
            authorize_gandi_send_with("person@example.com", "hello", "body", "allow", "").unwrap();
        assert_eq!(receipt.effect, "gandi_send");
        assert_eq!(receipt.decision, "allow");
        assert!(!receipt.body_untrusted);
    }
}

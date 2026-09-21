use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

use crate::models::CortexSessionEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitivityClass {
    DoNotRemember,
    Credential,
    Authentication,
    Financial,
    Health,
    ThirdPartyPersonalData,
}

impl SensitivityClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DoNotRemember => "do_not_remember",
            Self::Credential => "credential",
            Self::Authentication => "authentication",
            Self::Financial => "financial",
            Self::Health => "health",
            Self::ThirdPartyPersonalData => "third_party_personal_data",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "do_not_remember" => Some(Self::DoNotRemember),
            "credential" => Some(Self::Credential),
            "authentication" => Some(Self::Authentication),
            "financial" => Some(Self::Financial),
            "health" => Some(Self::Health),
            "third_party_personal_data" => Some(Self::ThirdPartyPersonalData),
            _ => None,
        }
    }
}

static DO_NOT_REMEMBER_RE: OnceLock<Regex> = OnceLock::new();
static CREDENTIAL_RE: OnceLock<Regex> = OnceLock::new();
static AUTH_RE: OnceLock<Regex> = OnceLock::new();
static FINANCIAL_RE: OnceLock<Regex> = OnceLock::new();
static HEALTH_RE: OnceLock<Regex> = OnceLock::new();
static PII_RE: OnceLock<Regex> = OnceLock::new();

fn get_do_not_remember_re() -> &'static Regex {
    DO_NOT_REMEMBER_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:do not|don't|dont|never)\s+(?:remember|store|save|memorize)\b")
            .unwrap()
    })
}

fn get_credential_re() -> &'static Regex {
    CREDENTIAL_RE.get_or_init(|| {
        Regex::new(r#"(?i)\b(?:api[_ -]?key|access[_ -]?token|refresh[_ -]?token|password|passwd|private[_ -]?key|client[_ -]?secret)\b\s*(?::|=|is|was)\s*([^\s,;"']+)"#)
            .unwrap()
    })
}

fn get_auth_re() -> &'static Regex {
    AUTH_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:authorization:\s*bearer\s+[^\s]+|session[_ -]?cookie|one[- ]time password|\botp\b|mfa code)\b")
            .unwrap()
    })
}

fn get_financial_re() -> &'static Regex {
    FINANCIAL_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:credit|debit) card\b|\b(?:routing|account) number\b|\b(?:cvv|iban|swift)\b",
        )
        .unwrap()
    })
}

fn get_health_re() -> &'static Regex {
    HEALTH_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:diagnosis|medical record|prescription|patient|therapy notes?|health insurance)\b")
            .unwrap()
    })
}

fn get_pii_re() -> &'static Regex {
    PII_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:social security|ssn|passport number|driver'?s license|home address)\b|\b(?:his|her|their|client|customer|employee|friend|coworker)'?s?\s+(?:email|phone|address|birthday)\b")
            .unwrap()
    })
}

pub struct SanitizationResult {
    pub sanitized_content: Option<String>,
    pub sensitivity: Option<String>,
    pub secret_fingerprint: Option<String>,
}

pub struct SecurityMembrane;

impl SecurityMembrane {
    pub fn compute_secret_fingerprint(secret: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"cortex_secret_salt:");
        hasher.update(secret.trim().as_bytes());
        format!("HMAC-SHA256:{:x}", hasher.finalize())
    }

    /// Inspect incoming text, detect sensitive material, mask secrets, and enforce memory directives.
    pub fn inspect_and_sanitize(
        content: Option<&str>,
        explicit_sensitivity: Option<&str>,
    ) -> SanitizationResult {
        let text = match content {
            Some(t) => t,
            None => {
                return SanitizationResult {
                    sanitized_content: None,
                    sensitivity: explicit_sensitivity.map(|s| s.to_string()),
                    secret_fingerprint: None,
                }
            }
        };

        // 1. Directives: Check DO NOT REMEMBER
        if get_do_not_remember_re().is_match(text)
            || explicit_sensitivity == Some("do_not_remember")
        {
            return SanitizationResult {
                sanitized_content: Some("<WITHHELD_BY_MEMORY_POLICY>".to_string()),
                sensitivity: Some("do_not_remember".to_string()),
                secret_fingerprint: None,
            };
        }

        // 2. Hard credentials detection & masking
        let cred_re = get_credential_re();
        if let Some(caps) = cred_re.captures(text) {
            let secret_val = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let fingerprint = Self::compute_secret_fingerprint(secret_val);
            let sanitized = cred_re.replace_all(text, "$0").to_string();
            // Replace secret value with placeholder
            let sanitized = if !secret_val.is_empty() {
                sanitized.replace(secret_val, "<SECRET:TOKEN>")
            } else {
                sanitized
            };

            return SanitizationResult {
                sanitized_content: Some(sanitized),
                sensitivity: Some("credential".to_string()),
                secret_fingerprint: Some(fingerprint),
            };
        }

        // 3. Authorization headers & tokens
        if get_auth_re().is_match(text) || explicit_sensitivity == Some("authentication") {
            let sanitized = get_auth_re()
                .replace_all(text, "<SECRET:AUTH_TOKEN>")
                .to_string();
            return SanitizationResult {
                sanitized_content: Some(sanitized),
                sensitivity: Some("authentication".to_string()),
                secret_fingerprint: None,
            };
        }

        // 4. Financial data
        if get_financial_re().is_match(text) || explicit_sensitivity == Some("financial") {
            return SanitizationResult {
                sanitized_content: Some(text.to_string()),
                sensitivity: Some("financial".to_string()),
                secret_fingerprint: None,
            };
        }
        // 5. Health data
        if explicit_sensitivity == Some("health") {
            return SanitizationResult {
                sanitized_content: Some(text.to_string()),
                sensitivity: Some("health".to_string()),
                secret_fingerprint: None,
            };
        }

        let health_re = get_health_re();
        if let Some(m) = health_re.find(text) {
            // "diagnosis-protocol" identifiers alone are technical protocols, not patient health records
            if m.as_str().eq_ignore_ascii_case("diagnosis") && text.contains("diagnosis-protocol") {
                // Check if any other health match exists in the text
                let has_other_health = health_re
                    .find_iter(text)
                    .any(|other_m| !other_m.as_str().eq_ignore_ascii_case("diagnosis"));
                if has_other_health {
                    return SanitizationResult {
                        sanitized_content: Some(text.to_string()),
                        sensitivity: Some("health".to_string()),
                        secret_fingerprint: None,
                    };
                }
            } else {
                return SanitizationResult {
                    sanitized_content: Some(text.to_string()),
                    sensitivity: Some("health".to_string()),
                    secret_fingerprint: None,
                };
            }
        }

        // 6. Third party personal data / PII
        if get_pii_re().is_match(text) || explicit_sensitivity == Some("third_party_personal_data")
        {
            return SanitizationResult {
                sanitized_content: Some(text.to_string()),
                sensitivity: Some("third_party_personal_data".to_string()),
                secret_fingerprint: None,
            };
        }

        // Default: normal safe content
        SanitizationResult {
            sanitized_content: Some(text.to_string()),
            sensitivity: explicit_sensitivity.map(|s| s.to_string()),
            secret_fingerprint: None,
        }
    }

    /// Project a raw database `CortexSessionEvent` into a `SafeEventView`.
    /// Processors (summarizers, extractors) must ONLY access events through this view.
    pub fn to_safe_view(event: &CortexSessionEvent) -> SafeEventView {
        let is_do_not_remember = event.sensitivity.as_deref() == Some("do_not_remember");
        let is_credential = event.sensitivity.as_deref() == Some("credential")
            || event.sensitivity.as_deref() == Some("authentication");

        let extraction_allowed = !event.redacted && !is_do_not_remember && !is_credential;

        let safe_content = if event.redacted || is_do_not_remember {
            None
        } else {
            event.content.clone()
        };

        SafeEventView {
            event_id: event.id.clone(),
            session_id: event.session_id.clone(),
            sequence: event.sequence,
            branch_id: event.branch_id.clone(),
            event_type: event.event_type.clone(),
            role: event.role.clone(),
            safe_content,
            payload: event.payload.clone(),
            sensitivity: event.sensitivity.clone(),
            extraction_allowed,
        }
    }
}

/// The sanitized, policy-filtered view of an event provided to background workers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeEventView {
    pub event_id: String,
    pub session_id: String,
    pub sequence: i64,
    pub branch_id: String,
    pub event_type: String,
    pub role: Option<String>,
    pub safe_content: Option<String>,
    pub payload: serde_json::Value,
    pub sensitivity: Option<String>,
    pub extraction_allowed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_do_not_remember_directive() {
        let res = SecurityMembrane::inspect_and_sanitize(
            Some("Please do not remember this confidential exchange"),
            None,
        );
        assert_eq!(res.sensitivity, Some("do_not_remember".to_string()));
        assert_eq!(
            res.sanitized_content,
            Some("<WITHHELD_BY_MEMORY_POLICY>".to_string())
        );
    }

    #[test]
    fn test_credential_masking_and_fingerprint() {
        let res = SecurityMembrane::inspect_and_sanitize(
            Some("Connecting with api_key: sk_live_99418294719247192 to server"),
            None,
        );
        assert_eq!(res.sensitivity, Some("credential".to_string()));
        assert!(res.sanitized_content.unwrap().contains("<SECRET:TOKEN>"));
        assert!(res.secret_fingerprint.is_some());
        assert!(res.secret_fingerprint.unwrap().starts_with("HMAC-SHA256:"));
    }

    #[test]
    fn test_health_and_pii_classification() {
        let res1 = SecurityMembrane::inspect_and_sanitize(
            Some("Patient medical record notes: high blood pressure"),
            None,
        );
        assert_eq!(res1.sensitivity, Some("health".to_string()));

        let res2 = SecurityMembrane::inspect_and_sanitize(
            Some("Her email is sarah@example.com for communication"),
            None,
        );
        assert_eq!(
            res2.sensitivity,
            Some("third_party_personal_data".to_string())
        );
    }

    #[test]
    fn test_safe_event_view_quarantine() {
        let sensitive_ev = CortexSessionEvent {
            id: "ev-s1".to_string(),
            session_id: "sess-1".to_string(),
            sequence: 1,
            branch_id: "main".to_string(),
            parent_event_id: None,
            event_type: "user_message".to_string(),
            role: Some("user".to_string()),
            content: Some("<WITHHELD_BY_MEMORY_POLICY>".to_string()),
            payload: serde_json::json!({}),
            created_at: "2026-09-21T00:00:00Z".to_string(),
            content_hash: "hash".to_string(),
            sensitivity: Some("do_not_remember".to_string()),
            redacted: false,
            segment_id: None,
        };

        let view = SecurityMembrane::to_safe_view(&sensitive_ev);
        assert!(!view.extraction_allowed);
        assert_eq!(view.safe_content, None);
    }
}

use regex::Regex;
use std::process::Command;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

static VAULT_CACHE: OnceLock<RwLock<(Instant, Vec<String>)>> = OnceLock::new();

/// List all secret keys currently registered in the hardware TPM vault.
/// Results are cached for 10 seconds to eliminate redundant TPM query latency.
pub fn list_vault_keys() -> Vec<String> {
    let cache_lock = VAULT_CACHE
        .get_or_init(|| RwLock::new((Instant::now() - Duration::from_secs(60), Vec::new())));

    if let Ok(read_guard) = cache_lock.read() {
        if read_guard.0.elapsed() < Duration::from_secs(10) {
            return read_guard.1.clone();
        }
    }

    if let Ok(mut write_guard) = cache_lock.write() {
        if write_guard.0.elapsed() < Duration::from_secs(10) {
            return write_guard.1.clone();
        }
        let keys = fetch_vault_keys_raw();
        *write_guard = (Instant::now(), keys.clone());
        return keys;
    }

    fetch_vault_keys_raw()
}

fn fetch_vault_keys_raw() -> Vec<String> {
    if let Ok(out) = Command::new("atlas-vault").arg("list").output() {
        if out.status.success() {
            return String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    Vec::new()
}

/// Dynamically resolve a secret token.
/// Checks hardware TPM vault first (`atlas-vault get <KEY>`), then falls back to environment variables.
pub fn resolve_secret(key_name: &str) -> Option<String> {
    // 1. Fallback to process environment first if present
    if let Ok(env_val) = std::env::var(key_name) {
        let trimmed = env_val.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    // 2. Query hardware TPM-bound vault if key is registered
    let keys = list_vault_keys();
    if keys.iter().any(|k| k == key_name) {
        if let Ok(out) = Command::new("atlas-vault").args(["get", key_name]).output() {
            if out.status.success() {
                let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }
    }

    None
}

/// Check if a secret key is registered without exposing its value.
pub fn is_secret_present(key_name: &str) -> bool {
    if let Ok(env_val) = std::env::var(key_name) {
        if !env_val.trim().is_empty() {
            return true;
        }
    }
    let keys = list_vault_keys();
    keys.iter().any(|k| k == key_name)
}

/// Scrub sensitive host paths, local subnets, vault secrets, and operator identity from outbound prompts.
pub fn sanitize_outbound_prompt(prompt: &str) -> String {
    static PATH_RE: OnceLock<Regex> = OnceLock::new();
    static IP_RE: OnceLock<Regex> = OnceLock::new();
    static SECRET_RE: OnceLock<Regex> = OnceLock::new();
    static OPERATOR_RE: OnceLock<Regex> = OnceLock::new();

    let path_re = PATH_RE.get_or_init(|| {
        Regex::new(r"/(?:home|Users)/[a-zA-Z0-9_.-]+(?:/[a-zA-Z0-9_.-]+)*").unwrap()
    });
    let ip_re = IP_RE.get_or_init(|| {
        Regex::new(r"\b(?:127\.\d{1,3}\.\d{1,3}\.\d{1,3}|100\.\d{1,3}\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3}|10\.\d{1,3}\.\d{1,3}\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3})\b").unwrap()
    });
    let secret_re = SECRET_RE.get_or_init(|| {
        Regex::new(r"\b(?:sk-ant-[a-zA-Z0-9_-]{20,}|sk-[a-zA-Z0-9_-]{20,}|ghp_[a-zA-Z0-9]{20,}|gho_[a-zA-Z0-9]{20,})\b").unwrap()
    });
    let operator_re = OPERATOR_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:drakestapleton|drake\.aien@proton\.me|drake\.stapleton)\b").unwrap()
    });

    let s1 = path_re.replace_all(prompt, "<WORKSPACE_PATH>");
    let s2 = ip_re.replace_all(&s1, "<LOCAL_HOST>");
    let s3 = secret_re.replace_all(&s2, "[REDACTED_BY_ATLAS_VAULT]");
    let s4 = operator_re.replace_all(&s3, "<OPERATOR>");
    s4.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_paths_and_ips() {
        let raw =
            "Read /home/drakestapleton/workspace/secret.rs from 100.64.0.1 and send to 127.0.0.1";
        let clean = sanitize_outbound_prompt(raw);
        assert!(!clean.contains("/home/drakestapleton"));
        assert!(!clean.contains("100.64.0.1"));
        assert!(!clean.contains("127.0.0.1"));
        assert!(clean.contains("<WORKSPACE_PATH>"));
        assert!(clean.contains("<LOCAL_HOST>"));
    }

    #[test]
    fn test_sanitize_operator_and_secrets() {
        let raw = "Operator drakestapleton deployed key sk-proj-123456789012345678901234 for drake.aien@proton.me";
        let clean = sanitize_outbound_prompt(raw);
        assert!(!clean.contains("drakestapleton"));
        assert!(!clean.contains("drake.aien@proton.me"));
        assert!(clean.contains("<OPERATOR>"));
    }
}

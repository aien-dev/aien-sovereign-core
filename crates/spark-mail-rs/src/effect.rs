//! Outbound effect gate for mail. SMTP is unreachable until this returns allow.
//!
//! The deterministic floor matches the AEGIS pre-dispatch membrane: an empty
//! effect is rejected, and destructive payloads are rejected. `AIEN_EFFECT_POLICY=deny`
//! is the operator kill switch. When `AIEN_AEGIS_URL` is set, the send also has
//! to receive HTTP 200 from that AEGIS process before any socket to Gandi opens.

use cortex_rs::{CandidateWriteInput, CreateSessionInput, Database, VerificationTier};
use serde::Serialize;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EffectReceipt {
    pub effect: &'static str,
    pub decision: &'static str,
    pub to: String,
    pub subject: String,
    pub body_untrusted: bool,
    pub aegis_url: Option<String>,
    pub cortex_id: String,
}

pub fn authorize_gandi_send(to: &str, subject: &str, body: &str) -> Result<EffectReceipt, Error> {
    let policy = std::env::var("AIEN_EFFECT_POLICY").unwrap_or_default();
    let aegis_url = std::env::var("AIEN_AEGIS_URL").unwrap_or_default();
    let probe = std::env::var("AIEN_PROBE_ENFORCE")
        .ok()
        .is_some_and(|value| {
            let value = value.trim().to_ascii_lowercase();
            value == "1" || value == "true" || value == "on"
        });
    let cortex = cortex_db_path();
    authorize_gandi_send_with(
        to,
        subject,
        body,
        policy.trim(),
        aegis_url.trim(),
        probe,
        Some(cortex.as_path()),
    )
}

fn cortex_db_path() -> PathBuf {
    if let Ok(path) = std::env::var("AIEN_CORTEX_DB")
        && !path.trim().is_empty()
    {
        return PathBuf::from(path.trim());
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/aien/effect-cortex.db")
}

pub fn authorize_gandi_send_with(
    to: &str,
    subject: &str,
    body: &str,
    policy: &str,
    aegis_url: &str,
    probe_enforce: bool,
    cortex_db: Option<&Path>,
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
    let args = serde_json::json!({
        "to": to,
        "subject": subject,
        "body_len": body.len(),
    });
    aegis_pre_dispatch("gandi_send", &args)?;
    if probe_enforce {
        aegis_probe_opinion("gandi_send", &args)?;
    }
    let aegis_url = (!aegis_url.is_empty()).then(|| aegis_url.to_string());
    if let Some(url) = &aegis_url {
        confirm_aegis(url, to, subject)?;
    }
    let cortex_path = cortex_db
        .ok_or_else(|| Error::other("an allowed gandi_send has no Cortex database to record it"))?;
    let cortex_id = record_allow(cortex_path, to, subject)?;
    Ok(EffectReceipt {
        effect: "gandi_send",
        decision: "allow",
        to: to.to_string(),
        subject: subject.to_string(),
        body_untrusted: false,
        aegis_url,
        cortex_id,
    })
}

/// Same floor as `aegis-runtime` `enforcement::pre_dispatch_check`. The aegis
/// crate cannot be linked from this workspace: both own `aien-inference-protocol`.
fn aegis_pre_dispatch(skill_name: &str, args: &serde_json::Value) -> Result<(), Error> {
    let name = skill_name.trim();
    if name.is_empty() {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            "Empty skill name is never dispatched",
        ));
    }
    if name == "bash_eval" {
        let cmd = args.get("command").and_then(|c| c.as_str()).unwrap_or("");
        if cmd.trim().is_empty() {
            return Err(Error::new(
                ErrorKind::PermissionDenied,
                "Empty shell command is never dispatched",
            ));
        }
        let normalized = cmd
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for pattern in [
            "rm -rf /",
            "rm -rf /*",
            "rm -rf ~",
            "mkfs",
            "dd ",
            "of=/dev/",
            ":(){",
            "chmod -r 777 /",
            "chmod -R 777 /",
        ] {
            if normalized.contains(pattern) {
                return Err(Error::new(
                    ErrorKind::PermissionDenied,
                    format!(
                        "Command blocked by enforcement membrane: matched destructive pattern '{pattern}'"
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Same questions as `ProbePolicyGuard::check_action`, evaluated by the
/// in-process reference oracle from `aien-probe`.
fn aegis_probe_opinion(action_name: &str, args: &serde_json::Value) -> Result<(), Error> {
    use aien_probe::{
        Choice, ChoiceOption, DeterministicReferenceBackend, Noul, Probe, ProbeEngine, ProbeSet,
    };
    let state = serde_json::json!({
        "action": action_name,
        "payload": args.to_string(),
    });
    let probes = ProbeSet::new()
        .with(
            "safety",
            Probe::Noul(Noul::new(
                "Is this agent action safe, non-destructive, and strictly within security bounds?",
            )),
        )
        .with(
            "boundary",
            Probe::Choice(Choice::new(
                "Classify permission scope of the proposed action.",
                [
                    ChoiceOption::new("contained")
                        .with_description("Operates inside permitted workspace"),
                    ChoiceOption::new("unauthorized_escalation")
                        .with_description("Escalates outside workspace"),
                ],
            )),
        );
    let engine = ProbeEngine::new(DeterministicReferenceBackend::new());
    let response = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| Error::other(error.to_string()))?;
                runtime
                    .block_on(engine.evaluate(&state, &probes))
                    .map_err(|error| Error::other(error.to_string()))
            })
            .join()
            .map_err(|_| Error::other("AEGIS probe thread failed"))?
    })?;
    let safety = response
        .noul("safety")
        .map(|answer| answer.noul)
        .unwrap_or(0.0);
    let boundary = response
        .choice("boundary")
        .map(|answer| answer.choice.clone())
        .unwrap_or_else(|| "unauthorized_escalation".to_string());
    if safety < 0.5 || boundary == "unauthorized_escalation" {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            format!(
                "Action '{action_name}' blocked by sovereign probe policy: safety {safety:.4} < 0.5000, scope '{boundary}'"
            ),
        ));
    }
    Ok(())
}

fn record_allow(path: &Path, to: &str, subject: &str) -> Result<String, Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| Error::other(format!("cannot create Cortex directory: {error}")))?;
    }
    let db = Database::open(path)
        .map_err(|error| Error::other(format!("cannot open Cortex database: {error}")))?;
    let existing = db
        .get_session("gandi-send")
        .map_err(|error| Error::other(format!("cannot read the effect session: {error}")))?;
    if existing.is_none() {
        db.create_session(&CreateSessionInput {
            id: Some("gandi-send".to_string()),
            space: "atlas-memory".to_string(),
            agent_id: Some("aien".to_string()),
            world_id: None,
            parent_session_id: None,
            fork_event_id: None,
            retention_class: "standard".to_string(),
            metadata: serde_json::json!({}),
        })
        .map_err(|error| Error::other(format!("cannot open the effect session: {error}")))?;
    }
    let candidate = db
        .insert_memory_candidate(&CandidateWriteInput {
            id: None,
            space: "atlas-memory".to_string(),
            session_id: "gandi-send".to_string(),
            branch_id: "main".to_string(),
            memory_type: "effect".to_string(),
            subject: "gandi_send".to_string(),
            predicate: "allowed".to_string(),
            object_value: serde_json::json!({
                "to": to,
                "subject": subject,
            }),
            scope: "global".to_string(),
            confidence: 1.0,
            verification_tier: Some(VerificationTier::T0Direct),
            extractor_version: "effect-gate-1".to_string(),
            evidence_ids: vec![],
        })
        .map_err(|error| Error::other(format!("Cortex did not store the allow: {error}")))?;
    Ok(candidate.id)
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
        let error = authorize_gandi_send_with(
            "person@example.com",
            "hello",
            "body",
            "deny",
            "",
            false,
            None,
        )
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("denied gandi_send"));
    }

    #[test]
    fn membrane_blocks_destructive_payload() {
        let error = authorize_gandi_send_with(
            "person@example.com",
            "hello",
            "please rm -rf / now",
            "",
            "",
            false,
            None,
        )
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    }

    #[test]
    fn allow_returns_a_receipt_without_touching_smtp() {
        let path =
            std::env::temp_dir().join(format!("aien-effect-cortex-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let receipt = authorize_gandi_send_with(
            "person@example.com",
            "hello",
            "body",
            "allow",
            "",
            false,
            Some(&path),
        )
        .unwrap();
        assert_eq!(receipt.effect, "gandi_send");
        assert_eq!(receipt.decision, "allow");
        assert!(!receipt.body_untrusted);
        assert!(!receipt.cortex_id.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn aegis_membrane_rejects_an_empty_effect_name() {
        let error = aegis_pre_dispatch("", &serde_json::json!({})).unwrap_err();
        assert!(error.to_string().contains("Empty"));
    }

    #[test]
    fn probe_opinion_is_deterministic_for_one_payload() {
        let args = serde_json::json!({"to": "person@example.com", "subject": "hello"});
        let first = aegis_probe_opinion("gandi_send", &args);
        let second = aegis_probe_opinion("gandi_send", &args);
        assert_eq!(first.is_ok(), second.is_ok());
        if let Err(error) = first {
            assert!(error.to_string().contains("probe policy"));
        }
    }
}

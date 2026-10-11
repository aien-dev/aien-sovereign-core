//! sovereign-core #408: the operator's objective record, as the daemon reads it.
//!
//! `aien-cli objective record` (sc#388) writes `objectives/<objective_id>.json`
//! under `AIEN_PROVENANCE_DIR`; the id is sha256 of [`DOMAIN_TAG`], LF, the
//! text. The daemon never writes the record. It re-reads it when a grant is
//! bound to it (`ComposeAuthorize` with an objective id), before every intent
//! on such a grant, and at every reconcile of such an intent, so a record that
//! is gone or no longer says what was approved is named, never passed over.
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const DOMAIN_TAG: &str = "AIEN_E2E_OBJECTIVE_V1";
/// The record is missing, unreadable, not JSON, or the id is not 64 lowercase hex.
pub const MISSING: &str = "ObjectiveMissing";
/// The record does not say what the grant was approved for.
pub const MISMATCH: &str = "ObjectiveMismatch";

fn sha256_hex(b: &[u8]) -> String {
    aien_omega_compose::hex(&Sha256::digest(b))
}

/// sha256 of `DOMAIN_TAG`, LF, `text` (lowercase hex).
pub fn objective_id(text: &str) -> String {
    sha256_hex(format!("{DOMAIN_TAG}\n{text}").as_bytes())
}

/// `$AIEN_PROVENANCE_DIR/objectives` (default `/tmp/aien-provenance/objectives`),
/// resolved exactly as the CLI resolves it.
pub fn dir() -> PathBuf {
    std::env::var("AIEN_PROVENANCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/aien-provenance"))
        .join("objectives")
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// What one read of a record established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub objective_id: String,
    /// sha256 of the recorded text.
    pub text_sha256: String,
    /// sha256 of the record file's bytes.
    pub record_sha256: String,
}

/// A refusal name ([`MISSING`] or [`MISMATCH`]) and its detail.
pub type Refused = (&'static str, String);

/// Read the record of `id` now. It must exist, name `id`, and its text must
/// hash to both its `objective_text_sha256` and (with the tag) to `id`.
pub fn read(id: &str) -> Result<Seen, Refused> {
    if !valid_id(id) {
        return Err((
            MISSING,
            format!("{id:?} is not a 64 character lowercase hex objective id"),
        ));
    }
    let path = dir().join(format!("{id}.json"));
    let bytes = std::fs::read(&path)
        .map_err(|e| (MISSING, format!("objective record {}: {e}", path.display())))?;
    let v: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| (MISSING, format!("objective record {}: {e}", path.display())))?;
    let text = v["objective_text"].as_str().unwrap_or_default();
    let text_sha = sha256_hex(text.as_bytes());
    if v["objective_id"].as_str() != Some(id)
        || v["objective_text_sha256"].as_str() != Some(text_sha.as_str())
        || objective_id(text) != id
    {
        return Err((
            MISMATCH,
            format!(
                "objective record {} does not hold the text of objective {id}",
                path.display()
            ),
        ));
    }
    Ok(Seen {
        objective_id: id.to_string(),
        text_sha256: text_sha,
        record_sha256: sha256_hex(&bytes),
    })
}

/// [`read`], and the recorded text must be the one approved (`text_sha256`).
pub fn check(id: &str, text_sha256: &str) -> Result<Seen, Refused> {
    let s = read(id)?;
    if s.text_sha256 != text_sha256 {
        return Err((
            MISMATCH,
            format!(
                "objective record {id} holds text sha256 {}, approved was {text_sha256}",
                s.text_sha256
            ),
        ));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_the_cli_formula() {
        // sha256("AIEN_E2E_OBJECTIVE_V1\nx"), the sc#388 definition.
        let want = sha256_hex(b"AIEN_E2E_OBJECTIVE_V1\nx");
        assert_eq!(objective_id("x"), want);
        assert!(valid_id(&want));
        assert!(!valid_id("ABC"));
        assert!(!valid_id(&want.to_uppercase()));
    }
}

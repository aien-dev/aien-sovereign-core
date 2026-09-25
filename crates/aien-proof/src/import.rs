//! Import external qualification output as receipts.
//!
//! Other repositories (SEED-0B runs, rollback verifiers, Store v1 crash
//! tests) emit a small machine readable assertion file. `import` turns that
//! file plus explicit source, artifact, and dependency references into a
//! sealed EvidenceReceiptV1. Prose is never parsed; the input contract is
//! JSON and any deviation fails closed.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::evidence::{
    check_commit, Assertion, LedgerRef, LeaseRef, Mutation, Receipt, Tier, Verdict, SCHEMA,
    SCHEMA_VERSION,
};
use crate::lease::check_resource_name;

/// Machine readable assertion file produced by each test setup.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssertionFile {
    pub assertions: Vec<AssertionInput>,
    /// Optional top level result. When absent it is derived from the
    /// assertions. A claimed PASS with a failing assertion is refused.
    #[serde(default)]
    pub result: Option<String>,
}

/// One assertion line in the input file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssertionInput {
    pub id: String,
    #[serde(default)]
    pub expected: String,
    #[serde(default)]
    pub observed: String,
    pub result: AssertionResult,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub note: String,
}

/// Accepts `pass`/`fail` strings or booleans.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum AssertionResult {
    Bool(bool),
    Word(String),
}

impl AssertionResult {
    fn pass(&self) -> Result<bool, String> {
        match self {
            AssertionResult::Bool(b) => Ok(*b),
            AssertionResult::Word(w) => match w.to_ascii_lowercase().as_str() {
                "pass" => Ok(true),
                "fail" => Ok(false),
                _ => Err(format!("assertion result must be pass or fail, got {w:?}")),
            },
        }
    }
}

/// All inputs needed to mint a receipt from external output.
#[derive(Debug, Clone, Default)]
pub struct ImportRequest {
    pub repo: String,
    pub commit: String,
    pub dirty: bool,
    pub kind: String,
    pub tier: String,
    pub toolchain: String,
    pub procedure: String,
    pub machine: String,
    pub input_artifacts: Vec<String>,
    pub output_artifacts: Vec<String>,
    pub dependencies: Vec<String>,
    pub declared_mutation: Option<String>,
    pub observed_mutation: Option<String>,
    pub authority: String,
    pub output_digest: Option<String>,
    pub output_file: Option<String>,
    pub external_refs: Vec<String>,
    pub ledger_index: Option<u64>,
    pub ledger_hash: Option<String>,
    pub lease_hold: Option<String>,
    pub lease_resource: Option<String>,
    pub timestamp: Option<u64>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parse and validate the assertion file. Never sniffs prose.
pub fn read_assertion_file(path: &Path) -> Result<(Vec<Assertion>, Verdict), String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("cannot read assertion file: {e}"))?;
    let file: AssertionFile =
        serde_json::from_str(&text).map_err(|e| format!("assertion file invalid: {e}"))?;
    if file.assertions.is_empty() {
        return Err("assertion file must carry at least one assertion".to_string());
    }
    let mut out = Vec::with_capacity(file.assertions.len());
    for input in &file.assertions {
        if input.id.trim().is_empty() {
            return Err("assertion id must not be empty".to_string());
        }
        out.push(Assertion {
            id: input.id.clone(),
            expected: input.expected.clone(),
            observed: input.observed.clone(),
            pass: input.result.pass()?,
            source: input.source.clone(),
            note: input.note.clone(),
        });
    }
    let derived = if out.iter().all(|a| a.pass) {
        Verdict::Pass
    } else {
        Verdict::Fail
    };
    if let Some(claimed) = file.result {
        let verdict =
            Verdict::parse(&claimed).ok_or_else(|| format!("bad result {claimed:?}"))?;
        if verdict == Verdict::Pass && derived != Verdict::Pass {
            return Err("assertion file claims PASS but an assertion failed".to_string());
        }
        // A setup may only downgrade (BLOCKED, INCOMPLETE, SKIPPED), never
        // upgrade a failing run. FAIL stays FAIL.
        if derived == Verdict::Fail && verdict == Verdict::Pass {
            return Err("cannot upgrade a failing run to PASS".to_string());
        }
        if derived == Verdict::Pass && verdict == Verdict::Fail {
            return Err("file claims FAIL but every assertion passed".to_string());
        }
        Ok((out, verdict))
    } else {
        Ok((out, derived))
    }
}

/// Build a sealed receipt from validated inputs. Hardware tiers require an
/// explicit declared mutation and a lease reference; PASS receipts require
/// an output digest (direct hex or hashed from an output file).
pub fn build_receipt(req: &ImportRequest, assertions: Vec<Assertion>, result: Verdict) -> Result<Receipt, String> {
    let tier = Tier::parse(&req.tier)
        .ok_or_else(|| format!("unknown qualification tier {:?}", req.tier))?;
    check_commit(&req.commit)?;
    if req.repo.trim().is_empty() {
        return Err("repo must not be empty".to_string());
    }
    if req.kind.trim().is_empty() {
        return Err("gate/kind must not be empty".to_string());
    }
    if req.procedure.trim().is_empty() {
        return Err("procedure identity must not be empty".to_string());
    }
    if req.machine.trim().is_empty() {
        return Err("machine identity must not be empty".to_string());
    }
    for dep in &req.dependencies {
        let low = dep.to_ascii_lowercase();
        if low.len() != 64 || !low.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("dependency {dep:?} is not a 64 digit receipt hash"));
        }
    }
    let declared = match &req.declared_mutation {
        Some(s) => Mutation::parse(s)
            .ok_or_else(|| format!("unknown mutation class {s:?}"))?,
        None => {
            if tier.is_hardware() {
                return Err("hardware receipts require --declared-mutation".to_string());
            }
            Mutation::None
        }
    };
    let observed = match &req.observed_mutation {
        Some(s) => Mutation::parse(s)
            .ok_or_else(|| format!("unknown mutation class {s:?}"))?,
        None => declared,
    };
    let output_digest = match (&req.output_digest, &req.output_file) {
        (Some(hex), _) => {
            if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("output digest must be 64 hex digits (BLAKE3)".to_string());
            }
            hex.to_ascii_lowercase()
        }
        (None, Some(path)) => {
            let bytes = std::fs::read(path).map_err(|e| format!("cannot read output file: {e}"))?;
            blake3::hash(&bytes).to_hex().to_string()
        }
        (None, None) => {
            if result == Verdict::Pass {
                return Err("PASS receipts require --output-digest or --output-file".to_string());
            }
            "0".repeat(64)
        }
    };
    let ledger = match (req.ledger_index, &req.ledger_hash) {
        (Some(index), Some(hash)) => {
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("ledger hash must be 64 hex digits".to_string());
            }
            Some(LedgerRef {
                index,
                hash: hash.to_ascii_lowercase(),
            })
        }
        (None, None) => None,
        _ => return Err("ledger reference needs both index and hash".to_string()),
    };
    let lease = match (&req.lease_hold, &req.lease_resource) {
        (Some(hold), Some(resource)) => {
            check_resource_name(resource)?;
            if hold.len() != 64 || !hold.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("lease hold id must be 64 hex digits".to_string());
            }
            Some(LeaseRef {
                hold_id: hold.to_ascii_lowercase(),
                resource: resource.clone(),
            })
        }
        (None, None) => None,
        _ => return Err("lease binding needs both hold id and resource".to_string()),
    };
    if tier.is_hardware() && lease.is_none() {
        return Err("hardware receipts require --lease-hold and --lease-resource".to_string());
    }
    let mut receipt = Receipt {
        schema: SCHEMA.to_string(),
        version: SCHEMA_VERSION,
        id: String::new(),
        kind: req.kind.clone(),
        tier,
        result,
        timestamp: req.timestamp.unwrap_or_else(now),
        repo: req.repo.clone(),
        commit: req.commit.to_ascii_lowercase(),
        dirty: req.dirty,
        toolchain: req.toolchain.clone(),
        procedure: req.procedure.clone(),
        machine: req.machine.clone(),
        env_class: tier.as_str().to_string(),
        input_artifacts: req.input_artifacts.clone(),
        output_artifacts: req.output_artifacts.clone(),
        assertions,
        dependencies: req.dependencies.iter().map(|d| d.to_ascii_lowercase()).collect(),
        declared_mutation: declared,
        observed_mutation: observed,
        authority: req.authority.clone(),
        output_digest,
        external_refs: req.external_refs.clone(),
        ledger,
        lease,
        required_features: vec![],
        reserved: String::new(),
    };
    let id = crate::evidence::receipt_id(&receipt)?;
    receipt.id = id;
    // Final self check: identity plus consistency, fail closed.
    let text = serde_json::to_vec(&receipt).map_err(|e| e.to_string())?;
    let (_, report) = crate::evidence::verify_bytes(&text)?;
    if report.status == Verdict::Fail {
        return Err(format!("built receipt fails self check: {}", report.reasons.join("; ")));
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{store_receipt, verify_bytes};

    fn write(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
    }

    fn req() -> ImportRequest {
        ImportRequest {
            repo: "https://github.com/aien-dev/aienos".to_string(),
            commit: "a".repeat(40),
            dirty: false,
            kind: "seed-0b-qemu".to_string(),
            tier: "QEMU".to_string(),
            toolchain: "rustc 1.90.0".to_string(),
            procedure: "scripts/qemu_seed0b.sh".to_string(),
            machine: "qemu-virt-aarch64".to_string(),
            input_artifacts: vec!["blake3:".to_string() + &"b".repeat(64)],
            output_digest: Some("c".repeat(64)),
            ..Default::default()
        }
    }

    #[test]
    fn import_roundtrip() {
        let dir = crate::testutil::temp_dir("import-ok");
        let file = dir.join("assert.json");
        write(
            &file,
            r#"{"assertions": [{"id": "artifact_signature_valid", "expected": "valid", "observed": "valid", "result": "pass", "source": "qemu"}]}"#,
        );
        let (assertions, result) = read_assertion_file(&file).unwrap();
        assert_eq!(result, Verdict::Pass);
        let receipt = build_receipt(&req(), assertions, result).unwrap();
        let store = crate::testutil::temp_dir("import-store");
        let id = store_receipt(&store, &receipt).unwrap();
        assert_eq!(id, receipt.id);
        let text = std::fs::read(store.join("receipts").join(format!("{id}.json"))).unwrap();
        let (_, report) = verify_bytes(&text).unwrap();
        assert_eq!(report.status, Verdict::Pass);
    }

    #[test]
    fn prose_is_never_parsed() {
        let dir = crate::testutil::temp_dir("import-prose");
        let file = dir.join("assert.json");
        write(&file, "all tests passed, trust me");
        assert!(read_assertion_file(&file).is_err());
    }

    #[test]
    fn claimed_pass_with_failed_assertion_refused() {
        let dir = crate::testutil::temp_dir("import-liar");
        let file = dir.join("assert.json");
        write(
            &file,
            r#"{"result": "PASS", "assertions": [{"id": "x", "result": "fail"}]}"#,
        );
        assert!(read_assertion_file(&file).is_err());
    }

    #[test]
    fn hardware_import_needs_mutation_and_lease() {
        let dir = crate::testutil::temp_dir("import-hw");
        let file = dir.join("assert.json");
        write(&file, r#"{"assertions": [{"id": "x", "result": "pass"}]}"#);
        let (assertions, result) = read_assertion_file(&file).unwrap();
        let mut r = req();
        r.tier = "MACHINE1_ATTENDED".to_string();
        r.machine = "machine-1".to_string();
        assert!(build_receipt(&r, assertions.clone(), result).is_err());
        r.declared_mutation = Some("REMOVABLE_MEDIA_ONLY".to_string());
        assert!(build_receipt(&r, assertions.clone(), result).is_err());
        r.lease_hold = Some("e".repeat(64));
        r.lease_resource = Some("machine-1".to_string());
        r.procedure = "scripts/seed0b_machine1.sh".to_string();
        let receipt = build_receipt(&r, assertions, result).unwrap();
        assert_eq!(receipt.env_class, "MACHINE1_ATTENDED");
        let _ = dir;
    }

    #[test]
    fn unknown_tier_and_mutation_refused() {
        let dir = crate::testutil::temp_dir("import-bad-enum");
        let file = dir.join("assert.json");
        write(&file, r#"{"assertions": [{"id": "x", "result": "pass"}]}"#);
        let (assertions, result) = read_assertion_file(&file).unwrap();
        let mut r = req();
        r.tier = "REAL_HARDWARE".to_string();
        assert!(build_receipt(&r, assertions.clone(), result).is_err());
        r.tier = "QEMU".to_string();
        r.declared_mutation = Some("FORMAT_EVERYTHING".to_string());
        assert!(build_receipt(&r, assertions, result).is_err());
        let _ = dir;
    }
}

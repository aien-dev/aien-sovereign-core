//! Gate manifests: small declarative descriptions of what closes a gate.
//!
//! The evidence system never decides architectural policy. It evaluates a
//! checked in manifest against stored receipts and reports exactly which
//! prerequisite is responsible for PASS, FAIL, BLOCKED, or INCOMPLETE.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::chain::tier_satisfies;
use crate::evidence::{list_receipts, load_receipt, Tier, Verdict};

/// Subdirectory of the proof store holding gate manifests.
pub const GATE_DIR: &str = "gates";

/// One prerequisite inside a gate manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    /// Human label used in status output, for example `SEED-0B-QEMU`.
    pub name: String,
    /// Receipt kind that satisfies this requirement, for example `m3`.
    pub kind: String,
    /// Optional nested gate requirement. When set, `kind` must be empty.
    #[serde(default)]
    pub gate_ref: Option<String>,
    /// Minimum qualification tier. A QEMU receipt never satisfies a
    /// Machine 1 requirement.
    #[serde(default = "default_tier")]
    pub min_tier: Tier,
    /// Assertion IDs that must exist and pass on the chosen receipt.
    #[serde(default)]
    pub assertions: Vec<String>,
    /// Optional source constraints for assertions whose origin matters.
    #[serde(default)]
    pub assertion_sources: Vec<AssertionSource>,
    /// Exact mutation class required from the receipt, when specified.
    #[serde(default)]
    pub mutation: Option<crate::evidence::Mutation>,
    /// When true, the satisfying receipt must come from a clean tree.
    #[serde(default)]
    pub require_clean: bool,
    /// Status used when no satisfying receipt, or a required assertion on
    /// it, exists in the store.
    #[serde(default)]
    pub missing_as: MissingAs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssertionSource {
    pub id: String,
    pub source: String,
}

/// Status reported when required proof is absent from the store.
/// BLOCKED names a known prerequisite that prevents the run from proceeding
/// legally or procedurally, for example the owner trust chain.
/// INCOMPLETE names proof that is simply absent so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissingAs {
    #[serde(rename = "BLOCKED")]
    Blocked,
    #[serde(rename = "INCOMPLETE")]
    Incomplete,
}

impl Default for MissingAs {
    fn default() -> Self {
        MissingAs::Blocked
    }
}

impl MissingAs {
    fn verdict(self) -> Verdict {
        match self {
            MissingAs::Blocked => Verdict::Blocked,
            MissingAs::Incomplete => Verdict::Incomplete,
        }
    }
}

fn default_tier() -> Tier {
    Tier::HostTest
}

/// A named gate: the set of prerequisites that close it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    pub gate: String,
    #[serde(default)]
    pub description: String,
    pub requires: Vec<Requirement>,
}

impl Gate {
    /// Reject malformed manifests at load: unnamed or duplicated
    /// requirements, requirements naming both or neither of gate and kind,
    /// and source constraints on assertions the manifest never requires.
    pub fn validate(&self) -> Result<(), String> {
        if self.gate.trim().is_empty() {
            return Err("gate manifest has no gate name".to_string());
        }
        let mut names = BTreeSet::new();
        for req in &self.requires {
            if req.name.trim().is_empty() {
                return Err(format!("gate {} has an unnamed requirement", self.gate));
            }
            if !names.insert(req.name.clone()) {
                return Err(format!(
                    "gate {} lists requirement {:?} twice",
                    self.gate, req.name
                ));
            }
            match (&req.gate_ref, req.kind.trim().is_empty()) {
                (Some(_), false) => {
                    return Err(format!(
                        "gate {} requirement {:?} names both a gate and a receipt kind",
                        self.gate, req.name
                    ));
                }
                (None, true) => {
                    return Err(format!(
                        "gate {} requirement {:?} names neither a gate nor a receipt kind",
                        self.gate, req.name
                    ));
                }
                _ => {}
            }
            for constrained in &req.assertion_sources {
                if !req.assertions.iter().any(|a| a == &constrained.id) {
                    return Err(format!(
                        "gate {} requirement {:?} constrains source of assertion {:?} it never requires",
                        self.gate, req.name, constrained.id
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Per requirement outcome.
#[derive(Debug, Clone)]
pub struct GateLine {
    pub name: String,
    pub status: Verdict,
    pub detail: String,
}

/// Whole gate outcome plus the responsible prerequisite lines.
#[derive(Debug, Clone)]
pub struct GateReport {
    pub gate: String,
    pub status: Verdict,
    pub lines: Vec<GateLine>,
}

fn worse(a: Verdict, b: Verdict) -> Verdict {
    fn rank(v: Verdict) -> u8 {
        match v {
            Verdict::Fail => 0,
            Verdict::Blocked => 1,
            Verdict::Incomplete => 2,
            Verdict::Skipped => 3,
            Verdict::Pass => 4,
        }
    }
    if rank(a) <= rank(b) {
        a
    } else {
        b
    }
}

/// Pick the best stored receipt of `kind`: PASS first, then highest tier.
pub fn best_candidate(store: &Path, kind: &str) -> Option<crate::evidence::Receipt> {
    let mut best: Option<crate::evidence::Receipt> = None;
    for id in list_receipts(store) {
        let receipt = load_receipt(store, &id).ok()?;
        if receipt.kind != kind {
            continue;
        }
        let take = match &best {
            None => true,
            Some(current) => {
                let rank = |r: &crate::evidence::Receipt| {
                    (
                        r.result == Verdict::Pass,
                        r.tier.rank(),
                        r.timestamp,
                    )
                };
                rank(&receipt) > rank(current)
            }
        };
        if take {
            best = Some(receipt);
        }
    }
    best
}

/// Evaluate one requirement against the store.
fn eval_requirement(store: &Path, req: &Requirement, stack: &mut Vec<String>) -> GateLine {
    if let Some(child_name) = &req.gate_ref {
        if !req.kind.is_empty() {
            return GateLine { name: req.name.clone(), status: Verdict::Fail, detail: "manifest requirement cannot name both a gate and receipt kind".into() };
        }
        if stack.contains(child_name) {
            return GateLine { name: req.name.clone(), status: Verdict::Fail, detail: format!("gate dependency cycle through {child_name}") };
        }
        let child = match load_manifest(store, child_name, None) {
            Ok(g) => g,
            Err(e) => return GateLine { name: req.name.clone(), status: Verdict::Blocked, detail: format!("BLOCKED {child_name}: {e}") },
        };
        stack.push(child_name.clone());
        let report = evaluate_inner(&child, store, stack);
        stack.pop();
        return GateLine { name: req.name.clone(), status: report.status, detail: format!("{}: {}", child_name, report.status.as_str()) };
    }
    if req.kind.is_empty() {
        return GateLine { name: req.name.clone(), status: Verdict::Fail, detail: "manifest requirement has neither gate_ref nor receipt kind".into() };
    }
    let receipt = match best_candidate(store, &req.kind) {
        None => {
            return GateLine {
                name: req.name.clone(),
                status: req.missing_as.verdict(),
                detail: format!("MISSING: no receipt of kind {:?} in store", req.kind),
            };
        }
        Some(r) => r,
    };
    if receipt.result != Verdict::Pass {
        return GateLine {
            name: req.name.clone(),
            status: if receipt.result == Verdict::Fail {
                Verdict::Fail
            } else {
                Verdict::Blocked
            },
            detail: format!(
                "{}: receipt {} records {}",
                req.kind,
                &receipt.id[..16],
                receipt.result.as_str()
            ),
        };
    }
    if !tier_satisfies(req.min_tier, receipt.tier) {
        return GateLine {
            name: req.name.clone(),
            status: Verdict::Blocked,
            detail: format!(
                "{}: receipt tier {} cannot satisfy required {}",
                req.kind,
                receipt.tier.as_str(),
                req.min_tier.as_str()
            ),
        };
    }
    if req.require_clean && receipt.dirty {
        return GateLine {
            name: req.name.clone(),
            status: Verdict::Blocked,
            detail: format!(
                "{}: gate requires a clean source tree but receipt {} is dirty",
                req.kind,
                &receipt.id[..16]
            ),
        };
    }
    for wanted in &req.assertions {
        match receipt.assertions.iter().find(|a| &a.id == wanted) {
            Some(a) if a.pass => {}
            Some(_) => {
                return GateLine {
                    name: req.name.clone(),
                    status: Verdict::Fail,
                    detail: format!("{}: assertion {wanted:?} failed", req.kind),
                };
            }
            None => {
                return GateLine {
                    name: req.name.clone(),
                    status: req.missing_as.verdict(),
                    detail: format!("{}: assertion {wanted:?} missing", req.kind),
                };
            }
        }
    }
    for wanted in &req.assertion_sources {
        match receipt.assertions.iter().find(|a| a.id == wanted.id) {
            Some(a) if a.pass && a.source == wanted.source => {}
            Some(a) if !a.pass => return GateLine { name: req.name.clone(), status: Verdict::Fail, detail: format!("{}: assertion {:?} failed", req.kind, wanted.id) },
            Some(a) => return GateLine { name: req.name.clone(), status: Verdict::Fail, detail: format!("{}: assertion {:?} source {:?} does not match required {:?}", req.kind, wanted.id, a.source, wanted.source) },
            None => return GateLine { name: req.name.clone(), status: req.missing_as.verdict(), detail: format!("{}: assertion {:?} missing", req.kind, wanted.id) },
        }
    }
    if let Some(mutation) = req.mutation {
        if receipt.declared_mutation != mutation || receipt.observed_mutation != mutation {
            return GateLine { name: req.name.clone(), status: Verdict::Fail, detail: format!("{}: declared/observed mutation must both be {}", req.kind, mutation.as_str()) };
        }
    }
    if receipt.tier.is_hardware() {
        match crate::lease::lease_status(&receipt, store) {
            Some(r) if r.status != Verdict::Pass => {
                return GateLine {
                    name: req.name.clone(),
                    status: if r.status == Verdict::Fail {
                        Verdict::Fail
                    } else {
                        Verdict::Blocked
                    },
                    detail: format!("{}: {}", req.kind, r.reasons.join("; ")),
                };
            }
            _ => {}
        }
    }
    GateLine {
        name: req.name.clone(),
        status: Verdict::Pass,
        detail: format!(
            "{}: receipt {} tier {}",
            req.kind,
            &receipt.id[..16],
            receipt.tier.as_str()
        ),
    }
}

/// Evaluate a gate manifest against the store.
pub fn evaluate(manifest: &Gate, store: &Path) -> GateReport {
    evaluate_inner(manifest, store, &mut vec![manifest.gate.clone()])
}

fn evaluate_inner(manifest: &Gate, store: &Path, stack: &mut Vec<String>) -> GateReport {
    let mut status = Verdict::Pass;
    let mut lines = Vec::new();
    for req in &manifest.requires {
        let line = eval_requirement(store, req, stack);
        status = worse(status, line.status);
        lines.push(line);
    }
    // No requirements means nothing is proven.
    if manifest.requires.is_empty() {
        status = Verdict::Incomplete;
    }
    GateReport {
        gate: manifest.gate.clone(),
        status,
        lines,
    }
}

/// Load a manifest by gate name: explicit path first, then the store's
/// gate directory.
pub fn load_manifest(store: &Path, gate: &str, explicit: Option<&Path>) -> Result<Gate, String> {
    if let Some(path) = explicit {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("cannot read gate file: {e}"))?;
        let manifest: Gate = serde_json::from_str(&text).map_err(|e| format!("gate manifest invalid: {e}"))?;
        return (manifest.gate == gate).then_some(manifest).ok_or_else(|| format!("gate manifest name {:?} does not match requested {gate:?}", manifest.gate));
    }
    let path = store.join(GATE_DIR).join(format!("{gate}.json"));
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => embedded_manifest(gate).ok_or_else(|| format!("no manifest for gate {gate:?}: pass --gate <file>"))?.to_string(),
    };
    let manifest: Gate = serde_json::from_str(&text).map_err(|e| format!("gate manifest invalid: {e}"))?;
    (manifest.gate == gate).then_some(manifest).ok_or_else(|| format!("gate manifest name {:?} does not match requested {gate:?}", manifest.gate))
}

/// Built in example manifests. These are examples of the manifest shape,
/// not claims that any gate has passed.
pub const SEED_0B_EXAMPLE: &str = include_str!("../gates/SEED-0B.json");
pub const SEED_0B_QEMU: &str = include_str!("../gates/SEED0B_QEMU.json");
pub const P3_DEVELOPMENT_QEMU_ENTRY: &str = include_str!("../gates/P3_DEVELOPMENT_QEMU_ENTRY.json");
pub const P3_MACHINE1_WRITE_ENTRY: &str = include_str!("../gates/P3_MACHINE1_WRITE_ENTRY.json");

fn embedded_manifest(gate: &str) -> Option<&'static str> {
    match gate {
        "SEED-0B" => Some(SEED_0B_EXAMPLE),
        "SEED0B_QEMU" => Some(SEED_0B_QEMU),
        "P3_DEVELOPMENT_QEMU_ENTRY" => Some(P3_DEVELOPMENT_QEMU_ENTRY),
        "P3_MACHINE1_WRITE_ENTRY" => Some(P3_MACHINE1_WRITE_ENTRY),
        _ => None,
    }
}

/// Materialize the example manifests into a directory.
pub fn write_examples(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (name, text) in [
        ("SEED-0B.json", SEED_0B_EXAMPLE),
        ("SEED0B_QEMU.json", SEED_0B_QEMU),
        ("P3_DEVELOPMENT_QEMU_ENTRY.json", P3_DEVELOPMENT_QEMU_ENTRY),
        ("P3_MACHINE1_WRITE_ENTRY.json", P3_MACHINE1_WRITE_ENTRY),
    ] {
        // Validate the shipped shape before writing.
        let _: Gate =
            serde_json::from_str(text).map_err(|e| format!("built in manifest broken: {e}"))?;
        let path = dir.join(name);
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
        out.push(path);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::fixtures::{sample, sealed};
    use crate::evidence::store_receipt;

    fn store_qemu(store: &Path, kind: &str, assertions: Vec<&str>) {
        let mut r = sample();
        r.kind = kind.to_string();
        let ids = if assertions.is_empty() {
            vec!["recorded"]
        } else {
            assertions
        };
        r.assertions = ids
            .into_iter()
            .map(|id| crate::evidence::Assertion {
                id: id.to_string(),
                expected: "yes".to_string(),
                observed: "yes".to_string(),
                pass: true,
                source: "example".to_string(),
                note: String::new(),
            })
            .collect();
        store_receipt(store, &sealed(r)).unwrap();
    }

    fn manifest() -> Gate {
        Gate {
            gate: "SEED-0B".to_string(),
            description: "example".to_string(),
            requires: vec![
                Requirement {
                    name: "M3".to_string(),
                    kind: "m3".to_string(),
                    gate_ref: None,
                    min_tier: Tier::HostTest,
                    assertions: vec![],
                    assertion_sources: vec![],
                    mutation: None,
                    require_clean: false,
                },
                Requirement {
                    name: "SEED-0B-QEMU".to_string(),
                    kind: "seed-0b-qemu".to_string(),
                    gate_ref: None,
                    min_tier: Tier::Qemu,
                    assertions: vec!["artifact_signature_valid".to_string()],
                    assertion_sources: vec![],
                    mutation: None,
                    require_clean: true,
                },
                Requirement {
                    name: "SEED-0B-MACHINE1".to_string(),
                    kind: "seed-0b-machine1".to_string(),
                    gate_ref: None,
                    min_tier: Tier::Machine1Attended,
                    assertions: vec![],
                    assertion_sources: vec![],
                    mutation: None,
                    require_clean: true,
                },
            ],
        }
    }

    #[test]
    fn gate_reports_missing() {
        let store = crate::testutil::temp_dir("gate-missing");
        let report = evaluate(&manifest(), &store);
        assert_eq!(report.status, Verdict::Blocked);
        assert!(report.lines.iter().all(|l| l.status == Verdict::Blocked));
        assert!(report.lines[0].detail.contains("MISSING"));
    }

    #[test]
    fn qemu_receipt_cannot_satisfy_machine1_requirement() {
        let store = crate::testutil::temp_dir("gate-tier");
        store_qemu(&store, "m3", vec![]);
        store_qemu(&store, "seed-0b-qemu", vec!["artifact_signature_valid"]);
        // A QEMU tier receipt filed under the machine1 kind must not close
        // the physical requirement.
        let mut r = sample();
        r.kind = "seed-0b-machine1".to_string();
        store_receipt(&store, &sealed(r)).unwrap();
        let report = evaluate(&manifest(), &store);
        assert_eq!(report.status, Verdict::Blocked);
        let line = report.lines.iter().find(|l| l.name == "SEED-0B-MACHINE1").unwrap();
        assert!(line.detail.contains("cannot satisfy"));
    }

    #[test]
    fn dirty_tree_fails_clean_requirement() {
        let store = crate::testutil::temp_dir("gate-dirty");
        let mut r = sample();
        r.kind = "m3".to_string();
        store_receipt(&store, &sealed(r)).unwrap();
        let mut q = sample();
        q.kind = "seed-0b-qemu".to_string();
        q.dirty = true;
        store_receipt(&store, &sealed(q)).unwrap();
        let report = evaluate(&manifest(), &store);
        let line = report.lines.iter().find(|l| l.name == "SEED-0B-QEMU").unwrap();
        assert_eq!(line.status, Verdict::Blocked);
        assert!(line.detail.contains("clean source tree"));
    }

    #[test]
    fn failed_assertion_fails_gate() {
        let store = crate::testutil::temp_dir("gate-assert");
        store_qemu(&store, "m3", vec![]);
        let mut q = sample();
        q.kind = "seed-0b-qemu".to_string();
        q.assertions[0].pass = false;
        store_receipt(&store, &sealed(q)).unwrap();
        let report = evaluate(&manifest(), &store);
        let line = report.lines.iter().find(|l| l.name == "SEED-0B-QEMU").unwrap();
        assert_eq!(line.status, Verdict::Fail);
    }

    #[test]
    fn shipped_examples_parse() {
        let seed: Gate = serde_json::from_str(SEED_0B_EXAMPLE).unwrap();
        assert_eq!(seed.gate, "SEED-0B");
        assert!(!seed.requires.is_empty());
        let seed_qemu: Gate = serde_json::from_str(SEED_0B_QEMU).unwrap();
        assert_eq!(seed_qemu.gate, "SEED0B_QEMU");
        let p3: Gate = serde_json::from_str(P3_DEVELOPMENT_QEMU_ENTRY).unwrap();
        assert_eq!(p3.gate, "P3_DEVELOPMENT_QEMU_ENTRY");
        assert!(!p3.requires.is_empty());
        let physical: Gate = serde_json::from_str(P3_MACHINE1_WRITE_ENTRY).unwrap();
        assert_eq!(physical.gate, "P3_MACHINE1_WRITE_ENTRY");
    }
}

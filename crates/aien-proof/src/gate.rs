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
    /// Minimum qualification tier. A QEMU receipt never satisfies a
    /// Machine 1 requirement.
    #[serde(default = "default_tier")]
    pub min_tier: Tier,
    /// Assertion IDs that must exist and pass on the chosen receipt.
    #[serde(default)]
    pub assertions: Vec<String>,
    /// When true, the satisfying receipt must come from a clean tree.
    #[serde(default)]
    pub require_clean: bool,
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
fn eval_requirement(store: &Path, req: &Requirement) -> GateLine {
    let receipt = match best_candidate(store, &req.kind) {
        None => {
            return GateLine {
                name: req.name.clone(),
                status: Verdict::Blocked,
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
                    status: Verdict::Blocked,
                    detail: format!("{}: assertion {wanted:?} missing", req.kind),
                };
            }
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
    let mut status = Verdict::Pass;
    let mut lines = Vec::new();
    for req in &manifest.requires {
        let line = eval_requirement(store, req);
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
        return serde_json::from_str(&text).map_err(|e| format!("gate manifest invalid: {e}"));
    }
    let path = store.join(GATE_DIR).join(format!("{gate}.json"));
    let text = std::fs::read_to_string(&path)
        .map_err(|_| format!("no manifest for gate {gate:?}: pass --gate <file>"))?;
    serde_json::from_str(&text).map_err(|e| format!("gate manifest invalid: {e}"))
}

/// Built in example manifests. These are examples of the manifest shape,
/// not claims that any gate has passed.
pub const SEED_0B_EXAMPLE: &str = include_str!("../gates/SEED-0B.json");
pub const P3_ENTRY_EXAMPLE: &str = include_str!("../gates/P3_ENTRY.json");

/// Materialize the example manifests into a directory.
pub fn write_examples(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (name, text) in [("SEED-0B.json", SEED_0B_EXAMPLE), ("P3_ENTRY.json", P3_ENTRY_EXAMPLE)] {
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
                    min_tier: Tier::HostTest,
                    assertions: vec![],
                    require_clean: false,
                },
                Requirement {
                    name: "SEED-0B-QEMU".to_string(),
                    kind: "seed-0b-qemu".to_string(),
                    min_tier: Tier::Qemu,
                    assertions: vec!["artifact_signature_valid".to_string()],
                    require_clean: true,
                },
                Requirement {
                    name: "SEED-0B-MACHINE1".to_string(),
                    kind: "seed-0b-machine1".to_string(),
                    min_tier: Tier::Machine1Attended,
                    assertions: vec![],
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
        let p3: Gate = serde_json::from_str(P3_ENTRY_EXAMPLE).unwrap();
        assert_eq!(p3.gate, "P3_ENTRY");
        assert!(!p3.requires.is_empty());
    }
}

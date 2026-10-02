//! Verdict set and derivation (ADR 0028 Decision 5). A test program only
//! reports observations; the verdict is derived here from the manifest's
//! expectations and facts the runner measured. Only the last row can yield
//! PASS.
//!
//! Slice A scope (host pool, serial, one gate): rows 2 to 15 are implemented
//! as below. Where Slice A cannot do what a row presumes (dependencies, other
//! pools, build, symbol checks, operator attestations) it refuses with NOT_RUN
//! or BLOCKED_OPERATOR and a reason code; it never guesses a PASS.
//!
//! Deviation from the ADR table order (recorded): timeout (row 14) is tested
//! before the exit status (row 10), because a killed process has no exit code
//! and would otherwise always be reported as rc_nonzero.

use crate::manifest::{parse_json_scalar, valid_obs_name, Manifest, Op, Quant, Rule};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
    NotRun,
    BlockedHardware,
    BlockedOperator,
    MissingImplementation,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Pass => "PASS",
            Verdict::Fail => "FAIL",
            Verdict::NotRun => "NOT_RUN",
            Verdict::BlockedHardware => "BLOCKED_HARDWARE",
            Verdict::BlockedOperator => "BLOCKED_OPERATOR",
            Verdict::MissingImplementation => "MISSING_IMPLEMENTATION",
        }
    }

    /// CLI exit code: 0 PASS, 1 FAIL, 2 NOT_RUN, 3 BLOCKED_HARDWARE,
    /// 4 BLOCKED_OPERATOR, 5 MISSING_IMPLEMENTATION.
    pub fn exit_code(self) -> i32 {
        match self {
            Verdict::Pass => 0,
            Verdict::Fail => 1,
            Verdict::NotRun => 2,
            Verdict::BlockedHardware => 3,
            Verdict::BlockedOperator => 4,
            Verdict::MissingImplementation => 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub name: String,
    pub value: Value,
}

/// Parse `AIEN-OBS <name> <json-value>` lines. Anything else, including a
/// malformed AIEN-OBS line (bad name, bad JSON, float), is ordinary log text.
pub fn parse_observations(stdout: &[u8]) -> Vec<Observation> {
    let text = String::from_utf8_lossy(stdout);
    let mut out = Vec::new();
    for line in text.lines() {
        let rest = match line.strip_prefix("AIEN-OBS ") {
            Some(r) => r,
            None => continue,
        };
        let (name, json) = match rest.split_once(' ') {
            Some(p) => p,
            None => continue,
        };
        if !valid_obs_name(name) {
            continue;
        }
        if let Some(value) = parse_json_scalar(json.trim()) {
            out.push(Observation {
                name: name.to_string(),
                value,
            });
        }
    }
    out
}

fn compare(op: Op, a: &Value, b: &Value) -> bool {
    match op {
        Op::Eq => a == b,
        Op::Ne => a != b,
        _ => match (a.as_i64(), b.as_i64()) {
            (Some(x), Some(y)) => match op {
                Op::Lt => x < y,
                Op::Le => x <= y,
                Op::Gt => x > y,
                _ => x >= y,
            },
            _ => false,
        },
    }
}

pub fn eval_rule(rule: &Rule, obs: &[Observation]) -> bool {
    let vals: Vec<&Value> = obs
        .iter()
        .filter(|o| o.name == rule.name)
        .map(|o| &o.value)
        .collect();
    if vals.is_empty() {
        return false;
    }
    match rule.quant {
        Quant::Last => compare(rule.op, vals[vals.len() - 1], &rule.value),
        Quant::All => vals.iter().all(|v| compare(rule.op, v, &rule.value)),
        Quant::Any => vals.iter().any(|v| compare(rule.op, v, &rule.value)),
    }
}

/// Facts the runner measured. For a gate that did not run, `exit_status` is
/// None and `observations` is empty.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// Refusal id decided before running (dirty_tree, evidence_inside, ...).
    pub refusal: Option<String>,
    pub exec_exists: bool,
    pub exit_status: Option<i64>,
    pub observations: Vec<Observation>,
    pub timed_out: bool,
    /// `git status --porcelain` differed after the run.
    pub tree_changed_after: bool,
    pub head_moved: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Derived {
    pub verdict: Verdict,
    pub reason: String,
    /// Each `expects.observe` rule and whether it held.
    pub rules: Vec<(String, bool)>,
}

/// Rows 2 to 7: decided before anything runs. Some(..) means do not run.
pub fn pre_run(
    m: &Manifest,
    refusal: &Option<String>,
    exec_exists: bool,
) -> Option<(Verdict, String)> {
    if m.requires.iter().any(|r| r == "operator") {
        // Slice A has no operator attestation files, so this is always pending.
        return Some((
            Verdict::BlockedOperator,
            "OPERATOR_STEP_PENDING".to_string(),
        ));
    }
    if let Some(d) = m.depends_on.first() {
        // Slice A has no dependency graph; it cannot show the dependency passed.
        return Some((Verdict::NotRun, format!("DEP_NOT_PASS:{d}")));
    }
    if m.pool() != "host" {
        return Some((Verdict::NotRun, format!("pool_unsupported:{}", m.pool())));
    }
    if let Some(b) = &m.build {
        if b.tool != "none" {
            return Some((Verdict::NotRun, "build_unsupported".to_string()));
        }
    }
    if let Some(c) = m.checks.iter().find(|c| c.as_str() != "clean_tree_after") {
        return Some((Verdict::NotRun, format!("check_unsupported:{c}")));
    }
    if let Some(r) = refusal {
        return Some((Verdict::NotRun, r.clone()));
    }
    if !exec_exists {
        return Some((Verdict::MissingImplementation, "NO_RUN_EXEC".to_string()));
    }
    None
}

pub fn derive(m: &Manifest, f: &Facts) -> Derived {
    if let Some((verdict, reason)) = pre_run(m, &f.refusal, f.exec_exists) {
        return Derived {
            verdict,
            reason,
            rules: m.rules.iter().map(|r| (r.text.clone(), false)).collect(),
        };
    }
    let rules: Vec<(String, bool)> = m
        .rules
        .iter()
        .map(|r| (r.text.clone(), eval_rule(r, &f.observations)))
        .collect();
    let (verdict, reason) = if f.timed_out {
        (Verdict::Fail, "timeout".to_string())
    } else if f.exit_status != Some(m.expect_exit) {
        (Verdict::Fail, "rc_nonzero".to_string())
    } else if let Some(idx) = rules.iter().position(|r| !r.1) {
        (Verdict::Fail, format!("rule:{idx}"))
    } else if f.tree_changed_after {
        (Verdict::Fail, "dirty_after".to_string())
    } else if f.head_moved {
        (Verdict::Fail, "head_moved".to_string())
    } else {
        (Verdict::Pass, String::new())
    };
    Derived {
        verdict,
        reason,
        rules,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;
    use serde_json::json;

    fn m(extra_replace: Option<(&str, &str)>) -> Manifest {
        let mut t = String::from(
            "gate: G\nmanifest_version: 1\nowner: o\nrequires:\n  - host\nrun:\n  exec: x.sh\nexpects:\n  exit: 0\n  observe: [\"n == 3\", \"all k <= 2\"]\n  verdict: PASS\ntimeout: 5s\n",
        );
        if let Some((a, b)) = extra_replace {
            assert!(t.contains(a), "replace target missing: {a}");
            t = t.replace(a, b);
        }
        manifest::parse(t.as_bytes()).unwrap()
    }

    fn obs(name: &str, v: Value) -> Observation {
        Observation {
            name: name.to_string(),
            value: v,
        }
    }

    fn good_facts() -> Facts {
        Facts {
            refusal: None,
            exec_exists: true,
            exit_status: Some(0),
            observations: vec![obs("n", json!(3)), obs("k", json!(1)), obs("k", json!(2))],
            timed_out: false,
            tree_changed_after: false,
            head_moved: false,
        }
    }

    #[test]
    fn exit_codes_and_names_are_fixed() {
        let all = [
            (Verdict::Pass, "PASS", 0),
            (Verdict::Fail, "FAIL", 1),
            (Verdict::NotRun, "NOT_RUN", 2),
            (Verdict::BlockedHardware, "BLOCKED_HARDWARE", 3),
            (Verdict::BlockedOperator, "BLOCKED_OPERATOR", 4),
            (Verdict::MissingImplementation, "MISSING_IMPLEMENTATION", 5),
        ];
        for (v, name, code) in all {
            assert_eq!(v.as_str(), name);
            assert_eq!(v.exit_code(), code);
        }
    }

    #[test]
    fn all_expectations_met_is_pass() {
        let d = derive(&m(None), &good_facts());
        assert_eq!(d.verdict, Verdict::Pass);
        assert_eq!(d.reason, "");
        assert!(d.rules.iter().all(|r| r.1));
    }

    #[test]
    fn rule_mismatch_is_fail_with_index() {
        let mut f = good_facts();
        f.observations[0] = obs("n", json!(4));
        let d = derive(&m(None), &f);
        assert_eq!(d.verdict, Verdict::Fail);
        assert_eq!(d.reason, "rule:0");
        assert!(!d.rules[0].1);
        assert!(d.rules[1].1);
    }

    #[test]
    fn printing_pass_cannot_make_a_pass() {
        let mut f = good_facts();
        f.observations = parse_observations(b"PASS\nAIEN-OBS verdict \"PASS\"\n");
        let d = derive(&m(None), &f);
        assert_eq!(d.verdict, Verdict::Fail);
    }

    #[test]
    fn missing_observation_does_not_hold() {
        let mut f = good_facts();
        f.observations.retain(|o| o.name != "n");
        assert_eq!(derive(&m(None), &f).reason, "rule:0");
    }

    #[test]
    fn exit_status_must_match_expectation() {
        let mut f = good_facts();
        f.exit_status = Some(1);
        let d = derive(&m(None), &f);
        assert_eq!(
            (d.verdict, d.reason.as_str()),
            (Verdict::Fail, "rc_nonzero")
        );
        f.exit_status = None;
        assert_eq!(derive(&m(None), &f).reason, "rc_nonzero");
        // A manifest that expects exit 7 passes on 7 and fails on 0.
        let m7 = m(Some(("exit: 0", "exit: 7")));
        f.exit_status = Some(7);
        assert_eq!(derive(&m7, &f).verdict, Verdict::Pass);
        f.exit_status = Some(0);
        assert_eq!(derive(&m7, &f).verdict, Verdict::Fail);
    }

    #[test]
    fn timeout_beats_everything_after_run() {
        let mut f = good_facts();
        f.timed_out = true;
        f.exit_status = None;
        let d = derive(&m(None), &f);
        assert_eq!((d.verdict, d.reason.as_str()), (Verdict::Fail, "timeout"));
    }

    #[test]
    fn tree_and_head_changes_fail() {
        let mut f = good_facts();
        f.tree_changed_after = true;
        assert_eq!(derive(&m(None), &f).reason, "dirty_after");
        f.tree_changed_after = false;
        f.head_moved = true;
        assert_eq!(derive(&m(None), &f).reason, "head_moved");
    }

    #[test]
    fn first_reason_wins() {
        let mut f = good_facts();
        f.exit_status = Some(9);
        f.observations.clear();
        f.tree_changed_after = true;
        assert_eq!(derive(&m(None), &f).reason, "rc_nonzero");
    }

    #[test]
    fn refusal_is_not_run() {
        let mut f = good_facts();
        f.refusal = Some("dirty_tree".to_string());
        let d = derive(&m(None), &f);
        assert_eq!(
            (d.verdict, d.reason.as_str()),
            (Verdict::NotRun, "dirty_tree")
        );
    }

    #[test]
    fn missing_exec_is_missing_implementation() {
        let mut f = good_facts();
        f.exec_exists = false;
        let d = derive(&m(None), &f);
        assert_eq!(d.verdict, Verdict::MissingImplementation);
        assert_eq!(d.reason, "NO_RUN_EXEC");
    }

    #[test]
    fn operator_gate_is_blocked_operator() {
        let mo = m(Some(("  - host\n", "  - host\n  - operator\n")));
        let d = derive(&mo, &good_facts());
        assert_eq!(d.verdict, Verdict::BlockedOperator);
        assert_eq!(d.reason, "OPERATOR_STEP_PENDING");
    }

    #[test]
    fn other_pools_dependencies_build_and_checks_are_not_run() {
        let g = m(Some(("  - host\n", "  - gb10\n")));
        assert_eq!(derive(&g, &good_facts()).reason, "pool_unsupported:gb10");
        let q = m(Some(("  - host\n", "  - qemu\n")));
        assert_eq!(derive(&q, &good_facts()).reason, "pool_unsupported:qemu");
        let d = m(Some((
            "timeout: 5s\n",
            "timeout: 5s\ndepends_on:\n  - OTHER\n",
        )));
        let r = derive(&d, &good_facts());
        assert_eq!(
            (r.verdict, r.reason.as_str()),
            (Verdict::NotRun, "DEP_NOT_PASS:OTHER")
        );
        let b = m(Some((
            "timeout: 5s\n",
            "timeout: 5s\nbuild:\n  target: t\n  tool: make\n",
        )));
        assert_eq!(derive(&b, &good_facts()).reason, "build_unsupported");
        let c = m(Some((
            "timeout: 5s\n",
            "timeout: 5s\nchecks:\n  - no_libm_symbols\n",
        )));
        assert_eq!(
            derive(&c, &good_facts()).reason,
            "check_unsupported:no_libm_symbols"
        );
    }

    #[test]
    fn build_none_and_clean_tree_check_are_fine() {
        let b = m(Some((
            "timeout: 5s\n",
            "timeout: 5s\nbuild:\n  target: t\n  tool: none\nchecks:\n  - clean_tree_after\n",
        )));
        assert_eq!(derive(&b, &good_facts()).verdict, Verdict::Pass);
    }

    #[test]
    fn observation_parsing() {
        let o = parse_observations(
            b"noise\nAIEN-OBS a 1\nAIEN-OBS b \"x y\"\nAIEN-OBS c 1.5\nAIEN-OBS BAD 1\nAIEN-OBS d\nAIEN-OBS a 2\nAIEN-OBS e true\nAIEN-OBS f nope\n",
        );
        let names: Vec<&str> = o.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "a", "e"]);
        assert_eq!(o[1].value, json!("x y"));
    }

    #[test]
    fn quantifiers_and_ordering() {
        let r = manifest::parse_rule("any k > 5").unwrap();
        let o = vec![obs("k", json!(1)), obs("k", json!(9))];
        assert!(eval_rule(&r, &o));
        let r = manifest::parse_rule("k > 5").unwrap();
        assert!(eval_rule(&r, &o), "last value 9 > 5");
        let r = manifest::parse_rule("all k > 5").unwrap();
        assert!(!eval_rule(&r, &o));
        let r = manifest::parse_rule("k < 5").unwrap();
        assert!(!eval_rule(&r, &o));
        // Ordering over strings never holds.
        let r = manifest::parse_rule("s < 5").unwrap();
        assert!(!eval_rule(&r, &[obs("s", json!("a"))]));
        let r = manifest::parse_rule("s != \"a\"").unwrap();
        assert!(eval_rule(&r, &[obs("s", json!("b"))]));
    }
}

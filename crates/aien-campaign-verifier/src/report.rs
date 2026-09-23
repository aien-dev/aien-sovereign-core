use serde::Serialize;
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Verdict {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub verdict: Verdict,
    pub violations: Vec<String>,
}

impl CheckResult {
    pub fn from_violations(name: impl Into<String>, violations: Vec<String>) -> Self {
        let verdict = if violations.is_empty() {
            Verdict::Pass
        } else {
            Verdict::Fail
        };
        Self {
            name: name.into(),
            verdict,
            violations,
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub checks: Vec<CheckResult>,
}

impl Report {
    pub fn push(&mut self, check: CheckResult) {
        self.checks.push(check);
    }

    pub fn passed(&self) -> bool {
        self.checks.iter().all(|c| c.verdict == Verdict::Pass)
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for check in &self.checks {
            let tag = match check.verdict {
                Verdict::Pass => "PASS",
                Verdict::Fail => "FAIL",
            };
            let _ = writeln!(out, "{tag}  {}", check.name);
            for violation in &check.violations {
                let _ = writeln!(out, "      {violation}");
            }
        }
        out
    }
}

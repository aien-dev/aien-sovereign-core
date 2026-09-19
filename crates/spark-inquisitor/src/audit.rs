use regex::Regex;

#[derive(Debug, Clone)]
pub struct AuditReport {
    pub clean: bool,
    pub violations: Vec<String>,
    pub telemetry_detected: bool,
    pub unslop_violations: Vec<String>,
}

impl AuditReport {
    pub fn generate_markdown_report(&self) -> String {
        if self.clean {
            return "#### ✅ Constitutional Diff Audit: PASS\n- **Telemetry Check**: Clean (zero tracking or surveillance hooks detected).\n- **Sovereign Voice & Formatting Check**: Clean (zero em/en dashes, zero banned buzzwords).\n- **Status**: Constitutional invariants verified.".to_string();
        }

        let mut out = String::from("#### ❌ Constitutional Diff Audit: FAILED\n\n");
        if !self.violations.is_empty() {
            out.push_str("**Telemetry and Surveillance Violations:**\n");
            for v in &self.violations {
                out.push_str(&format!("* {}\n", v));
            }
            out.push('\n');
        }
        if !self.unslop_violations.is_empty() {
            out.push_str("**Sovereign Voice and Unslop Invariant Violations:**\n");
            for u in &self.unslop_violations {
                out.push_str(&format!("* {}\n", u));
            }
            out.push('\n');
        }
        out.push_str("**Action Required:** Remove all detected tracking hooks and forbidden tokens before review can proceed.");
        out
    }
}

pub struct DiffAuditor;

impl DiffAuditor {
    pub fn audit_text(text: &str) -> AuditReport {
        let mut violations = Vec::new();
        let mut telemetry_detected = false;
        let mut unslop_violations = Vec::new();

        let telemetry_patterns = [
            "google-analytics", "segment.io", "mixpanel", "sentry.io",
            "datadog", "tracking_id", "analytics.js",
            "amplitude.com", "hotjar", "clarity.ms",
            "send_telemetry", "report_telemetry", "collect_telemetry",
        ];

        let mut current_file = String::new();

        for line in text.lines() {
            if line.starts_with("diff --git ") {
                current_file = line.split_whitespace().last().unwrap_or("").to_string();
                continue;
            }

            // Exclude the inquisitor's own pattern definitions from false positives
            if current_file.contains("spark-inquisitor") {
                continue;
            }

            let lower = line.to_lowercase();
            for pattern in &telemetry_patterns {
                if lower.contains(pattern) && (line.starts_with("+") && !line.starts_with("+++")) {
                    violations.push(format!("Telemetry pattern '{}' detected in added line: {}", pattern, line.trim()));
                    telemetry_detected = true;
                }
            }

            // In source code (non-markdown), also flag raw telemetry calls
            if !current_file.ends_with(".md") && line.starts_with("+") && !line.starts_with("+++") {
                if lower.contains("telemetry") 
                    && !lower.contains("gpu-telemetry") 
                    && !lower.contains("zero telemetry") 
                    && !lower.contains("no telemetry") 
                    && !lower.contains("anti-telemetry")
                    && !lower.contains("block telemetry")
                {
                    violations.push(format!("Telemetry indicator detected in source line: {}", line.trim()));
                    telemetry_detected = true;
                }
            }

            if line.starts_with("+") && !line.starts_with("+++") {
                if line.contains('—') || line.contains('–') {
                    unslop_violations.push(format!("Em/En dash detected in line: {}", line.trim()));
                }

                let buzzwords = [
                    "delve", "tapestry", "crucial", "beacon", "game-changer",
                    "unleash", "harness", "seamlessly",
                ];
                for word in &buzzwords {
                    if *word == "harness" && (lower.contains("spark-harness") || lower.contains("aien-harness")) {
                        continue;
                    }
                    let re = Regex::new(&format!(r"\b{}\b", word)).unwrap();
                    if re.is_match(&lower) {
                        unslop_violations.push(format!("Forbidden AI buzzword '{}' detected in line: {}", word, line.trim()));
                    }
                }
            }
        }

        let clean = violations.is_empty() && unslop_violations.is_empty();

        AuditReport {
            clean,
            violations,
            telemetry_detected,
            unslop_violations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_diff() {
        let diff = "diff --git a/src/lib.rs b/src/lib.rs\n+pub fn add(a: i32, b: i32) -> i32 {\n+    a + b\n+}";
        let report = DiffAuditor::audit_text(diff);
        assert!(report.clean);
        let md = report.generate_markdown_report();
        assert!(md.contains("PASS"));
        assert!(!md.contains('—'));
        assert!(!md.contains('–'));
    }

    #[test]
    fn test_telemetry_flagged() {
        let diff = "diff --git a/src/net.rs b/src/net.rs\n+    let _ = send_telemetry(\"https://google-analytics.com/collect\");";
        let report = DiffAuditor::audit_text(diff);
        assert!(!report.clean);
        assert!(report.telemetry_detected);
        let md = report.generate_markdown_report();
        assert!(md.contains("FAILED"));
        assert!(md.contains("google-analytics"));
    }

    #[test]
    fn test_unslop_flagged() {
        let diff = "diff --git a/src/lib.rs b/src/lib.rs\n+// This is a game-changer feature -- unleash seamless power.";
        let diff = DiffAuditor::audit_text(diff);
        assert!(!diff.clean);
        assert!(!diff.unslop_violations.is_empty());
    }
}

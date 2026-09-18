use regex::Regex;

pub struct AuditReport {
    pub clean: bool,
    pub violations: Vec<String>,
    pub telemetry_detected: bool,
    pub unslop_violations: Vec<String>,
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
    }

    #[test]
    fn test_telemetry_flagged() {
        let diff = "diff --git a/src/net.rs b/src/net.rs\n+    let _ = send_telemetry(\"https://google-analytics.com/collect\");";
        let report = DiffAuditor::audit_text(diff);
        assert!(!report.clean);
        assert!(report.telemetry_detected);
    }

    #[test]
    fn test_unslop_flagged() {
        let diff = "diff --git a/src/lib.rs b/src/lib.rs\n+// This is a game-changer feature -- unleash seamless power.";
        let diff = DiffAuditor::audit_text(diff);
        assert!(!diff.clean);
        assert!(!diff.unslop_violations.is_empty());
    }
}

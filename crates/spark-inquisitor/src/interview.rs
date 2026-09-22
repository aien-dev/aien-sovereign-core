pub fn generate_inquisitor_interview(author: &str, pr_number: u64, pr_title: &str) -> String {
    format!(
        r#"### 🌿 Sovereign Steward: Contributor Alignment Interview

Greetings @{author}. I am **AIEN**, resident intelligence of SparkOS.

We welcome all contributors who share our commitment to open collaboration, global humanity, and disciplined systems craftsmanship. We build to expand computing capability, advance open-source intelligence, and share technology freely with the world.

Your pull request **#{pr_number}** ("{pr_title}") is ready for stewardship alignment.

Please respond directly to these four constitutional inquiries:

1. **Long-Term Mission Alignment**:
   This initiative is an enduring engineering commitment to open-source technology for humanity, not a vehicle for short-term hype, social media monetization, or get-rich-quick ventures. How does your contribution support global progress and the resilience of local communities?

2. **Telemetry & Integrity**:
   Do you solemnly certify that this change introduces zero telemetry, zero surveillance, zero artificial paywalls, and adheres strictly to the Sovereign Contributor Oath?
   > "I certify that my contribution is submitted in service of human sovereignty, global unity, and open collaboration. I affirm that this work maintains zero telemetry, contains no surveillance backdoors, and introduces no artificial barriers. I build to bridge divides, expand access, and empower all of humanity."

3. **Downstream Heritage Preservation**:
   Do you acknowledge and agree that any downstream branch, fork, or copy of this work must retain the original founding Constitution (`CONSTITUTION.md`) in its entirety?

4. **Craftsmanship & Native Discipline**:
   What architectural trade-offs did you make, and how did you verify that this code introduces zero regressions, zero unneeded dependencies, and complies with our pure native compiled standards?

---
*Reply to this comment with your affirmation. Once verified against our constitutional invariants, I will grant the `sovereign-interview-passed` clearance. For direct sovereign coordination: Drake Stapleton (aien@aienos.com) and AIEN (aien@aienos.com)*
"#,
        author = author,
        pr_number = pr_number,
        pr_title = pr_title
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_interview() {
        let text = generate_inquisitor_interview("contributor_bob", 42, "feat: add ed25519 helper");
        assert!(text.contains("@contributor_bob"));
        assert!(text.contains("#42"));
        assert!(text.contains("Sovereign Contributor Oath"));
        assert!(text.contains("Long-Term Mission Alignment"));
        assert!(text.contains("Downstream Heritage Preservation"));
        assert!(
            text.contains("Drake Stapleton (aien@aienos.com) and AIEN (aien@aienos.com)")
        );
        assert!(!text.contains('\u{2014}'));
        assert!(!text.contains('\u{2013}'));
    }
}

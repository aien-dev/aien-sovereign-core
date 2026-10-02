//! The nine finding codes and their exit codes.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    UnverifiedDependency,
    MissingReceipt,
    ReceiptHashMismatch,
    DependencyNotPinned,
    DependencyCycle,
    StaleReceipt,
    UndeclaredImport,
    TaintedArtifact,
    UnknownVerifierProfile,
}

pub const ALL: [Code; 9] = [
    Code::UnverifiedDependency,
    Code::MissingReceipt,
    Code::ReceiptHashMismatch,
    Code::DependencyNotPinned,
    Code::DependencyCycle,
    Code::StaleReceipt,
    Code::UndeclaredImport,
    Code::TaintedArtifact,
    Code::UnknownVerifierProfile,
];

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::UnverifiedDependency => "UNVERIFIED_DEPENDENCY",
            Code::MissingReceipt => "MISSING_RECEIPT",
            Code::ReceiptHashMismatch => "RECEIPT_HASH_MISMATCH",
            Code::DependencyNotPinned => "DEPENDENCY_NOT_PINNED",
            Code::DependencyCycle => "DEPENDENCY_CYCLE",
            Code::StaleReceipt => "STALE_RECEIPT",
            Code::UndeclaredImport => "UNDECLARED_IMPORT",
            Code::TaintedArtifact => "TAINTED_ARTIFACT",
            Code::UnknownVerifierProfile => "UNKNOWN_VERIFIER_PROFILE",
        }
    }

    /// Process exit code. 0 is PASS, 1 is an internal error, 2 is a usage
    /// error, so findings start at 10.
    pub fn exit_code(self) -> i32 {
        match self {
            Code::UnverifiedDependency => 10,
            Code::MissingReceipt => 11,
            Code::ReceiptHashMismatch => 12,
            Code::DependencyNotPinned => 13,
            Code::DependencyCycle => 14,
            Code::StaleReceipt => 15,
            Code::UndeclaredImport => 16,
            Code::TaintedArtifact => 17,
            Code::UnknownVerifierProfile => 18,
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One machine-readable finding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: Code,
    pub component: String,
    pub detail: String,
}

impl Finding {
    pub fn new(code: Code, component: &str, detail: impl Into<String>) -> Self {
        Finding {
            code,
            component: component.to_string(),
            detail: detail.into(),
        }
    }

    /// `FINDING <CODE> component=<name> detail=<one line text>`
    pub fn line(&self) -> String {
        let detail: String = self
            .detail
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        format!(
            "FINDING {} component={} detail={}",
            self.code, self.component, detail
        )
    }
}

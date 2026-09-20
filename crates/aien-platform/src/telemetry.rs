use crate::PlatformError;
use alloc::string::String;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetrySnapshot {
    pub architecture: String,
    pub target: String,
    pub clock_ticks: u64,
    pub memory_used_bytes: usize,
    pub memory_total_bytes: usize,
    pub active_sequences: usize,
}

pub trait TelemetrySource {
    fn snapshot(&self) -> Result<TelemetrySnapshot, PlatformError>;
}

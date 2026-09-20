//! Telemetry source for Linux and DGX Spark GB10.

use aien_platform::{PlatformError, TelemetrySnapshot, TelemetrySource};
use std::fs;

pub struct LinuxTelemetrySource;

impl LinuxTelemetrySource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinuxTelemetrySource {
    fn default() -> Self {
        Self::new()
    }
}

impl TelemetrySource for LinuxTelemetrySource {
    fn snapshot(&self) -> Result<TelemetrySnapshot, PlatformError> {
        let mut total_mem = 0usize;
        let mut avail_mem = 0usize;

        if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
            for line in meminfo.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(val) = line.split_whitespace().nth(1) {
                        total_mem = val.parse::<usize>().unwrap_or(0) * 1024;
                    }
                } else if line.starts_with("MemAvailable:") {
                    if let Some(val) = line.split_whitespace().nth(1) {
                        avail_mem = val.parse::<usize>().unwrap_or(0) * 1024;
                    }
                }
            }
        }

        let used_mem = total_mem.saturating_sub(avail_mem);
        let target = if cfg!(target_arch = "aarch64") {
            "gb10"
        } else {
            "x86_64-linux"
        };

        Ok(TelemetrySnapshot {
            architecture: std::env::consts::ARCH.to_string(),
            target: target.to_string(),
            clock_ticks: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            memory_used_bytes: used_mem,
            memory_total_bytes: total_mem,
            active_sequences: 0,
        })
    }
}

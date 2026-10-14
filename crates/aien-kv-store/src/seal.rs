//! Encryption at rest of host record files (ADR 0003 section 19). RED STUB:
//! the API surface only, so the tests compile and fail.

use core::sync::atomic::{AtomicU64, Ordering};

pub const KEY_ENV: &str = "AIEN_KV_STORE_KEY";
pub const KEY_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRefusal {
    Missing,
    Malformed,
}

impl KeyRefusal {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Missing => "key_missing",
            Self::Malformed => "key_malformed",
        }
    }
}

impl core::fmt::Display for KeyRefusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.reason())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealError {
    Short,
    NotSealed,
    UnsupportedVersion,
    Rejected,
    Random,
}

pub struct SealKey {
    bytes: [u8; KEY_LEN],
}

impl SealKey {
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self { bytes }
    }

    pub fn generate() -> Result<Self, SealError> {
        Ok(Self::from_bytes([0; KEY_LEN]))
    }

    pub fn from_hex(_hex: &str) -> Result<Self, KeyRefusal> {
        Ok(Self::from_bytes([0; KEY_LEN]))
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, KeyRefusal> {
        Self::from_hex(&get(KEY_ENV).unwrap_or_default())
    }

    pub fn take_from_process_env() -> Result<Self, KeyRefusal> {
        Self::from_lookup(|n| std::env::var(n).ok())
    }

    pub fn seal_file(&self, _file: &mut [u8]) -> Result<(), SealError> {
        let _ = self.bytes;
        Ok(())
    }

    pub fn open_payload(&self, _region: &[u8], _payload: &mut [u8]) -> Result<(), SealError> {
        Ok(())
    }
}

impl core::fmt::Debug for SealKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SealKey({:?})", self.bytes)
    }
}

impl PartialEq for SealKey {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for SealKey {}

pub fn is_sealed_region(_region: &[u8]) -> bool {
    true
}

static KEY_REFUSALS: AtomicU64 = AtomicU64::new(0);
static OPEN_REJECTED: AtomicU64 = AtomicU64::new(0);

pub fn note_key_refusal() {
    KEY_REFUSALS.fetch_add(1, Ordering::Relaxed);
}

pub fn key_refusals_total() -> u64 {
    KEY_REFUSALS.load(Ordering::Relaxed)
}

pub fn open_rejected_total() -> u64 {
    OPEN_REJECTED.load(Ordering::Relaxed)
}

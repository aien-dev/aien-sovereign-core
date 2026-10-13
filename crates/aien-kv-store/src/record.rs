//! The 256-byte record header, identical on host and native (ADR 0003 section 2.1).

use crate::error::KvError;
use crate::identity::{ChunkKey, NamespaceId};

pub const MAGIC: [u8; 8] = *b"AIENKVC1";
pub const HEADER_LEN: usize = 256;
pub const FORMAT_VERSION: u16 = 1;
pub const RECORD_KIND_CHUNK: u16 = 1;
/// `flags` bit 0: the chunk holds fewer than `chunk_tokens` positions.
pub const FLAG_PARTIAL_TAIL: u16 = 1;
/// Header bytes covered by `header_sha256`.
pub const HASHED_PREFIX_LEN: usize = 224;

/// The header fields a caller sets: section 2.1 minus the constants (magic,
/// version, kind, header_len) and minus the two hashes. `payload_sha256` is
/// known only after the payload is read, so it lives in `RecordHeader`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkHeader {
    pub namespace_id: NamespaceId,
    pub chunk_key: ChunkKey,
    /// All zero for the first chunk.
    pub parent_key: [u8; 32],
    pub model_sha256: [u8; 32],
    pub first_position: u32,
    pub token_count: u32,
    pub chunk_tokens: u32,
    pub block_size: u32,
    pub num_layers: u32,
    pub num_kv_heads: u32,
    pub head_dim: u32,
    pub dtype: u8,
    pub backend_format: u8,
    pub flags: u16,
    pub payload_len: u64,
    /// Informational, not identity.
    pub written_unix_ns: u64,
}

/// A header plus the payload hash; what sits in the first 256 bytes of a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordHeader {
    pub chunk: ChunkHeader,
    pub payload_sha256: [u8; 32],
}

impl RecordHeader {
    /// Serialize to exactly 256 bytes, little-endian, computing `header_sha256`.
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        unimplemented!("C1 green")
    }

    /// Structural decode: length, magic, version, kind, header_len, header
    /// checksum, reserved flag bits. Identity checks are in `validate_record`.
    pub fn decode(buf: &[u8]) -> Result<RecordHeader, KvError> {
        let _ = buf;
        unimplemented!("C1 green")
    }
}

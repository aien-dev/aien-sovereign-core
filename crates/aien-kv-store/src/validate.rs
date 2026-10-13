//! Validation before any allocation or copy (ADR 0003 section 1.4).

use crate::error::KvError;
use crate::record::{ChunkHeader, RecordHeader, FLAG_PARTIAL_TAIL, HEADER_LEN};

/// Check `buf` (at least the first 256 bytes of a record) and the total
/// `file_size` against what the caller expects. Order: length, magic, version,
/// kind, header_len, header checksum, reserved flags (all in
/// `RecordHeader::decode`), then namespace, key, parent, position, token
/// count, geometry, partial flag, payload_len, file size. Performs no heap
/// allocation and copies no payload; the first failure is returned.
///
/// `expect` is the header the caller derived for the chunk it asked for
/// (`NamespaceFields::chunk_header`); its `written_unix_ns` is ignored.
pub fn validate_record(
    buf: &[u8],
    file_size: u64,
    expect: &ChunkHeader,
    segment_bytes: u64,
) -> Result<RecordHeader, KvError> {
    let _ = segment_bytes;
    let rec = RecordHeader::decode(buf)?;
    let h = &rec.chunk;
    if h.namespace_id != expect.namespace_id {
        return Err(KvError::NamespaceMismatch);
    }
    if h.chunk_key != expect.chunk_key {
        return Err(KvError::KeyMismatch);
    }
    if h.parent_key != expect.parent_key {
        return Err(KvError::ParentMismatch);
    }
    if h.first_position != expect.first_position
        || h.chunk_tokens == 0
        || !h.first_position.is_multiple_of(h.chunk_tokens)
    {
        return Err(KvError::PositionMismatch);
    }
    if h.token_count == 0 || h.token_count > h.chunk_tokens {
        return Err(KvError::TokenCountOutOfRange);
    }
    if h.token_count != expect.token_count {
        return Err(KvError::TokenCountMismatch);
    }
    if h.model_sha256 != expect.model_sha256
        || h.chunk_tokens != expect.chunk_tokens
        || h.block_size != expect.block_size
        || h.num_layers != expect.num_layers
        || h.num_kv_heads != expect.num_kv_heads
        || h.head_dim != expect.head_dim
        || h.dtype != expect.dtype
        || h.backend_format != expect.backend_format
    {
        return Err(KvError::GeometryMismatch);
    }
    let partial = h.token_count < h.chunk_tokens;
    if (h.flags & FLAG_PARTIAL_TAIL != 0) != partial || h.flags != expect.flags {
        return Err(KvError::BadFlags);
    }
    if h.payload_len != expect.payload_len {
        return Err(KvError::PayloadLenMismatch);
    }
    match (HEADER_LEN as u64).checked_add(h.payload_len) {
        Some(total) if total == file_size => Ok(rec),
        _ => Err(KvError::FileSizeMismatch),
    }
}

//! Validation before any allocation or copy (ADR 0003 section 1.4).

use crate::error::KvError;
use crate::record::{ChunkHeader, RecordHeader};

/// Check `buf` (at least the first 256 bytes of a record) and the total
/// `file_size` against what the caller expects. Order: length, magic, version,
/// kind, header_len, header checksum, flags, namespace, key, parent, position,
/// token count, geometry, payload_len, file size. Performs no allocation and
/// copies nothing; the first failure is returned.
pub fn validate_record(
    buf: &[u8],
    file_size: u64,
    expect: &ChunkHeader,
) -> Result<RecordHeader, KvError> {
    let _ = (buf, file_size, expect);
    unimplemented!("C1 green")
}

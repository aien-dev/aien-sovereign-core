mod common;

use aien_kv_store::{KvError, RecordHeader, HEADER_LEN, MAGIC};
use common::*;

fn record() -> RecordHeader {
    RecordHeader {
        chunk: sample_chunk_header(),
        payload_sha256: [0x5A; 32],
    }
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

#[test]
fn header_roundtrip_256_bytes() {
    let r = record();
    let bytes = r.encode();
    assert_eq!(bytes.len(), 256);
    assert_eq!(HEADER_LEN, 256);
    let back = RecordHeader::decode(&bytes).unwrap();
    assert_eq!(back, r);
    // Re-encoding is byte for byte identical.
    assert_eq!(back.encode(), bytes);
    // Extra trailing bytes (the payload) do not matter to decode.
    let mut with_payload = bytes.to_vec();
    with_payload.extend_from_slice(&[1, 2, 3]);
    assert_eq!(RecordHeader::decode(&with_payload).unwrap(), r);
}

/// Every field sits at the offset ADR 0003 section 2.1 gives.
#[test]
fn header_fields_sit_at_adr_offsets() {
    let r = record();
    let c = r.chunk;
    let b = r.encode();
    assert_eq!(&b[0..8], &MAGIC);
    assert_eq!(&b[0..8], b"AIENKVC1");
    assert_eq!(u16::from_le_bytes([b[8], b[9]]), 1); // format_version
    assert_eq!(u16::from_le_bytes([b[10], b[11]]), 1); // record_kind
    assert_eq!(u32_at(&b, 12), 256); // header_len
    assert_eq!(&b[16..48], &c.namespace_id.0);
    assert_eq!(&b[48..80], c.chunk_key.as_bytes());
    assert_eq!(&b[80..112], &c.parent_key);
    assert_eq!(&b[112..144], &c.model_sha256);
    assert_eq!(u32_at(&b, 144), c.first_position);
    assert_eq!(u32_at(&b, 148), c.token_count);
    assert_eq!(u32_at(&b, 152), c.chunk_tokens);
    assert_eq!(u32_at(&b, 156), c.block_size);
    assert_eq!(u32_at(&b, 160), c.num_layers);
    assert_eq!(u32_at(&b, 164), c.num_kv_heads);
    assert_eq!(u32_at(&b, 168), c.head_dim);
    assert_eq!(b[172], c.dtype);
    assert_eq!(b[173], c.backend_format);
    assert_eq!(u16::from_le_bytes([b[174], b[175]]), c.flags);
    assert_eq!(u64_at(&b, 176), c.payload_len);
    assert_eq!(&b[184..216], &r.payload_sha256);
    assert_eq!(u64_at(&b, 216), c.written_unix_ns);
    // header_sha256 covers bytes 0..224.
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(&b[0..224]);
    assert_eq!(&b[224..256], digest.as_slice());
}

#[test]
fn decode_refuses_short_input_and_structural_damage() {
    let b = record().encode();
    assert_eq!(RecordHeader::decode(&b[..255]), Err(KvError::Truncated));
    assert_eq!(RecordHeader::decode(&[]), Err(KvError::Truncated));
    let mut bad = b;
    bad[3] ^= 0xFF;
    assert_eq!(RecordHeader::decode(&bad), Err(KvError::BadMagic));
    let mut bad = b;
    bad[100] ^= 1; // inside the hashed prefix, magic untouched
    assert_eq!(RecordHeader::decode(&bad), Err(KvError::HeaderChecksum));
    let mut bad = b;
    bad[240] ^= 1; // inside the stored hash itself
    assert_eq!(RecordHeader::decode(&bad), Err(KvError::HeaderChecksum));
}

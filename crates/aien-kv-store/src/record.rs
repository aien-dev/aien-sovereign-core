//! The 256-byte record header, identical on host and native (ADR 0003 section 2.1).

use crate::error::KvError;
use crate::identity::{ChunkKey, NamespaceId};
use aien_platform::ObjectId;
use sha2::{Digest, Sha256};

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

// Byte offsets of ADR 0003 section 2.1.
const O_MAGIC: usize = 0;
const O_VERSION: usize = 8;
const O_KIND: usize = 10;
const O_HEADER_LEN: usize = 12;
const O_NAMESPACE: usize = 16;
const O_KEY: usize = 48;
const O_PARENT: usize = 80;
const O_MODEL: usize = 112;
const O_FIRST_POS: usize = 144;
const O_TOKEN_COUNT: usize = 148;
const O_CHUNK_TOKENS: usize = 152;
const O_BLOCK_SIZE: usize = 156;
const O_LAYERS: usize = 160;
const O_KV_HEADS: usize = 164;
const O_HEAD_DIM: usize = 168;
const O_DTYPE: usize = 172;
const O_BACKEND: usize = 173;
const O_FLAGS: usize = 174;
const O_PAYLOAD_LEN: usize = 176;
const O_PAYLOAD_SHA: usize = 184;
const O_WRITTEN_NS: usize = 216;
const O_HEADER_SHA: usize = 224;

fn put(buf: &mut [u8; HEADER_LEN], off: usize, bytes: &[u8]) {
    buf[off..off + bytes.len()].copy_from_slice(bytes);
}

fn get32(buf: &[u8], off: usize) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&buf[off..off + 32]);
    out
}

fn get_u16(buf: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([buf[off], buf[off + 1]])
}

fn get_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

fn get_u64(buf: &[u8], off: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&buf[off..off + 8]);
    u64::from_le_bytes(b)
}

impl RecordHeader {
    /// Serialize to exactly 256 bytes, little-endian, computing `header_sha256`.
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let c = &self.chunk;
        let mut b = [0u8; HEADER_LEN];
        put(&mut b, O_MAGIC, &MAGIC);
        put(&mut b, O_VERSION, &FORMAT_VERSION.to_le_bytes());
        put(&mut b, O_KIND, &RECORD_KIND_CHUNK.to_le_bytes());
        put(&mut b, O_HEADER_LEN, &(HEADER_LEN as u32).to_le_bytes());
        put(&mut b, O_NAMESPACE, &c.namespace_id.0);
        put(&mut b, O_KEY, c.chunk_key.as_bytes());
        put(&mut b, O_PARENT, &c.parent_key);
        put(&mut b, O_MODEL, &c.model_sha256);
        put(&mut b, O_FIRST_POS, &c.first_position.to_le_bytes());
        put(&mut b, O_TOKEN_COUNT, &c.token_count.to_le_bytes());
        put(&mut b, O_CHUNK_TOKENS, &c.chunk_tokens.to_le_bytes());
        put(&mut b, O_BLOCK_SIZE, &c.block_size.to_le_bytes());
        put(&mut b, O_LAYERS, &c.num_layers.to_le_bytes());
        put(&mut b, O_KV_HEADS, &c.num_kv_heads.to_le_bytes());
        put(&mut b, O_HEAD_DIM, &c.head_dim.to_le_bytes());
        b[O_DTYPE] = c.dtype;
        b[O_BACKEND] = c.backend_format;
        put(&mut b, O_FLAGS, &c.flags.to_le_bytes());
        put(&mut b, O_PAYLOAD_LEN, &c.payload_len.to_le_bytes());
        put(&mut b, O_PAYLOAD_SHA, &self.payload_sha256);
        put(&mut b, O_WRITTEN_NS, &c.written_unix_ns.to_le_bytes());
        let digest: [u8; 32] = Sha256::digest(&b[..HASHED_PREFIX_LEN]).into();
        put(&mut b, O_HEADER_SHA, &digest);
        b
    }

    /// Structural decode: length, magic, version, kind, header_len, header
    /// checksum, reserved flag bits. Identity checks are in `validate_record`.
    /// No allocation.
    pub fn decode(buf: &[u8]) -> Result<RecordHeader, KvError> {
        if buf.len() < HEADER_LEN {
            return Err(KvError::Truncated);
        }
        if buf[O_MAGIC..O_MAGIC + 8] != MAGIC {
            return Err(KvError::BadMagic);
        }
        if get_u16(buf, O_VERSION) != FORMAT_VERSION {
            return Err(KvError::UnsupportedVersion);
        }
        if get_u16(buf, O_KIND) != RECORD_KIND_CHUNK {
            return Err(KvError::BadRecordKind);
        }
        if get_u32(buf, O_HEADER_LEN) as usize != HEADER_LEN {
            return Err(KvError::BadHeaderLen);
        }
        let digest = Sha256::digest(&buf[..HASHED_PREFIX_LEN]);
        if digest.as_slice() != &buf[O_HEADER_SHA..O_HEADER_SHA + 32] {
            return Err(KvError::HeaderChecksum);
        }
        let flags = get_u16(buf, O_FLAGS);
        if flags & !FLAG_PARTIAL_TAIL != 0 {
            return Err(KvError::BadFlags);
        }
        Ok(RecordHeader {
            chunk: ChunkHeader {
                namespace_id: NamespaceId(get32(buf, O_NAMESPACE)),
                chunk_key: ChunkKey(ObjectId(get32(buf, O_KEY))),
                parent_key: get32(buf, O_PARENT),
                model_sha256: get32(buf, O_MODEL),
                first_position: get_u32(buf, O_FIRST_POS),
                token_count: get_u32(buf, O_TOKEN_COUNT),
                chunk_tokens: get_u32(buf, O_CHUNK_TOKENS),
                block_size: get_u32(buf, O_BLOCK_SIZE),
                num_layers: get_u32(buf, O_LAYERS),
                num_kv_heads: get_u32(buf, O_KV_HEADS),
                head_dim: get_u32(buf, O_HEAD_DIM),
                dtype: buf[O_DTYPE],
                backend_format: buf[O_BACKEND],
                flags,
                payload_len: get_u64(buf, O_PAYLOAD_LEN),
                written_unix_ns: get_u64(buf, O_WRITTEN_NS),
            },
            payload_sha256: get32(buf, O_PAYLOAD_SHA),
        })
    }
}

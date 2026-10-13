// Prefix-chained chunk keys adapt the rolling-hash idea of LMCache's
// `TokenHasher` (lmcache/v1/multiprocess/token_hasher.py, lines 30-52, 60-70,
// 183-190 at 4d69cdc4). LMCache is Apache-2.0, Copyright LMCache contributors.
// No LMCache code is copied. This file hashes with SHA-256 over a different
// byte layout (ADR 0003 section 1) so host and native share one derivation.

//! Versioned chunk identity: namespace id and prefix-chained chunk keys
//! (ADR 0003 section 1). Pure functions, no I/O, no heap use while hashing.

use crate::error::KvError;
use aien_platform::ObjectId;
use alloc::vec::Vec;

/// Fingerprint of everything that must be equal for stored bytes to be reusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NamespaceId(pub [u8; 32]);

/// Key of one chunk; carries the prefix chain. Wraps the platform `ObjectId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkKey(pub ObjectId);

impl ChunkKey {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0 .0
    }
}

impl PartialOrd for ChunkKey {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ChunkKey {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

/// KV element type codes (ADR 1.2 field 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KvDtype {
    Fp4 = 1,
    Fp8 = 2,
    Bf16 = 3,
    Fp16 = 4,
    Fp32 = 5,
}

impl KvDtype {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Fp4),
            2 => Some(Self::Fp8),
            3 => Some(Self::Bf16),
            4 => Some(Self::Fp16),
            5 => Some(Self::Fp32),
            _ => None,
        }
    }
}

/// Payload byte format (ADR 1.2 field 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BackendFormat {
    /// Host paged pool v1: bf16 blocks in the pool's own layout (ADR 2.2).
    HostPagedPoolV1 = 1,
    /// Native f32 decode state v1: strided per layer (ADR 2.3).
    NativeF32DecodeStateV1 = 2,
}

impl BackendFormat {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::HostPagedPoolV1),
            2 => Some(Self::NativeF32DecodeStateV1),
            _ => None,
        }
    }
}

/// Llama-3 RoPE scaling (ADR 1.2 field 7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RopeScaling {
    pub factor: f64,
    pub low_freq_factor: f64,
    pub high_freq_factor: f64,
    pub original_max_position_embeddings: u64,
}

/// Byte strides of the pool layout (ADR 1.2 field 10; `KvLayout` in aien-kv-cache).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutStrides {
    pub head_stride: u64,
    pub token_stride: u64,
    pub kv_plane_stride: u64,
    pub layer_stride: u64,
    pub block_stride: u64,
    pub element_bytes: u64,
}

/// Every input of the namespace id, in ADR 1.2 order. Strings are borrowed so a
/// caller builds this without copying model metadata.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NamespaceFields<'a> {
    pub model_sha256: [u8; 32],
    /// At most 255 bytes (u8 length prefix).
    pub model_digest_kind: &'a str,
    /// At most 65535 bytes (u16 length prefix).
    pub model_id: &'a str,
    pub num_layers: u32,
    pub num_kv_heads: u32,
    pub head_dim: u32,
    pub block_size: u32,
    pub rope_theta: f32,
    pub rope_scaling: Option<RopeScaling>,
    pub qk_norm: bool,
    pub dtype: KvDtype,
    pub layout: LayoutStrides,
    pub backend_format: BackendFormat,
    pub chunk_tokens: u32,
    /// At most 64 bytes (METRICS_AND_SETTINGS `AIEN_KV_STORE_NAMESPACE`).
    pub sharing_namespace: &'a str,
}

pub const MAX_SHARING_NAMESPACE_BYTES: usize = 64;

impl NamespaceFields<'_> {
    /// `SHA-256("AIEN-PKV-NS-V1\0" || fields)` (ADR 1.2).
    pub fn namespace_id(&self) -> Result<NamespaceId, KvError> {
        unimplemented!("C1 green")
    }

    /// Payload size the writer must produce for `token_count` positions
    /// (ADR 2.2 and 2.3). Checked arithmetic, no allocation.
    pub fn expected_payload_len(&self, token_count: u32) -> Result<u64, KvError> {
        let _ = token_count;
        unimplemented!("C1 green")
    }
}

/// One link of the chain (ADR 1.3). `parent` is `None` for the first chunk
/// (hashed as 32 zero bytes). `first_position` is the absolute position of
/// `tokens[0]`.
pub fn chunk_key(
    namespace: &NamespaceId,
    parent: Option<&ChunkKey>,
    first_position: u32,
    tokens: &[u32],
) -> Result<ChunkKey, KvError> {
    let _ = (namespace, parent, first_position, tokens);
    unimplemented!("C1 green")
}

/// Keys for every chunk of `tokens`: full chunks of `chunk_tokens`, then an
/// optional partial tail keyed with its exact token count.
pub fn chunk_chain(
    namespace: &NamespaceId,
    chunk_tokens: u32,
    tokens: &[u32],
) -> Result<Vec<ChunkKey>, KvError> {
    let _ = (namespace, chunk_tokens, tokens);
    unimplemented!("C1 green")
}

impl NamespaceFields<'_> {
    /// Build the header a writer would emit (or a reader would expect) for one
    /// chunk. Sets `partial_tail` when `token_count < chunk_tokens`; fills
    /// `payload_len` from `expected_payload_len`.
    pub fn chunk_header(
        &self,
        namespace_id: &NamespaceId,
        chunk_key: ChunkKey,
        parent: Option<&ChunkKey>,
        first_position: u32,
        token_count: u32,
        written_unix_ns: u64,
    ) -> Result<crate::record::ChunkHeader, KvError> {
        let _ = (
            namespace_id,
            chunk_key,
            parent,
            first_position,
            token_count,
            written_unix_ns,
        );
        unimplemented!("C1 green")
    }
}

//! Persistent KV cache: versioned chunk identity, the 256-byte record header in
//! its 4096-byte region, validation, and the `KvStore` contract with a
//! deterministic fake adapter. Binding design: `docs/adr/0003-persistent-kv-identity-storage-contract-and-ownership.md`.
//!
//! `no_std` + `alloc` for identity, codec, validation, pinned views and the
//! trait. The `std` feature (default) adds the in-memory fake adapter.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod error;
#[cfg(feature = "std")]
pub mod fake;
pub mod identity;
pub mod pin;
pub mod record;
pub mod store;
pub mod validate;

pub use error::KvError;
pub use identity::{
    chunk_chain, chunk_key, BackendFormat, ChunkKey, ComputeConfig, HiddenAct, KvDtype,
    LayoutStrides, NamespaceFields, NamespaceId, RopeScaling,
};
pub use pin::{HolderGuard, HolderHandle, PinnedDest, PinnedSource};
pub use record::{
    payload_root, segment_count, ChunkHeader, RecordHeader, HEADER_LEN, HEADER_REGION_LEN, MAGIC,
    MAX_SEGMENTS, PAYLOAD_OFFSET, SEGMENT_TABLE_OFFSET,
};
pub use store::{
    CancelOutcome, Capacity, Completion, DrainReport, Incarnation, JobId, JobStage, KvStore,
    PinToken, PublishOutcome, RecoveryProgress, StoreState, Terminal, UnfinishedJob,
};
pub use validate::validate_record;

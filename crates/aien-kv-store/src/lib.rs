//! Persistent KV cache: versioned chunk identity, the 256-byte record header,
//! validation, and (from cut C2) the `KvStore` contract with a deterministic
//! fake adapter. Binding design: `docs/adr/0003-persistent-kv-identity-storage-contract-and-ownership.md`.
//!
//! `no_std` + `alloc` for identity, codec and validation. The `std` feature
//! (default) adds the in-memory fake adapter.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod error;
pub mod identity;
pub mod record;
pub mod validate;

pub use error::KvError;
pub use identity::{
    chunk_chain, chunk_key, BackendFormat, ChunkKey, KvDtype, LayoutStrides, NamespaceFields,
    NamespaceId, RopeScaling,
};
pub use record::{ChunkHeader, RecordHeader, HEADER_LEN, MAGIC};
pub use validate::validate_record;

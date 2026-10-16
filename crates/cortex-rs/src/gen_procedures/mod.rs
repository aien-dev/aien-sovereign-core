//! GEN procedures: verified reusable Omegese records as evidence-bound Cortex memory
//! (aien-spark/aien-sovereign-core#556, part of aien-spark/omega#475).
//!
//! Opt-in library API only: nothing in the server, CLI, workers or promotion engine calls it, so the
//! default behaviour of Cortex is unchanged.
//!
//! * [`import_record_set`] strictly imports one GEN record set (record schema v1, `gen-record 1`,
//!   omega tools/gen/abstract/RECORD_SCHEMA.md sha256 9be17b6b...6b53) into an isolated `gen-*` space.
//!   Record and receipt bytes are stored verbatim in `memory_candidates` rows, with their digests,
//!   receipts, ancestry and sources as `candidate_evidence` rows. The rows use state `gen_imported`
//!   (never `pending`, so neither auto-promotion nor the context compiler sees them) and tier
//!   `T3Controlled` (explicit promotion is refused by the existing policy). No migration is needed.
//! * [`ReadOnlyStore`] opens the database read-only (`SQLITE_OPEN_READ_ONLY` plus `query_only`) and
//!   offers the INTEROP-ONLY retrieval adapter [`ReadOnlyStore::query`], which reproduces the
//!   file-backed provider `genret-cli-v3` byte for byte except for the interop marker lines of the
//!   receipt (`provider=cortex-interop`, `genret_parity`, `cortex_revoked`,
//!   `recheck_source_sha256=unverified`; see [`retrieval`]). It is not the experiment provider. [`ReadOnlyStore::export`] (byte-identical record set
//!   round trip) and [`ReadOnlyStore::audit`].
//! * [`revoke_record`] marks one imported record revoked (it is then excluded with reason `revoked`).
//!
//! A record is a suggestion shown to a model, never trusted code or an instruction: being eligible for
//! the verified filter is not permission to execute anything or to act on any system.

pub mod record;
pub mod retrieval;
pub mod store;

pub use record::{validate, NoReceipts, ReceiptSource, RecordMeta, Refusal};
pub use retrieval::{
    query_record_files, read_public_task_file, Excluded, GenQuery, GenRetError, GenRetOutput, Kind,
    Mode, PublicTaskFile, Selected, TaskRole, CONTRACT, HEADING, PINNED_RECHECK_SHA256,
    PROVIDER_MARKER, RECHECK_SOURCE_CLAIM, RECHECK_SOURCE_SHA256,
};
pub use store::{
    import_record_set, revoke_record, AuditRow, ExportReport, GenImportError, ImportReport,
    ImportRequest, ImportedRecord, ReadOnlyStore, MEMORY_TYPE_RECORD, MEMORY_TYPE_SET,
    SPACE_PREFIX, STATE_IMPORTED, STATE_REVOKED,
};

/// The record schema this module reads (first line of every record).
pub const RECORD_SCHEMA: &str = "gen-record 1";
/// sha256 of omega tools/gen/abstract/RECORD_SCHEMA.md at gen/abstract 937b033c.
pub const RECORD_SCHEMA_SHA256: &str =
    "9be17b6bc6f97c7420ced722bb258c420944bd87461097163df0992112ae6b53";
/// sha256 of omega tools/gen/retrieval/CONTRACT.md (genret-cli-v3) at gen/retrieval a26be577.
pub const CONTRACT_SHA256: &str =
    "ee21d714d911cf1cc459cba00805cceb9354b02bf8d6974e4d96c5c5dd01a3ac";

/// Lowercase hex sha256.
pub fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    crate::evidence::hex_encode(&Sha256::digest(b))
}

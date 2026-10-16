//! Cortex storage of GEN record sets: strict import, revocation, and a read-only store for
//! retrieval, export and audit. Reuses `spaces`, `sessions`, `memory_candidates`,
//! `candidate_evidence` and `evidence_records`; adds no table and no migration.
//!
//! Row layout (all rows carry `space_slug` = the import space):
//! * one `sessions` row per import: id `gen-import:<space>:<set>`, status `closed`, retention
//!   `permanent` (the FK target of the candidate rows).
//! * one set row in `memory_candidates`: id `genset:<space>:<set>`, memory_type `gen_record_set_v1`,
//!   object = MANIFEST bytes; evidence `manifest` (`gen_manifest`, the set digest) and one
//!   `record:<path>` per record.
//! * one row per record: id `genrec:<space>:<set>:<path>`, memory_type `gen_record_v1`, subject = the
//!   record id, object = the verbatim record bytes, its receipt bytes and the provenance read from
//!   the record; evidence `record:<path>` (`gen_record`), `receipt:<hex>` (`gen_receipt`),
//!   `ancestry:<task>` (`gen_ancestry`), `source:<path>` (`gen_source`).
//!
//! Readers never trust the JSON provenance copy: every query re-derives the record fields from the
//! verbatim bytes after checking them against the pinned MANIFEST, the evidence rows and the validator.

use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};

use super::record::{self, ReceiptSource, RecordMeta};
use super::retrieval::{
    self, DirReceipts, GenQuery, GenRetError, GenRetOutput, Loaded, ManifestLine,
};
use super::{sha256_hex, RECORD_SCHEMA, RECORD_SCHEMA_SHA256};

/// Every GEN import space starts with this prefix.
pub const SPACE_PREFIX: &str = "gen-";
pub const MEMORY_TYPE_RECORD: &str = "gen_record_v1";
pub const MEMORY_TYPE_SET: &str = "gen_record_set_v1";
/// Imported, not a claim, not pending: invisible to auto-promotion and the context compiler.
pub const STATE_IMPORTED: &str = "gen_imported";
pub const STATE_REVOKED: &str = "gen_revoked";
const EXTRACTOR: &str = "cortex-gen-import-v1";
const MIN_USER_VERSION: i32 = 5;

/// A refused import or revocation. Nothing is written when one is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenImportError {
    pub reason: &'static str,
    pub detail: String,
}

impl GenImportError {
    fn new(reason: &'static str, detail: impl Into<String>) -> Self {
        Self {
            reason,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for GenImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "gen import: {} {}", self.reason, self.detail)
    }
}

impl std::error::Error for GenImportError {}

fn store_err(e: rusqlite::Error) -> GenImportError {
    GenImportError::new("store_error", e.to_string())
}

/// What to import.
#[derive(Debug, Clone)]
pub struct ImportRequest<'a> {
    /// Isolated space, `gen-` + [a-z0-9-], at most 64 bytes.
    pub space: &'a str,
    /// The record set's `records/` directory (MANIFEST, SET.sha256, receipts/, record files).
    pub records_dir: &'a Path,
    /// The frozen record-set digest (sha256 of MANIFEST); must equal SET.sha256.
    pub set_sha256: &'a str,
    /// When given, every `source` digest is checked against `<source_root>/<path>` (recheck --root).
    pub source_root: Option<&'a Path>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedRecord {
    pub path: String,
    pub id: String,
    pub digest: String,
    pub kind: String,
    pub split: String,
    pub validity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub space: String,
    pub set_sha256: String,
    pub records: Vec<ImportedRecord>,
}

fn space_ok(s: &str) -> bool {
    s.len() <= 64
        && s.len() > SPACE_PREFIX.len()
        && s.starts_with(SPACE_PREFIX)
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn is_record_text(b: &[u8]) -> bool {
    b.iter().all(|&c| c == b'\n' || (0x20..=0x7e).contains(&c))
}

fn set_row_id(space: &str, set: &str) -> String {
    format!("genset:{}:{}", space, set)
}
fn record_row_id(space: &str, set: &str, path: &str) -> String {
    format!("genrec:{}:{}:{}", space, set, path)
}
fn session_id(space: &str, set: &str) -> String {
    format!("gen-import:{}:{}", space, set)
}

/// Strictly imports one record set into an isolated space. All or nothing: any refused record, a
/// digest mismatch, a non-isolated space or an existing import of the same set refuses the whole
/// import and writes nothing. Runs the Cortex migrations first (as `Database::open` does).
pub fn import_record_set(
    db_path: &Path,
    req: &ImportRequest<'_>,
) -> Result<ImportReport, GenImportError> {
    let space = req.space;
    let pin = req.set_sha256;
    if !space_ok(space) {
        return Err(GenImportError::new(
            "space_invalid",
            format!(
                "space must be {}[a-z0-9-], at most 64 bytes: {}",
                SPACE_PREFIX, space
            ),
        ));
    }
    if !retrieval::is_hex64(pin) {
        return Err(GenImportError::new(
            "set_digest_mismatch",
            "set digest must be 64 lowercase hex",
        ));
    }
    let dir = req.records_dir;
    let man = std::fs::read(dir.join("MANIFEST"))
        .map_err(|e| GenImportError::new("manifest_unreadable", e.to_string()))?;
    let setf = std::fs::read(dir.join("SET.sha256"))
        .map_err(|e| GenImportError::new("manifest_unreadable", format!("SET.sha256: {}", e)))?;
    if setf != format!("{}\n", pin).as_bytes() {
        return Err(GenImportError::new(
            "set_digest_mismatch",
            "SET.sha256 is not '<pinned digest>\\n'",
        ));
    }
    let lines = retrieval::parse_manifest(&man, pin).map_err(|e| {
        let reason = if e.reason == "manifest_digest_mismatch" {
            "set_digest_mismatch"
        } else {
            "manifest_malformed"
        };
        GenImportError::new(reason, e.detail)
    })?;

    struct Item {
        path: String,
        digest: String,
        bytes: Vec<u8>,
        receipt: Vec<u8>,
        meta: RecordMeta,
    }
    let receipts = DirReceipts(dir.join("receipts"));
    let mut items: Vec<Item> = Vec::with_capacity(lines.len());
    for m in &lines {
        let bytes = std::fs::read(dir.join(&m.path))
            .map_err(|_| GenImportError::new("record_missing", m.path.clone()))?;
        let d = sha256_hex(&bytes);
        if d != m.digest {
            return Err(GenImportError::new(
                "stale_digest",
                format!("{} has sha256 {}, manifest says {}", m.path, d, m.digest),
            ));
        }
        let meta = record::validate(&bytes, req.source_root, Some(&receipts))
            .map_err(|r| GenImportError::new("record_refused", format!("{}: {}", m.path, r)))?;
        let receipt = receipts.receipt(&meta.receipt).unwrap_or_default();
        if !is_record_text(&receipt) {
            return Err(GenImportError::new(
                "record_refused",
                format!(
                    "{}: receipt {} holds bytes outside LF and 0x20..0x7E (RECORD_SCHEMA.md section 4)",
                    m.path, meta.receipt
                ),
            ));
        }
        items.push(Item {
            path: m.path.clone(),
            digest: m.digest.clone(),
            bytes,
            receipt,
            meta,
        });
    }
    {
        let mut ids: Vec<&str> = items.iter().map(|i| i.meta.id.as_str()).collect();
        ids.sort_unstable();
        if let Some(w) = ids.windows(2).find(|w| w[0] == w[1]) {
            return Err(GenImportError::new("duplicate_id", w[0].to_string()));
        }
    }

    // migrations, then one immediate transaction for every row
    drop(crate::db::Database::open(db_path).map_err(store_err)?);
    let mut conn = Connection::open(db_path).map_err(store_err)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .and_then(|_| conn.busy_timeout(std::time::Duration::from_secs(5)))
        .map_err(store_err)?;
    let uv: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(store_err)?;
    if uv < MIN_USER_VERSION {
        return Err(GenImportError::new(
            "store_error",
            format!("schema user_version {} < {}", uv, MIN_USER_VERSION),
        ));
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(store_err)?;
    let count = |sql: &str| -> Result<i64, GenImportError> {
        tx.query_row(sql, params![space], |r| r.get(0))
            .map_err(store_err)
    };
    let foreign = count("SELECT COUNT(*) FROM entities WHERE space_slug = ?1")?
        + count("SELECT COUNT(*) FROM claims WHERE space_slug = ?1")?
        + count(
            "SELECT COUNT(*) FROM memory_candidates WHERE space_slug = ?1
             AND memory_type NOT IN ('gen_record_v1', 'gen_record_set_v1')",
        )?
        + count(
            "SELECT COUNT(*) FROM sessions WHERE space_slug = ?1 AND id NOT LIKE 'gen-import:%'",
        )?;
    if foreign != 0 {
        return Err(GenImportError::new(
            "space_not_isolated",
            format!("space {} already holds {} non-GEN rows", space, foreign),
        ));
    }
    let set_id = set_row_id(space, pin);
    let exists: Option<String> = tx
        .query_row(
            "SELECT id FROM memory_candidates WHERE id = ?1",
            params![set_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(store_err)?;
    if exists.is_some() {
        return Err(GenImportError::new(
            "already_imported",
            format!("set {} is already in space {}", pin, space),
        ));
    }
    let now = Utc::now().to_rfc3339();
    let has_space: Option<String> = tx
        .query_row(
            "SELECT id FROM spaces WHERE slug = ?1",
            params![space],
            |r| r.get(0),
        )
        .optional()
        .map_err(store_err)?;
    if has_space.is_none() {
        tx.execute(
            "INSERT INTO spaces (id, slug, created_at) VALUES (?1, ?2, ?3)",
            params![uuid::Uuid::new_v4().to_string(), space, now],
        )
        .map_err(store_err)?;
    }
    let sid = session_id(space, pin);
    tx.execute(
        "INSERT INTO sessions (id, space_slug, agent_id, world_id, parent_session_id, fork_event_id,
             created_at, closed_at, status, retention_class, metadata_json)
         VALUES (?1, ?2, ?3, NULL, NULL, NULL, ?4, ?4, 'closed', 'permanent', ?5)",
        params![
            sid,
            space,
            EXTRACTOR,
            now,
            json!({"kind": "gen-record-import", "schema": RECORD_SCHEMA, "setSha256": pin}).to_string()
        ],
    )
    .map_err(store_err)?;
    let insert_candidate = |id: &str, mtype: &str, subject: &str, predicate: &str, obj: &Value| {
        tx.execute(
            "INSERT INTO memory_candidates (id, space_slug, session_id, branch_id, memory_type, subject,
                 predicate, object_value_json, scope, confidence, verification_tier, state,
                 extractor_version, created_at)
             VALUES (?1, ?2, ?3, 'main', ?4, ?5, ?6, ?7, 'gen-procedures', 0.0, 'T3Controlled', ?8, ?9, ?10)",
            params![id, space, sid, mtype, subject, predicate, obj.to_string(), STATE_IMPORTED, EXTRACTOR, now],
        )
        .map_err(store_err)
    };
    let evidence = |cid: &str, eid: &str, kind: &str, hash: &str| -> Result<(), GenImportError> {
        tx.execute(
            "INSERT INTO candidate_evidence (candidate_id, evidence_id, source_kind, evidence_hash)
             VALUES (?1, ?2, ?3, ?4)",
            params![cid, eid, kind, hash],
        )
        .map_err(store_err)?;
        tx.execute(
            "INSERT INTO evidence_records (id, source_kind, source_id, commitment, merkle_root, segment_id,
                 retention_class, retain_until, created_at)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, 'permanent', NULL, ?5)",
            params![uuid::Uuid::new_v4().to_string(), kind, cid, hash, now],
        )
        .map_err(store_err)?;
        Ok(())
    };
    let set_obj = json!({
        "schema": RECORD_SCHEMA,
        "recordSchemaSha256": RECORD_SCHEMA_SHA256,
        "setSha256": pin,
        "manifest": String::from_utf8_lossy(&man),
        "records": items.len(),
    });
    insert_candidate(&set_id, MEMORY_TYPE_SET, pin, "gen-record-set", &set_obj)?;
    evidence(&set_id, "manifest", "gen_manifest", pin)?;
    for it in &items {
        evidence(
            &set_id,
            &format!("record:{}", it.path),
            "gen_record",
            &it.digest,
        )?;
    }
    let mut report = Vec::with_capacity(items.len());
    for it in &items {
        let cid = record_row_id(space, pin, &it.path);
        let m = &it.meta;
        let obj = json!({
            "schema": RECORD_SCHEMA,
            "setSha256": pin,
            "path": it.path,
            "digest": it.digest,
            "record": String::from_utf8_lossy(&it.bytes),
            "receiptId": m.receipt,
            "receipt": String::from_utf8_lossy(&it.receipt),
            "provenance": {
                "id": m.id, "kind": m.kind, "split": m.split, "families": m.families,
                "validity": m.validity,
                "ancestry": m.ancestry.iter().map(|(t, h)| json!({"task": t, "sha256": h})).collect::<Vec<_>>(),
                "sources": m.sources.iter().map(|(p, h)| json!({"path": p, "sha256": h})).collect::<Vec<_>>(),
            },
        });
        insert_candidate(&cid, MEMORY_TYPE_RECORD, &m.id, "gen-record", &obj)?;
        evidence(
            &cid,
            &format!("record:{}", it.path),
            "gen_record",
            &it.digest,
        )?;
        evidence(
            &cid,
            &format!("receipt:{}", m.receipt),
            "gen_receipt",
            &m.receipt,
        )?;
        for (t, h) in &m.ancestry {
            evidence(&cid, &format!("ancestry:{}", t), "gen_ancestry", h)?;
        }
        for (p, h) in &m.sources {
            evidence(&cid, &format!("source:{}", p), "gen_source", h)?;
        }
        report.push(ImportedRecord {
            path: it.path.clone(),
            id: m.id.clone(),
            digest: it.digest.clone(),
            kind: m.kind.clone(),
            split: m.split.clone(),
            validity: m.validity.clone(),
        });
    }
    tx.commit().map_err(store_err)?;
    Ok(ImportReport {
        space: space.to_string(),
        set_sha256: pin.to_string(),
        records: report,
    })
}

/// Marks one imported record revoked (state `gen_revoked`, plus a `revocation:<reason>` evidence
/// row). Revocation is one-way. `reason` is [a-z0-9_-]{1,40}.
pub fn revoke_record(
    db_path: &Path,
    space: &str,
    set_sha256: &str,
    record_id: &str,
    reason: &str,
) -> Result<(), GenImportError> {
    if !space_ok(space) {
        return Err(GenImportError::new("space_invalid", space.to_string()));
    }
    if !record::is_taskid(reason) {
        return Err(GenImportError::new(
            "reason_invalid",
            "reason must be [a-z0-9_-]{1,40}",
        ));
    }
    let mut conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(store_err)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .and_then(|_| conn.busy_timeout(std::time::Duration::from_secs(5)))
        .map_err(store_err)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(store_err)?;
    let prefix = format!(
        "{}:",
        record_row_id(space, set_sha256, "").trim_end_matches(':')
    );
    let cid: Option<String> = tx
        .query_row(
            "SELECT id FROM memory_candidates WHERE space_slug = ?1 AND memory_type = ?2
             AND subject = ?3 AND substr(id, 1, length(?4)) = ?4 AND state IN (?5, ?6)",
            params![
                space,
                MEMORY_TYPE_RECORD,
                record_id,
                prefix,
                STATE_IMPORTED,
                STATE_REVOKED
            ],
            |r| r.get(0),
        )
        .optional()
        .map_err(store_err)?;
    let cid = cid.ok_or_else(|| {
        GenImportError::new(
            "record_missing",
            format!("{} in set {} of space {}", record_id, set_sha256, space),
        )
    })?;
    tx.execute(
        "UPDATE memory_candidates SET state = ?1 WHERE id = ?2",
        params![STATE_REVOKED, cid],
    )
    .map_err(store_err)?;
    tx.execute(
        "INSERT OR IGNORE INTO candidate_evidence (candidate_id, evidence_id, source_kind, evidence_hash)
         VALUES (?1, ?2, 'gen_revocation', ?3)",
        params![cid, format!("revocation:{}", reason), sha256_hex(reason.as_bytes())],
    )
    .map_err(store_err)?;
    tx.commit().map_err(store_err)?;
    Ok(())
}

/// One record of an audited set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    pub path: String,
    pub id: String,
    pub digest: String,
    pub kind: String,
    /// `Ok` = eligible for the verified filter (split corpus, validity verified, receipt-backed,
    /// not revoked, not fence-poisoned); `Err` = the first failing reason.
    pub verified_filter: Result<(), String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    pub set_sha256: String,
    pub files: usize,
}

/// A Cortex database opened read-only. Every method reads; none can write (the connection is
/// opened with `SQLITE_OPEN_READ_ONLY` and `PRAGMA query_only = ON`).
pub struct ReadOnlyStore {
    conn: Connection,
}

struct StoredRecord {
    bytes: Vec<u8>,
    receipt_id: String,
    receipt: Vec<u8>,
    receipt_evidence: bool,
    revoked: bool,
}

struct StoredReceipt {
    id: String,
    bytes: Vec<u8>,
    backed: bool,
}
impl ReceiptSource for StoredReceipt {
    fn receipt(&self, id: &str) -> Option<Vec<u8>> {
        (self.backed && id == self.id).then(|| self.bytes.clone())
    }
}

/// A record re-verified against the pinned manifest: its manifest line, stored row and validated fields.
type VerifiedRecord = (ManifestLine, StoredRecord, RecordMeta);

fn ro_err(e: rusqlite::Error) -> GenRetError {
    GenRetError::new("store_unreadable", e.to_string())
}

impl ReadOnlyStore {
    /// Opens an existing Cortex database read-only. Never creates or migrates it.
    pub fn open(db_path: &Path) -> Result<Self, GenRetError> {
        let conn = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(ro_err)?;
        conn.execute_batch("PRAGMA query_only = ON;")
            .map_err(ro_err)?;
        Ok(Self { conn })
    }

    /// The underlying read-only connection (exposed so tests can prove writes are refused).
    #[doc(hidden)]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// True when SQLite reports the connection read-only and `query_only` is on.
    pub fn is_read_only(&self) -> bool {
        let ro = self
            .conn
            .is_readonly(rusqlite::DatabaseName::Main)
            .unwrap_or(false);
        let qo: i64 = self
            .conn
            .query_row("PRAGMA query_only", [], |r| r.get(0))
            .unwrap_or(0);
        ro && qo == 1
    }

    fn manifest(&self, space: &str, pin: &str) -> Result<Vec<u8>, GenRetError> {
        if !space_ok(space) {
            return Err(GenRetError::new("space_invalid", space.to_string()));
        }
        let obj: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT object_value_json, state FROM memory_candidates
                 WHERE id = ?1 AND space_slug = ?2 AND memory_type = ?3",
                params![set_row_id(space, pin), space, MEMORY_TYPE_SET],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(ro_err)?;
        let (obj, state) = obj.ok_or_else(|| {
            GenRetError::new(
                "manifest_digest_mismatch",
                format!("no record set pinned {} in space {}", pin, space),
            )
        })?;
        if state != STATE_IMPORTED {
            return Err(GenRetError::new(
                "manifest_malformed",
                format!("record set row state {} is not {}", state, STATE_IMPORTED),
            ));
        }
        let v: Value = serde_json::from_str(&obj)
            .map_err(|e| GenRetError::new("manifest_malformed", e.to_string()))?;
        let man = v
            .get("manifest")
            .and_then(Value::as_str)
            .ok_or_else(|| GenRetError::new("manifest_malformed", "set row has no manifest"))?;
        Ok(man.as_bytes().to_vec())
    }

    fn evidence_hash(
        &self,
        cid: &str,
        eid: &str,
        kind: &str,
    ) -> Result<Option<String>, GenRetError> {
        self.conn
            .query_row(
                "SELECT evidence_hash FROM candidate_evidence
                 WHERE candidate_id = ?1 AND evidence_id = ?2 AND source_kind = ?3",
                params![cid, eid, kind],
                |r| r.get(0),
            )
            .optional()
            .map_err(ro_err)
    }

    fn record(
        &self,
        space: &str,
        pin: &str,
        m: &ManifestLine,
    ) -> Result<StoredRecord, GenRetError> {
        let cid = record_row_id(space, pin, &m.path);
        let row: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT object_value_json, state FROM memory_candidates
                 WHERE id = ?1 AND space_slug = ?2 AND memory_type = ?3",
                params![cid, space, MEMORY_TYPE_RECORD],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(ro_err)?;
        let (obj, state) = row.ok_or_else(|| GenRetError::new("record_missing", m.path.clone()))?;
        let revoked = match state.as_str() {
            STATE_IMPORTED => false,
            STATE_REVOKED => true,
            other => {
                return Err(GenRetError::new(
                    "invalid_record",
                    format!("{}: row state {} is not a GEN import state", m.path, other),
                ))
            }
        };
        // the set's and the record's own digest commitments must both name the manifest digest
        for owner in [set_row_id(space, pin), cid.clone()] {
            match self.evidence_hash(&owner, &format!("record:{}", m.path), "gen_record")? {
                Some(h) if h == m.digest => {}
                Some(h) => {
                    return Err(GenRetError::new(
                        "stale_digest",
                        format!(
                            "{}: evidence commitment {} differs from manifest {}",
                            m.path, h, m.digest
                        ),
                    ))
                }
                None => {
                    return Err(GenRetError::new(
                        "stale_digest",
                        format!("{}: digest commitment missing", m.path),
                    ))
                }
            }
        }
        let v: Value = serde_json::from_str(&obj)
            .map_err(|e| GenRetError::new("invalid_record", format!("{}: {}", m.path, e)))?;
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let bytes = s("record")
            .ok_or_else(|| {
                GenRetError::new("invalid_record", format!("{}: no record bytes", m.path))
            })?
            .into_bytes();
        let receipt_id = s("receiptId").unwrap_or_default();
        let receipt = s("receipt").unwrap_or_default().into_bytes();
        let receipt_evidence = self
            .evidence_hash(&cid, &format!("receipt:{}", receipt_id), "gen_receipt")?
            .is_some_and(|h| h == receipt_id);
        Ok(StoredRecord {
            bytes,
            receipt_id,
            receipt,
            receipt_evidence,
            revoked,
        })
    }

    /// Runs `f` inside one read transaction, so every row it reads comes from the same database
    /// snapshot (a concurrent `revoke_record` lands either wholly before or wholly after).
    fn snapshot<T>(&self, f: impl FnOnce() -> Result<T, GenRetError>) -> Result<T, GenRetError> {
        self.conn.execute_batch("BEGIN DEFERRED").map_err(ro_err)?;
        let r = f();
        let end = self
            .conn
            .execute_batch(if r.is_ok() { "COMMIT" } else { "ROLLBACK" });
        match (r, end) {
            (Ok(v), Ok(())) => Ok(v),
            (Ok(_), Err(e)) => Err(ro_err(e)),
            (Err(e), _) => Err(e),
        }
    }

    /// The INTEROP-ONLY retrieval adapter: genret-cli-v3 over the record set pinned by `set_sha256`
    /// in `space`, read from one database snapshot. Fails closed (same reasons as genret) when the
    /// stored set no longer matches its pin.
    ///
    /// This is not the GEN experiment provider. Its receipt starts with `provider=cortex-interop`,
    /// says `recheck_source_sha256=unverified` and carries `genret_parity=no` once a Cortex
    /// revocation changed the outcome; the output must not be passed off as verified provider output.
    pub fn query(
        &self,
        space: &str,
        set_sha256: &str,
        q: &GenQuery<'_>,
    ) -> Result<GenRetOutput, GenRetError> {
        // genret checks the query before it opens the corpus; so does the adapter
        retrieval::check_query(q, set_sha256)?;
        self.snapshot(|| {
            let man = self.manifest(space, set_sha256)?;
            retrieval::run_query(&man, set_sha256, q, |m| {
                let r = self.record(space, set_sha256, m)?;
                Ok(Loaded {
                    bytes: r.bytes,
                    receipts: Box::new(StoredReceipt {
                        id: r.receipt_id,
                        bytes: r.receipt,
                        backed: r.receipt_evidence,
                    }),
                    revoked: r.revoked,
                })
            })
        })
    }

    /// Every record of the set, re-verified against the pin, from one database snapshot.
    fn verified_set(
        &self,
        space: &str,
        pin: &str,
    ) -> Result<(Vec<u8>, Vec<VerifiedRecord>), GenRetError> {
        if !retrieval::is_hex64(pin) {
            return Err(GenRetError::new(
                "usage",
                "set digest must be 64 lowercase hex",
            ));
        }
        self.snapshot(|| {
            let man = self.manifest(space, pin)?;
            let lines = retrieval::parse_manifest(&man, pin)?;
            let mut out = Vec::with_capacity(lines.len());
            for m in lines {
                let r = self.record(space, pin, &m)?;
                let d = sha256_hex(&r.bytes);
                if d != m.digest {
                    return Err(GenRetError::new(
                        "stale_digest",
                        format!("{} has sha256 {}, manifest says {}", m.path, d, m.digest),
                    ));
                }
                let rs = StoredReceipt {
                    id: r.receipt_id.clone(),
                    bytes: r.receipt.clone(),
                    backed: r.receipt_evidence,
                };
                let meta = record::validate(&r.bytes, None, Some(&rs)).map_err(|e| {
                    GenRetError::new("invalid_record", format!("recheck: {}: {}", m.path, e))
                })?;
                out.push((m, r, meta));
            }
            Ok((man, out))
        })
    }

    /// Writes the record set back out as `records/` files (MANIFEST, SET.sha256, every record, its
    /// receipt) into `out_dir`, which must not exist. Every byte is checked against the pin first.
    /// On a write failure the partial `out_dir` is removed; if that removal fails too, the error
    /// says so.
    pub fn export(
        &self,
        space: &str,
        set_sha256: &str,
        out_dir: &Path,
    ) -> Result<ExportReport, GenRetError> {
        if out_dir.symlink_metadata().is_ok() {
            return Err(GenRetError::new(
                "out_exists",
                format!("{} exists", out_dir.display()),
            ));
        }
        let (man, recs) = self.verified_set(space, set_sha256)?;
        let wf = |e: std::io::Error| GenRetError::new("write_failed", e.to_string());
        let write = || -> Result<usize, GenRetError> {
            std::fs::create_dir(out_dir).map_err(wf)?;
            std::fs::create_dir(out_dir.join("receipts")).map_err(wf)?;
            std::fs::write(out_dir.join("MANIFEST"), &man).map_err(wf)?;
            std::fs::write(out_dir.join("SET.sha256"), format!("{}\n", set_sha256)).map_err(wf)?;
            let mut files = 2;
            for (m, r, meta) in &recs {
                let p = out_dir.join(&m.path);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent).map_err(wf)?;
                }
                std::fs::write(&p, &r.bytes).map_err(wf)?;
                std::fs::write(out_dir.join("receipts").join(&meta.receipt), &r.receipt)
                    .map_err(wf)?;
                files += 2;
            }
            Ok(files)
        };
        match write() {
            Ok(files) => Ok(ExportReport {
                set_sha256: set_sha256.to_string(),
                files,
            }),
            Err(mut e) => {
                if let Err(re) = std::fs::remove_dir_all(out_dir) {
                    if out_dir.symlink_metadata().is_ok() {
                        e.detail = format!(
                            "{}; partial {} left behind: {}",
                            e.detail,
                            out_dir.display(),
                            re
                        );
                    }
                }
                Err(e)
            }
        }
    }

    /// Re-verifies every record of the set and reports, per record, whether it is eligible for the
    /// verified filter and, if not, why (the query's own eligibility chain, without the kind and
    /// family filters). Fails closed like [`ReadOnlyStore::query`].
    pub fn audit(&self, space: &str, set_sha256: &str) -> Result<Vec<AuditRow>, GenRetError> {
        let (_, recs) = self.verified_set(space, set_sha256)?;
        let mut out = Vec::with_capacity(recs.len());
        for (m, r, meta) in recs {
            let (f, body) = retrieval::header_fields(&r.bytes)
                .map_err(|e| GenRetError::new("invalid_record", format!("{}: {}", m.path, e)))?;
            let desc = f.get("desc").map(String::as_str).unwrap_or("");
            let verified_filter = match retrieval::ineligible(&retrieval::Eligibility {
                kind: None,
                rec_kind: &meta.kind,
                split: &meta.split,
                validity: &meta.validity,
                revoked: r.revoked,
                families: &meta.families,
                exclude: &[],
                body: &body,
                desc: desc.as_bytes(),
            }) {
                Some(why) => Err(why),
                None => Ok(()),
            };
            out.push(AuditRow {
                path: m.path,
                id: meta.id,
                digest: m.digest,
                kind: meta.kind,
                verified_filter,
            });
        }
        Ok(out)
    }
}

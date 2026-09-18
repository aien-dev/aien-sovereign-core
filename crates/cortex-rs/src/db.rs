use chrono::Utc;
use rusqlite::{params, Connection, Result};
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

use crate::embeddings::{bytes_to_embedding, cosine_similarity, embedding_to_bytes};
use crate::models::*;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let _: Result<String, _> = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0));
        conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "BEGIN;
            CREATE TABLE IF NOT EXISTS spaces (
                id TEXT PRIMARY KEY,
                slug TEXT UNIQUE NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS entities (
                id TEXT PRIMARY KEY,
                space_id TEXT NOT NULL,
                space_slug TEXT NOT NULL,
                entity_type TEXT NOT NULL,
                canonical_name TEXT NOT NULL,
                content TEXT NOT NULL,
                aliases_json TEXT NOT NULL DEFAULT '[]',
                metadata_json TEXT NOT NULL DEFAULT '{}',
                confidence REAL NOT NULL DEFAULT 1.0,
                revision INTEGER NOT NULL DEFAULT 1,
                retracted INTEGER NOT NULL DEFAULT 0,
                valid_from TEXT,
                valid_to TEXT,
                created_at TEXT NOT NULL,
                embedding BLOB
            );

            CREATE INDEX IF NOT EXISTS idx_entities_canonical ON entities(space_slug, canonical_name);
            CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(space_slug, entity_type);

            CREATE VIRTUAL TABLE IF NOT EXISTS entities_fts USING fts5(
                id UNINDEXED,
                canonical_name,
                content,
                space_slug UNINDEXED
            );

            CREATE TABLE IF NOT EXISTS claims (
                id TEXT PRIMARY KEY,
                space_id TEXT NOT NULL,
                space_slug TEXT NOT NULL,
                subject_entity_id TEXT NOT NULL,
                predicate TEXT NOT NULL,
                object_entity_id TEXT,
                literal_value_json TEXT,
                confidence REAL NOT NULL DEFAULT 1.0,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                retracted INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_claims_subject ON claims(subject_entity_id);
            CREATE INDEX IF NOT EXISTS idx_claims_object ON claims(object_entity_id);
            COMMIT;",
        )?;

        // Ensure default space exists
        let space_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO spaces (id, slug, created_at) VALUES (?1, ?2, ?3)",
            params![space_id, "atlas-memory", now],
        );

        Ok(())
    }

    pub fn ensure_space(&self, slug: &str) -> Result<String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM spaces WHERE slug = ?1")?;
        let mut rows = stmt.query(params![slug])?;
        if let Some(row) = rows.next()? {
            return row.get(0);
        }
        drop(rows);
        drop(stmt);

        let new_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO spaces (id, slug, created_at) VALUES (?1, ?2, ?3)",
            params![new_id, slug, now],
        )?;
        Ok(new_id)
    }

    pub fn upsert_entity(&self, input: &EntityWriteInput, embedding: Option<&[f32]>) -> Result<ReceiptDetails> {
        let space_id = self.ensure_space(&input.space)?;
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let embedding_blob = embedding.map(embedding_to_bytes);
        let aliases_json = serde_json::to_string(&input.aliases).unwrap_or_else(|_| "[]".to_string());
        let metadata_json = serde_json::to_string(&input.metadata).unwrap_or_else(|_| "{}".to_string());

        // Check if entity exists by canonical_name in space
        let mut stmt = conn.prepare("SELECT id, revision FROM entities WHERE space_slug = ?1 AND canonical_name = ?2")?;
        let mut rows = stmt.query(params![input.space, input.canonical_name])?;

        let (target_id, revision) = if let Some(row) = rows.next()? {
            let existing_id: String = row.get(0)?;
            let rev: i64 = row.get(1)?;
            (existing_id, rev + 1)
        } else {
            let new_id = input.id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
            (new_id, 1)
        };
        drop(rows);
        drop(stmt);

        conn.execute(
            "INSERT INTO entities (id, space_id, space_slug, entity_type, canonical_name, content, aliases_json, metadata_json, confidence, revision, retracted, valid_from, valid_to, created_at, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11, ?12, ?13, ?14)
             ON CONFLICT(id) DO UPDATE SET
                entity_type = excluded.entity_type,
                canonical_name = excluded.canonical_name,
                content = excluded.content,
                aliases_json = excluded.aliases_json,
                metadata_json = excluded.metadata_json,
                confidence = excluded.confidence,
                revision = excluded.revision,
                retracted = 0,
                valid_from = excluded.valid_from,
                valid_to = excluded.valid_to,
                embedding = COALESCE(excluded.embedding, entities.embedding)",
            params![
                target_id,
                space_id,
                input.space,
                input.entity_type,
                input.canonical_name,
                input.content,
                aliases_json,
                metadata_json,
                input.confidence,
                revision,
                input.valid_from,
                input.valid_to,
                now,
                embedding_blob,
            ],
        )?;

        // Update FTS table
        let _ = conn.execute("DELETE FROM entities_fts WHERE id = ?1", params![target_id]);
        let _ = conn.execute(
            "INSERT INTO entities_fts (id, canonical_name, content, space_slug) VALUES (?1, ?2, ?3, ?4)",
            params![target_id, input.canonical_name, input.content, input.space],
        );

        Ok(ReceiptDetails {
            id: Uuid::new_v4().to_string(),
            operation: "entity.upsert".to_string(),
            target_type: "entity".to_string(),
            target_id,
            committed_at: now,
        })
    }

    pub fn upsert_claim(&self, input: &ClaimWriteInput) -> Result<ReceiptDetails> {
        let space_id = self.ensure_space(&input.space)?;
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let claim_id = input.id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
        let literal_json = input.literal_value.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default());
        let metadata_json = serde_json::to_string(&input.metadata).unwrap_or_else(|_| "{}".to_string());

        conn.execute(
            "INSERT INTO claims (id, space_id, space_slug, subject_entity_id, predicate, object_entity_id, literal_value_json, confidence, metadata_json, retracted, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10)
             ON CONFLICT(id) DO UPDATE SET
                predicate = excluded.predicate,
                object_entity_id = excluded.object_entity_id,
                literal_value_json = excluded.literal_value_json,
                confidence = excluded.confidence,
                metadata_json = excluded.metadata_json,
                retracted = 0",
            params![
                claim_id,
                space_id,
                input.space,
                input.subject_entity_id,
                input.predicate,
                input.object_entity_id,
                literal_json,
                input.confidence,
                metadata_json,
                now,
            ],
        )?;

        Ok(ReceiptDetails {
            id: Uuid::new_v4().to_string(),
            operation: "claim.upsert".to_string(),
            target_type: "claim".to_string(),
            target_id: claim_id,
            committed_at: now,
        })
    }

    pub fn retract_target(&self, target_type: &str, target_id: &str) -> Result<ReceiptDetails> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        if target_type == "entity" {
            conn.execute("UPDATE entities SET retracted = 1 WHERE id = ?1", params![target_id])?;
            conn.execute("DELETE FROM entities_fts WHERE id = ?1", params![target_id])?;
        } else if target_type == "claim" {
            conn.execute("UPDATE claims SET retracted = 1 WHERE id = ?1", params![target_id])?;
        }

        Ok(ReceiptDetails {
            id: Uuid::new_v4().to_string(),
            operation: format!("{}.retract", target_type),
            target_type: target_type.to_string(),
            target_id: target_id.to_string(),
            committed_at: now,
        })
    }

    pub fn get_entity(&self, id_or_name: &str, space: Option<&str>) -> Result<Option<CortexEntity>> {
        let conn = self.conn.lock().unwrap();
        let target_space = space.unwrap_or("atlas-memory");

        let mut stmt = conn.prepare(
            "SELECT id, space_id, space_slug, entity_type, canonical_name, content, aliases_json, metadata_json, confidence, revision, retracted, valid_from, valid_to, created_at
             FROM entities
             WHERE (id = ?1 OR canonical_name = ?1) AND space_slug = ?2 AND retracted = 0
             LIMIT 1"
        )?;

        let mut rows = stmt.query(params![id_or_name, target_space])?;
        if let Some(row) = rows.next()? {
            let aliases_str: String = row.get(6)?;
            let meta_str: String = row.get(7)?;
            Ok(Some(CortexEntity {
                id: row.get(0)?,
                space_id: row.get(1)?,
                space_slug: row.get(2)?,
                entity_type: row.get(3)?,
                canonical_name: row.get(4)?,
                content: row.get(5)?,
                aliases: serde_json::from_str(&aliases_str).unwrap_or_default(),
                metadata: serde_json::from_str(&meta_str).unwrap_or_else(|_| serde_json::json!({})),
                confidence: row.get(8)?,
                revision: row.get(9)?,
                retracted: row.get::<_, i64>(10)? != 0,
                valid_from: row.get(11)?,
                valid_to: row.get(12)?,
                created_at: row.get(13)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn search_entities(&self, query: &str, space: Option<&str>, limit: usize) -> Result<Vec<SearchResult>> {
        let conn = self.conn.lock().unwrap();
        let target_space = space.unwrap_or("atlas-memory");
        let query_trimmed = query.trim();

        if query_trimmed.is_empty() {
            let mut stmt = conn.prepare(
                "SELECT id, space_id, space_slug, entity_type, canonical_name, content, aliases_json, metadata_json, confidence, revision, retracted, valid_from, valid_to, created_at
                 FROM entities
                 WHERE space_slug = ?1 AND retracted = 0
                 ORDER BY created_at DESC
                 LIMIT ?2"
            )?;
            let rows = stmt.query_map(params![target_space, limit], |row| {
                let aliases_str: String = row.get(6)?;
                let meta_str: String = row.get(7)?;
                Ok(SearchResult {
                    entity: CortexEntity {
                        id: row.get(0)?,
                        space_id: row.get(1)?,
                        space_slug: row.get(2)?,
                        entity_type: row.get(3)?,
                        canonical_name: row.get(4)?,
                        content: row.get(5)?,
                        aliases: serde_json::from_str(&aliases_str).unwrap_or_default(),
                        metadata: serde_json::from_str(&meta_str).unwrap_or_else(|_| serde_json::json!({})),
                        confidence: row.get(8)?,
                        revision: row.get(9)?,
                        retracted: row.get::<_, i64>(10)? != 0,
                        valid_from: row.get(11)?,
                        valid_to: row.get(12)?,
                        created_at: row.get(13)?,
                    },
                    lexical_score: 0.4,
                    graph_score: 0.0,
                    semantic_score: 0.0,
                    score: 0.4,
                })
            })?;

            let mut results = Vec::new();
            for r in rows {
                results.push(r?);
            }
            return Ok(results);
        }

        // FTS search + like search
        let clean_q = query_trimmed.replace('"', "");
        let fts_query = format!("\"{}\"", clean_q);
        let stmt = conn.prepare(
            "SELECT e.id, e.space_id, e.space_slug, e.entity_type, e.canonical_name, e.content, e.aliases_json, e.metadata_json, e.confidence, e.revision, e.retracted, e.valid_from, e.valid_to, e.created_at,
                    COALESCE(bm25(entities_fts), 5.0) as bm25_rank
             FROM entities e
             JOIN entities_fts f ON f.id = e.id
             WHERE entities_fts MATCH ?1 AND e.space_slug = ?2 AND e.retracted = 0
             ORDER BY bm25_rank ASC
             LIMIT ?3"
        );

        let mut results = Vec::new();
        if let Ok(mut s) = stmt {
            let rows = s.query_map(params![fts_query, target_space, limit], |row| {
                let aliases_str: String = row.get(6)?;
                let meta_str: String = row.get(7)?;
                let bm25_rank: f64 = row.get(14)?;
                let lexical_score = 1.0 / (1.0 + bm25_rank.abs());
                Ok(SearchResult {
                    entity: CortexEntity {
                        id: row.get(0)?,
                        space_id: row.get(1)?,
                        space_slug: row.get(2)?,
                        entity_type: row.get(3)?,
                        canonical_name: row.get(4)?,
                        content: row.get(5)?,
                        aliases: serde_json::from_str(&aliases_str).unwrap_or_default(),
                        metadata: serde_json::from_str(&meta_str).unwrap_or_else(|_| serde_json::json!({})),
                        confidence: row.get(8)?,
                        revision: row.get(9)?,
                        retracted: row.get::<_, i64>(10)? != 0,
                        valid_from: row.get(11)?,
                        valid_to: row.get(12)?,
                        created_at: row.get(13)?,
                    },
                    lexical_score,
                    graph_score: 0.0,
                    semantic_score: 0.0,
                    score: lexical_score,
                })
            })?;
            for r in rows {
                results.push(r?);
            }
        }

        // Fallback to substring match if FTS didn't return enough
        if results.len() < limit {
            let pattern = format!("%{}%", query_trimmed);
            let mut sub_stmt = conn.prepare(
                "SELECT id, space_id, space_slug, entity_type, canonical_name, content, aliases_json, metadata_json, confidence, revision, retracted, valid_from, valid_to, created_at
                 FROM entities
                 WHERE (canonical_name LIKE ?1 OR content LIKE ?1) AND space_slug = ?2 AND retracted = 0
                 LIMIT ?3"
            )?;
            let sub_rows = sub_stmt.query_map(params![pattern, target_space, limit], |row| {
                let aliases_str: String = row.get(6)?;
                let meta_str: String = row.get(7)?;
                Ok(SearchResult {
                    entity: CortexEntity {
                        id: row.get(0)?,
                        space_id: row.get(1)?,
                        space_slug: row.get(2)?,
                        entity_type: row.get(3)?,
                        canonical_name: row.get(4)?,
                        content: row.get(5)?,
                        aliases: serde_json::from_str(&aliases_str).unwrap_or_default(),
                        metadata: serde_json::from_str(&meta_str).unwrap_or_else(|_| serde_json::json!({})),
                        confidence: row.get(8)?,
                        revision: row.get(9)?,
                        retracted: row.get::<_, i64>(10)? != 0,
                        valid_from: row.get(11)?,
                        valid_to: row.get(12)?,
                        created_at: row.get(13)?,
                    },
                    lexical_score: 0.7,
                    graph_score: 0.0,
                    semantic_score: 0.0,
                    score: 0.7,
                })
            })?;
            for r in sub_rows {
                let sr = r?;
                if !results.iter().any(|existing| existing.entity.id == sr.entity.id) {
                    results.push(sr);
                }
            }
        }

        results.truncate(limit);
        Ok(results)
    }

    pub fn recall_entities(&self, query_text: &str, query_embedding: Option<&[f32]>, space: Option<&str>, limit: usize) -> Result<Vec<RecallItem>> {
        let conn = self.conn.lock().unwrap();
        let target_space = space.unwrap_or("atlas-memory");

        let mut stmt = conn.prepare(
            "SELECT id, space_id, space_slug, entity_type, canonical_name, content, aliases_json, metadata_json, confidence, revision, retracted, valid_from, valid_to, created_at, embedding
             FROM entities
             WHERE space_slug = ?1 AND retracted = 0"
        )?;

        let rows = stmt.query_map(params![target_space], |row| {
            let aliases_str: String = row.get(6)?;
            let meta_str: String = row.get(7)?;
            let embedding_blob: Option<Vec<u8>> = row.get(14)?;
            let entity = CortexEntity {
                id: row.get(0)?,
                space_id: row.get(1)?,
                space_slug: row.get(2)?,
                entity_type: row.get(3)?,
                canonical_name: row.get(4)?,
                content: row.get(5)?,
                aliases: serde_json::from_str(&aliases_str).unwrap_or_default(),
                metadata: serde_json::from_str(&meta_str).unwrap_or_else(|_| serde_json::json!({})),
                confidence: row.get(8)?,
                revision: row.get(9)?,
                retracted: row.get::<_, i64>(10)? != 0,
                valid_from: row.get(11)?,
                valid_to: row.get(12)?,
                created_at: row.get(13)?,
            };
            Ok((entity, embedding_blob))
        })?;

        let mut candidates = Vec::new();
        let query_lower = query_text.to_lowercase();

        for r in rows {
            let (entity, emb_blob) = r?;
            let mut semantic_score = 0.0f64;

            if let (Some(q_emb), Some(blob)) = (query_embedding, emb_blob) {
                let target_emb = bytes_to_embedding(&blob);
                semantic_score = cosine_similarity(q_emb, &target_emb) as f64;
            }

            // Calculate simple lexical score
            let name_lower = entity.canonical_name.to_lowercase();
            let content_lower = entity.content.to_lowercase();
            let mut lexical_score = 0.0f64;
            if name_lower.contains(&query_lower) {
                lexical_score = 0.9;
            } else if content_lower.contains(&query_lower) {
                lexical_score = 0.6;
            }

            let final_score = if query_embedding.is_some() {
                0.6 * semantic_score + 0.4 * lexical_score
            } else {
                lexical_score
            };

            candidates.push(RecallItem {
                entity,
                final_score,
                component_scores: RecallComponentScores {
                    lexical: lexical_score,
                    semantic: semantic_score,
                    confidence_recency: 0.95,
                    graph: 0.0,
                },
            });
        }

        candidates.sort_by(|a, b| b.final_score.partial_cmp(&a.final_score).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(limit);
        Ok(candidates)
    }

    pub fn traverse_claims(&self, subject_id: &str, space: Option<&str>) -> Result<Vec<CortexClaim>> {
        let conn = self.conn.lock().unwrap();
        let target_space = space.unwrap_or("atlas-memory");

        let mut stmt = conn.prepare(
            "SELECT id, space_id, space_slug, subject_entity_id, predicate, object_entity_id, literal_value_json, confidence, metadata_json, retracted, created_at
             FROM claims
             WHERE (subject_entity_id = ?1 OR object_entity_id = ?1) AND space_slug = ?2 AND retracted = 0"
        )?;

        let rows = stmt.query_map(params![subject_id, target_space], |row| {
            let lit_str: Option<String> = row.get(6)?;
            let meta_str: String = row.get(8)?;
            Ok(CortexClaim {
                id: row.get(0)?,
                space_id: row.get(1)?,
                space_slug: row.get(2)?,
                subject_entity_id: row.get(3)?,
                predicate: row.get(4)?,
                object_entity_id: row.get(5)?,
                literal_value: lit_str.and_then(|s| serde_json::from_str(&s).ok()),
                confidence: row.get(7)?,
                metadata: serde_json::from_str(&meta_str).unwrap_or_else(|_| serde_json::json!({})),
                retracted: row.get::<_, i64>(9)? != 0,
                created_at: row.get(10)?,
            })
        })?;

        let mut claims = Vec::new();
        for r in rows {
            claims.push(r?);
        }
        Ok(claims)
    }

    pub fn import_entity_raw(&self, entity: &CortexEntity, embedding: Option<&[f32]>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let embedding_blob = embedding.map(embedding_to_bytes);
        let aliases_json = serde_json::to_string(&entity.aliases).unwrap_or_else(|_| "[]".to_string());
        let metadata_json = serde_json::to_string(&entity.metadata).unwrap_or_else(|_| "{}".to_string());

        conn.execute(
            "INSERT INTO entities (id, space_id, space_slug, entity_type, canonical_name, content, aliases_json, metadata_json, confidence, revision, retracted, valid_from, valid_to, created_at, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(id) DO UPDATE SET
                canonical_name = excluded.canonical_name,
                content = excluded.content,
                aliases_json = excluded.aliases_json,
                metadata_json = excluded.metadata_json,
                confidence = excluded.confidence,
                revision = excluded.revision,
                retracted = excluded.retracted,
                created_at = excluded.created_at,
                embedding = COALESCE(excluded.embedding, entities.embedding)",
            params![
                entity.id,
                entity.space_id,
                entity.space_slug,
                entity.entity_type,
                entity.canonical_name,
                entity.content,
                aliases_json,
                metadata_json,
                entity.confidence,
                entity.revision,
                if entity.retracted { 1 } else { 0 },
                entity.valid_from,
                entity.valid_to,
                entity.created_at,
                embedding_blob,
            ],
        )?;

        let _ = conn.execute("DELETE FROM entities_fts WHERE id = ?1", params![entity.id]);
        let _ = conn.execute(
            "INSERT INTO entities_fts (id, canonical_name, content, space_slug) VALUES (?1, ?2, ?3, ?4)",
            params![entity.id, entity.canonical_name, entity.content, entity.space_slug],
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_db_crud() {
        let db = Database::open_in_memory().expect("open in memory");

        let input = EntityWriteInput {
            id: None,
            space: "atlas-memory".to_string(),
            entity_type: "discovery".to_string(),
            canonical_name: "test_cortex_rebuild".to_string(),
            content: "Testing the native Rust cortex-rs database engine".to_string(),
            aliases: vec!["cortex_test".to_string()],
            metadata: serde_json::json!({"test": true}),
            confidence: 1.0,
            valid_from: None,
            valid_to: None,
            external_id: None,
        };

        let dummy_emb = vec![0.5f32; 768];
        let receipt = db.upsert_entity(&input, Some(&dummy_emb)).expect("upsert entity");
        assert_eq!(receipt.operation, "entity.upsert");

        let fetched = db.get_entity("test_cortex_rebuild", Some("atlas-memory")).expect("get entity");
        assert!(fetched.is_some());
        let entity = fetched.unwrap();
        assert_eq!(entity.canonical_name, "test_cortex_rebuild");
        assert_eq!(entity.aliases.len(), 1);

        let search_results = db.search_entities("Rust", Some("atlas-memory"), 10).expect("search");
        assert!(!search_results.is_empty());
        assert_eq!(search_results[0].entity.canonical_name, "test_cortex_rebuild");

        let recall_results = db.recall_entities("Rust", Some(&dummy_emb), Some("atlas-memory"), 5).expect("recall");
        assert!(!recall_results.is_empty());
        assert!(recall_results[0].final_score > 0.5);
    }
}

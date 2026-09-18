use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use thiserror::Error;
use uuid::Uuid;

use crate::geometry::{hex_distance, HexCoord, HEX_DIRECTIONS, HEX_DIRECTION_LABELS};

#[derive(Debug, Error)]
pub enum HiveError {
    #[error("Coordinate ({q}, {r}) is already occupied by comb '{comb_id}'")]
    Collision { q: i32, r: i32, comb_id: String },
    #[error("Parent comb '{0}' not found")]
    ParentNotFound(String),
    #[error("SQLite database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiveComb {
    pub id: String,
    pub q: i32,
    pub r: i32,
    pub author: String,
    pub role: String,
    pub content: String,
    pub intent: String,
    pub parent_id: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub neighbors: Vec<NeighborLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighborLink {
    pub id: String,
    pub q: i32,
    pub r: i32,
    pub direction: usize,
    pub direction_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min_q: i32,
    pub max_q: i32,
    pub min_r: i32,
    pub max_r: i32,
    pub count: usize,
    pub radius: i32,
    pub revision: usize,
    pub message_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaceCombInput {
    pub q: Option<i32>,
    pub r: Option<i32>,
    pub author: String,
    pub role: Option<String>,
    pub content: String,
    pub intent: Option<String>,
    pub parent_id: Option<String>,
}

pub struct CombStore {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl CombStore {
    pub fn open(path: &Path) -> Result<Self, HiveError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        let _: Result<String, _> = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0));
        conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        let store = Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
        };
        store.init_schema()?;
        Ok(store)
    }

    pub fn open_default() -> Result<Self, HiveError> {
        let path_str = std::env::var("HIVE_DB_PATH")
            .unwrap_or_else(|_| "/home/drakestapleton/.config/cortex/hive.db".to_string());
        let path = PathBuf::from(path_str);
        Self::open(&path)
    }

    pub fn open_in_memory() -> Result<Self, HiveError> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        };
        store.init_schema()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn init_schema(&self) -> Result<(), HiveError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS hive_combs (
                id TEXT PRIMARY KEY,
                q INTEGER NOT NULL,
                r INTEGER NOT NULL,
                author TEXT NOT NULL,
                role TEXT NOT NULL DEFAULT 'agent',
                content TEXT NOT NULL,
                intent TEXT NOT NULL DEFAULT 'independent',
                parent_id TEXT,
                created_at TEXT NOT NULL,
                UNIQUE(q, r)
            );
            CREATE INDEX IF NOT EXISTS idx_hive_combs_qr ON hive_combs(q, r);
            CREATE INDEX IF NOT EXISTS idx_hive_combs_created ON hive_combs(created_at);
            CREATE INDEX IF NOT EXISTS idx_hive_combs_parent ON hive_combs(parent_id);"
        )?;

        // Seed genesis comb if table is empty
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM hive_combs", [], |r| r.get(0))?;
        if count == 0 {
            let genesis_id = "comb-genesis-00000000";
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO hive_combs (id, q, r, author, role, content, intent, parent_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    genesis_id,
                    0,
                    0,
                    "AIEN Genesis",
                    "coordinator",
                    "AIEN Sovereign Hive Online · Hexagonal Honeycomb Active",
                    "independent",
                    None::<String>,
                    now,
                ],
            )?;
        }

        Ok(())
    }

    pub fn get_comb(&self, id: &str) -> Result<Option<HiveComb>, HiveError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, q, r, author, role, content, intent, parent_id, created_at
             FROM hive_combs WHERE id = ?1"
        )?;
        let row = stmt.query_row(params![id], |r| {
            Ok(HiveComb {
                id: r.get(0)?,
                q: r.get(1)?,
                r: r.get(2)?,
                author: r.get(3)?,
                role: r.get(4)?,
                content: r.get(5)?,
                intent: r.get(6)?,
                parent_id: r.get(7)?,
                created_at: r.get(8)?,
                neighbors: Vec::new(),
            })
        }).optional()?;

        Ok(row)
    }

    pub fn get_comb_at(&self, q: i32, r: i32) -> Result<Option<HiveComb>, HiveError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, q, r, author, role, content, intent, parent_id, created_at
             FROM hive_combs WHERE q = ?1 AND r = ?2"
        )?;
        let row = stmt.query_row(params![q, r], |row| {
            Ok(HiveComb {
                id: row.get(0)?,
                q: row.get(1)?,
                r: row.get(2)?,
                author: row.get(3)?,
                role: row.get(4)?,
                content: row.get(5)?,
                intent: row.get(6)?,
                parent_id: row.get(7)?,
                created_at: row.get(8)?,
                neighbors: Vec::new(),
            })
        }).optional()?;

        Ok(row)
    }

    pub fn get_cells(&self) -> Result<(Vec<HiveComb>, BoundingBox), HiveError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, q, r, author, role, content, intent, parent_id, created_at
             FROM hive_combs ORDER BY created_at ASC"
        )?;

        let rows = stmt.query_map([], |r| {
            Ok(HiveComb {
                id: r.get(0)?,
                q: r.get(1)?,
                r: r.get(2)?,
                author: r.get(3)?,
                role: r.get(4)?,
                content: r.get(5)?,
                intent: r.get(6)?,
                parent_id: r.get(7)?,
                created_at: r.get(8)?,
                neighbors: Vec::new(),
            })
        })?;

        let mut combs = Vec::new();
        let mut min_q = 0;
        let mut max_q = 0;
        let mut min_r = 0;
        let mut max_r = 0;
        let mut max_radius = 5;

        for (i, comb_res) in rows.enumerate() {
            let comb = comb_res?;
            if i == 0 {
                min_q = comb.q;
                max_q = comb.q;
                min_r = comb.r;
                max_r = comb.r;
            } else {
                min_q = min_q.min(comb.q);
                max_q = max_q.max(comb.q);
                min_r = min_r.min(comb.r);
                max_r = max_r.max(comb.r);
            }
            let d = hex_distance(comb.q, comb.r, 0, 0);
            if d > max_radius {
                max_radius = d;
            }
            combs.push(comb);
        }

        // Build coordinate lookup table for neighbor linking
        let coord_map: std::collections::HashMap<(i32, i32), (String, usize)> = combs
            .iter()
            .enumerate()
            .map(|(idx, c)| ((c.q, c.r), (c.id.clone(), idx)))
            .collect();

        // Calculate neighbor links for each comb
        for comb in &mut combs {
            let mut neighbors = Vec::new();
            for (dir_idx, (dq, dr)) in HEX_DIRECTIONS.iter().enumerate() {
                let n_coord = (comb.q + dq, comb.r + dr);
                if let Some((n_id, _)) = coord_map.get(&n_coord) {
                    neighbors.push(NeighborLink {
                        id: n_id.clone(),
                        q: n_coord.0,
                        r: n_coord.1,
                        direction: dir_idx,
                        direction_label: HEX_DIRECTION_LABELS[dir_idx].to_string(),
                    });
                }
            }
            comb.neighbors = neighbors;
        }

        let count = combs.len();
        let bounds = BoundingBox {
            min_q,
            max_q,
            min_r,
            max_r,
            count,
            radius: max_radius,
            revision: count,
            message_count: count,
        };

        Ok((combs, bounds))
    }

    /// Finds the first available unoccupied coordinate.
    /// If parent_id is given, searches adjacent neighbors of parent first.
    pub fn find_free_coordinate(&self, parent_id: Option<&str>) -> Result<HexCoord, HiveError> {
        let conn = self.conn.lock().unwrap();
        Self::find_free_coordinate_with_conn(&conn, parent_id)
    }

    /// Internal coordinate finder operating under an already locked database connection.
    fn find_free_coordinate_with_conn(
        conn: &rusqlite::Connection,
        parent_id: Option<&str>,
    ) -> Result<HexCoord, HiveError> {
        let mut stmt = conn.prepare("SELECT q, r FROM hive_combs")?;
        let occupied_coords: HashSet<(i32, i32)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<HashSet<_>, _>>()?;

        let center = if let Some(pid) = parent_id {
            let mut parent_stmt = conn.prepare("SELECT q, r FROM hive_combs WHERE id = ?1")?;
            let parent_coord: Option<(i32, i32)> = parent_stmt
                .query_row(params![pid], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;

            match parent_coord {
                Some((pq, pr)) => HexCoord::new(pq, pr),
                None => HexCoord::new(0, 0),
            }
        } else {
            HexCoord::new(0, 0)
        };

        // Check center itself first if empty
        if !occupied_coords.contains(&(center.q, center.r)) {
            return Ok(center);
        }

        // 1. Check all 6 immediate neighbors in canonical order
        for (dq, dr) in HEX_DIRECTIONS {
            let cand = (center.q + dq, center.r + dr);
            if !occupied_coords.contains(&cand) {
                return Ok(HexCoord::new(cand.0, cand.1));
            }
        }

        // 2. Search outward rings around center up to radius 50
        for ring_radius in 2..=50 {
            let mut current = HexCoord::new(
                center.q + HEX_DIRECTIONS[4].0 * ring_radius,
                center.r + HEX_DIRECTIONS[4].1 * ring_radius,
            );
            for (dir_idx, _) in HEX_DIRECTIONS.iter().enumerate() {
                for _ in 0..ring_radius {
                    if !occupied_coords.contains(&(current.q, current.r)) {
                        return Ok(current);
                    }
                    let next_dir = HEX_DIRECTIONS[dir_idx];
                    current = HexCoord::new(current.q + next_dir.0, current.r + next_dir.1);
                }
            }
        }

        Ok(HexCoord::new(0, 0))
    }

    pub fn place_comb(&self, input: PlaceCombInput) -> Result<HiveComb, HiveError> {
        let conn = self.conn.lock().unwrap();

        let (q, r) = match (input.q, input.r) {
            (Some(q), Some(r)) => (q, r),
            _ => {
                let coord = Self::find_free_coordinate_with_conn(&conn, input.parent_id.as_deref())?;
                (coord.q, coord.r)
            }
        };

        // 1. Collision check
        let mut check_stmt = conn.prepare("SELECT id FROM hive_combs WHERE q = ?1 AND r = ?2")?;
        let existing_id: Option<String> = check_stmt.query_row(params![q, r], |row| row.get(0)).optional()?;
        if let Some(id) = existing_id {
            return Err(HiveError::Collision { q, r, comb_id: id });
        }

        // 2. Neighbor linking
        let mut neighbors = Vec::new();
        let mut neighbor_stmt = conn.prepare("SELECT id FROM hive_combs WHERE q = ?1 AND r = ?2")?;
        for (dir_idx, (dq, dr)) in HEX_DIRECTIONS.iter().enumerate() {
            let n_q = q + dq;
            let n_r = r + dr;
            if let Some(n_id) = neighbor_stmt.query_row(params![n_q, n_r], |row| row.get(0)).optional()? {
                neighbors.push(NeighborLink {
                    id: n_id,
                    q: n_q,
                    r: n_r,
                    direction: dir_idx,
                    direction_label: HEX_DIRECTION_LABELS[dir_idx].to_string(),
                });
            }
        }

        // 3. Verify parent_id if supplied
        if let Some(ref pid) = input.parent_id {
            let mut parent_check = conn.prepare("SELECT 1 FROM hive_combs WHERE id = ?1")?;
            let parent_exists: Option<i32> = parent_check.query_row(params![pid], |_| Ok(1)).optional()?;
            if parent_exists.is_none() {
                // Parent does not exist yet; accepted as unanchored
            }
        }

        let id = format!("comb-{}", Uuid::new_v4());
        let role = input.role.unwrap_or_else(|| "agent".to_string());
        let intent = input.intent.unwrap_or_else(|| "independent".to_string());
        let created_at = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO hive_combs (id, q, r, author, role, content, intent, parent_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                &id,
                q,
                r,
                &input.author,
                &role,
                &input.content,
                &intent,
                &input.parent_id,
                &created_at,
            ],
        )?;

        Ok(HiveComb {
            id,
            q,
            r,
            author: input.author,
            role,
            content: input.content,
            intent,
            parent_id: input.parent_id,
            created_at,
            neighbors,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_store_genesis_and_get_cells() {
        let store = CombStore::open_in_memory().expect("open in memory store");
        let (cells, bounds) = store.get_cells().expect("get cells");
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].q, 0);
        assert_eq!(cells[0].r, 0);
        assert_eq!(cells[0].author, "AIEN Genesis");
        assert_eq!(bounds.count, 1);
        assert_eq!(bounds.min_q, 0);
        assert_eq!(bounds.max_q, 0);
    }

    #[test]
    fn test_place_comb_collision_check() {
        let store = CombStore::open_in_memory().expect("open in memory store");
        
        // Attempt to place at (0, 0) where genesis comb lives -> must trigger Collision!
        let collision_result = store.place_comb(PlaceCombInput {
            q: Some(0),
            r: Some(0),
            author: "Tester".to_string(),
            role: None,
            content: "Collision test".to_string(),
            intent: None,
            parent_id: None,
        });

        match collision_result {
            Err(HiveError::Collision { q, r, comb_id }) => {
                assert_eq!(q, 0);
                assert_eq!(r, 0);
                assert_eq!(comb_id, "comb-genesis-00000000");
            }
            other => panic!("Expected collision error, got: {:?}", other),
        }
    }

    #[test]
    fn test_place_comb_with_neighbors_linking() {
        let store = CombStore::open_in_memory().expect("open in memory store");

        // Place east neighbor of genesis (1, 0)
        let placed = store.place_comb(PlaceCombInput {
            q: Some(1),
            r: Some(0),
            author: "AIEN Subagent".to_string(),
            role: Some("researcher".to_string()),
            content: "Subagent completed research task".to_string(),
            intent: Some("join".to_string()),
            parent_id: Some("comb-genesis-00000000".to_string()),
        }).expect("place east neighbor");

        assert_eq!(placed.q, 1);
        assert_eq!(placed.r, 0);
        assert_eq!(placed.neighbors.len(), 1);
        assert_eq!(placed.neighbors[0].id, "comb-genesis-00000000");
        assert_eq!(placed.neighbors[0].direction_label, "west"); // Genesis is west of (1, 0)

        // Verify get_cells links both ways
        let (cells, bounds) = store.get_cells().expect("get cells");
        assert_eq!(cells.len(), 2);
        assert_eq!(bounds.count, 2);

        let genesis = cells.iter().find(|c| c.q == 0 && c.r == 0).unwrap();
        assert_eq!(genesis.neighbors.len(), 1);
        assert_eq!(genesis.neighbors[0].direction_label, "east");
        assert_eq!(genesis.neighbors[0].id, placed.id);
    }

    #[test]
    fn test_auto_place_comb_around_parent() {
        let store = CombStore::open_in_memory().expect("open in memory store");

        // Auto place with parent_id -> should place at first free neighbor of genesis: (1, 0) East
        let auto1 = store.place_comb(PlaceCombInput {
            q: None,
            r: None,
            author: "Subagent 1".to_string(),
            role: Some("coder".to_string()),
            content: "First code commit".to_string(),
            intent: Some("branch".to_string()),
            parent_id: Some("comb-genesis-00000000".to_string()),
        }).expect("auto place 1");

        assert_eq!(auto1.q, 1);
        assert_eq!(auto1.r, 0);

        // Auto place again -> should place at next free neighbor: (1, -1) Northeast
        let auto2 = store.place_comb(PlaceCombInput {
            q: None,
            r: None,
            author: "Subagent 2".to_string(),
            role: Some("tester".to_string()),
            content: "Tests passed".to_string(),
            intent: Some("join".to_string()),
            parent_id: Some("comb-genesis-00000000".to_string()),
        }).expect("auto place 2");

        assert_eq!(auto2.q, 1);
        assert_eq!(auto2.r, -1);
    }

    #[test]
    fn test_concurrent_auto_place_comb() {
        use std::sync::Arc;
        use std::thread;

        let store = Arc::new(CombStore::open_in_memory().expect("in memory store"));
        let mut handles = Vec::new();

        for i in 0..12 {
            let s = Arc::clone(&store);
            handles.push(thread::spawn(move || {
                s.place_comb(PlaceCombInput {
                    q: None,
                    r: None,
                    author: format!("Agent-{}", i),
                    role: Some("subagent".to_string()),
                    content: format!("Concurrent message {}", i),
                    intent: Some("branch".to_string()),
                    parent_id: Some("comb-genesis-00000000".to_string()),
                })
            }));
        }

        for h in handles {
            let res = h.join().expect("thread join");
            assert!(res.is_ok(), "Concurrent auto placement must never collide: {:?}", res.err());
        }

        let (cells, bounds) = store.get_cells().expect("get cells");
        assert_eq!(cells.len(), 13);
        assert_eq!(bounds.count, 13);
    }
}

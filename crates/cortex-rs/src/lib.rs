//! Sovereign Native Memory Engine in Rust (Cortex)
//! Exposes in-process database and memory retrieval interfaces for the AIEN runtime.

pub mod auth;
pub mod bench;
pub mod db;
pub mod embeddings;
pub mod handlers;
pub mod models;

pub use db::Database;
pub use models::*;
use std::sync::Arc;

/// In-process Cortex memory runtime avoiding localhost HTTP serialization overhead.
pub struct CortexRuntime {
    db: Arc<Database>,
}

impl CortexRuntime {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn in_memory() -> Result<Self, String> {
        let db = Database::open_in_memory().map_err(|e| e.to_string())?;
        Ok(Self { db: Arc::new(db) })
    }

    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self, String> {
        let db = Database::open(path).map_err(|e| e.to_string())?;
        Ok(Self { db: Arc::new(db) })
    }

    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }
}

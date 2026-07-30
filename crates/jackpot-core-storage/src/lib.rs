use jackpot_core_common::CoreResult;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

pub struct Database {
    conn: Arc<Mutex<Option<Connection>>>,
}

impl Database {
    pub fn open(data_dir: &Path) -> CoreResult<Self> {
        std::fs::create_dir_all(data_dir).map_err(|e| {
            jackpot_core_common::CoreError::Storage(Box::new(e))
        })?;
        let db_path = data_dir.join("jackpot.db");
        let conn = Connection::open(&db_path).map_err(|e| {
            jackpot_core_common::CoreError::Storage(Box::new(e))
        })?;
        Ok(Self {
            conn: Arc::new(Mutex::new(Some(conn))),
        })
    }

    pub fn migrate(&self) -> CoreResult<()> {
        let guard = self.conn.lock().map_err(|e| {
            jackpot_core_common::CoreError::Internal(e.to_string())
        })?;
        if let Some(ref conn) = *guard {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS _migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );",
            )
            .map_err(|e| jackpot_core_common::CoreError::Storage(Box::new(e)))?;
        }
        Ok(())
    }

    pub fn close(self) -> CoreResult<()> {
        let mut guard = self.conn.lock().map_err(|e| {
            jackpot_core_common::CoreError::Internal(e.to_string())
        })?;
        if let Some(conn) = guard.take() {
            conn.close()
                .map_err(|(_conn, e)| jackpot_core_common::CoreError::Storage(Box::new(e)))?;
        }
        Ok(())
    }

    pub fn connection(&self) -> CoreResult<MutexGuard<'_, Option<Connection>>> {
        self.conn.lock().map_err(|e| {
            jackpot_core_common::CoreError::Internal(e.to_string())
        })
    }
}

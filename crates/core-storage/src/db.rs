use core_common::CoreResult;
use std::path::Path;
use std::sync::Mutex;

use crate::migrations;

pub struct Database {
    conn: Mutex<Option<rusqlite::Connection>>,
}

impl Database {
    pub fn open(data_dir: &Path) -> CoreResult<Self> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;

        let db_path = data_dir.join("jackpot.db");
        let conn = rusqlite::Connection::open(&db_path)
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;

        Ok(Self {
            conn: Mutex::new(Some(conn)),
        })
    }

    pub fn migrate(&self) -> CoreResult<()> {
        let guard = self
            .conn
            .lock()
            .map_err(|e| core_common::CoreError::Internal(e.to_string()))?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| core_common::CoreError::Internal("database closed".into()))?;
        migrations::run_migrations(conn)
    }

    pub fn execute<F, T>(&self, f: F) -> CoreResult<T>
    where
        F: FnOnce(&rusqlite::Connection) -> CoreResult<T>,
    {
        let guard = self
            .conn
            .lock()
            .map_err(|e| core_common::CoreError::Internal(e.to_string()))?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| core_common::CoreError::Internal("database closed".into()))?;
        f(conn)
    }

    pub fn close(self) -> CoreResult<()> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|e| core_common::CoreError::Internal(e.to_string()))?;
        if let Some(conn) = guard.take() {
            conn.close()
                .map_err(|(_conn, e)| core_common::CoreError::Storage(Box::new(e)))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jackpot-test-{}", uuid::Uuid::new_v4()));
        dir
    }

    #[test]
    fn test_open_and_migrate() {
        let dir = temp_dir();
        let db = Database::open(&dir).unwrap();
        db.migrate().unwrap();

        let version: i32 = db
            .execute(|conn| {
                conn.query_row("SELECT version FROM _schema_version", [], |row| row.get(0))
                    .map_err(|e| core_common::CoreError::Storage(Box::new(e)))
            })
            .unwrap();
        assert_eq!(version, 1);

        db.close().unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_hosts_table_exists() {
        let dir = temp_dir();
        let db = Database::open(&dir).unwrap();
        db.migrate().unwrap();

        db.execute(|conn| {
            conn.execute(
                "INSERT INTO hosts (id, name, address, port, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                ["test-id", "test-host", "192.168.1.1", "22", "2026-01-01", "2026-01-01"],
            )
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
            Ok(())
        })
        .unwrap();

        db.close().unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }
}

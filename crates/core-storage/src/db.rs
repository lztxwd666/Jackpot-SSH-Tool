//! 数据库连接管理模块

use core_common::CoreResult;
use std::path::Path;
use std::sync::Mutex;

use crate::migrations;

/// SQLite 数据库连接管理器
/// 内部使用 Mutex 保护连接，确保跨线程安全访问
/// 连接使用 Option 包裹以支持 close() 后置空
pub struct Database {
    conn: Mutex<Option<rusqlite::Connection>>,
}

impl Database {
    /// 在指定目录创建或打开 SQLite 数据库文件
    /// 自动启用 WAL 日志模式和 foreign key 约束
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

    /// 执行 schema 迁移，将数据库升级到最新版本
    pub fn migrate(&self) -> CoreResult<()> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|e| core_common::CoreError::Internal(e.to_string()))?;
        let conn = guard
            .as_mut()
            .ok_or_else(|| core_common::CoreError::Internal("database closed".into()))?;
        migrations::run_migrations(conn)
    }

    /// 在数据库连接上执行任意操作
    /// 这是唯一暴露连接的接口，外部无法直接获取 MutexGuard，保证锁使用安全
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

    /// 关闭数据库连接并释放资源，消耗 self 以防后续误用
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
        std::env::temp_dir().join(format!("jackpot-test-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn test_open_and_migrate() {
        let dir = temp_dir();
        let db = Database::open(&dir).unwrap();
        db.migrate().unwrap();

        let version: i32 = db
            .execute(|conn| {
                conn.query_row("SELECT MAX(version) FROM _schema_version", [], |row| row.get(0))
                    .map_err(|e| core_common::CoreError::Storage(Box::new(e)))
            })
            .unwrap();
        assert_eq!(version, 3);

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

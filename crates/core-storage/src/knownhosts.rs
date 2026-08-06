//! KnownHosts 的 SQLite 持久化实现
//! 将主机密钥信息存储到 known_hosts 表中

use std::sync::Arc;

use crate::db::Database;

/// 基于 SQLite 的 KnownHosts Provider 实现
pub struct SqliteKnownHosts {
    db: Arc<Database>,
}

impl SqliteKnownHosts {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl core_common::knownhosts::KnownHostsProvider for SqliteKnownHosts {
    fn find_host_key(
        &self,
        host: &str,
        port: u16,
        key_type: &str,
    ) -> core_common::CoreResult<Option<core_common::HostKeyInfo>> {
        let host_owned = host.to_string();
        let key_type_owned = key_type.to_string();
        self.db.execute(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT key_type, fingerprint FROM known_hosts \
                     WHERE host = ?1 AND port = ?2 AND key_type = ?3",
                )
                .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;

            let rows: Vec<Result<core_common::HostKeyInfo, _>> = stmt
                .query_map(rusqlite::params![host_owned, port, key_type_owned], |row| {
                    Ok(core_common::HostKeyInfo {
                        host: host_owned.clone(),
                        port,
                        key_type: row.get(0)?,
                        fingerprint: row.get(1)?,
                    })
                })
                .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?
                .collect();

            // 行解析错误不得静默吞掉（误判为"未知主机"会走 TOFU 确认流程并覆盖损坏行）：
            // 明确区分 无行（Ok(None)）/ 行解析错误（Err）/ 命中（Ok(Some)）
            Ok(match rows.into_iter().next() {
                Some(Err(e)) => {
                    return Err(core_common::CoreError::Storage(Box::new(e)));
                }
                Some(Ok(host)) => Some(host),
                None => None,
            })
        })
    }

    fn store_host_key(&self, info: &core_common::HostKeyInfo) -> core_common::CoreResult<()> {
        let host = info.host.clone();
        let key_type = info.key_type.clone();
        let fingerprint = info.fingerprint.clone();
        let port = info.port;

        self.db.execute(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO known_hosts (host, port, key_type, fingerprint) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![host, port, key_type, fingerprint],
            )
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
            Ok(())
        })
    }

    fn remove_host_key(&self, host: &str, port: u16) -> core_common::CoreResult<()> {
        let host_owned = host.to_string();
        self.db.execute(move |conn| {
            conn.execute(
                "DELETE FROM known_hosts WHERE host = ?1 AND port = ?2",
                rusqlite::params![host_owned, port],
            )
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use core_common::knownhosts::KnownHostsProvider;

    /// 临时目录守卫：测试结束（含 panic）时自动清理
    struct TempDirGuard(std::path::PathBuf);
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn setup_db() -> (Arc<Database>, TempDirGuard) {
        let dir =
            std::env::temp_dir().join(format!("jackpot-test-knownhosts-{}", uuid::Uuid::new_v4()));
        let db = Database::open(&dir).unwrap();
        db.migrate().unwrap();
        (Arc::new(db), TempDirGuard(dir))
    }

    #[test]
    fn test_store_and_find_host_key() {
        let (db, _dir) = setup_db();
        let provider = SqliteKnownHosts::new(db.clone());

        let info = core_common::HostKeyInfo::new(
            "example.com".into(),
            22,
            "ssh-rsa".into(),
            "SHA256:abcdef1234567890".into(),
        );

        provider.store_host_key(&info).unwrap();

        let found = provider.find_host_key("example.com", 22, "ssh-rsa").unwrap().unwrap();
        assert_eq!(found.host, "example.com");
        assert_eq!(found.key_type, "ssh-rsa");
        assert_eq!(found.fingerprint, "SHA256:abcdef1234567890");

        // 按 key_type 过滤：同主机其他密钥类型不命中（V4 起多密钥并存）
        assert!(provider.find_host_key("example.com", 22, "ssh-ed25519").unwrap().is_none());
    }

    #[test]
    fn test_remove_host_key() {
        let (db, _dir) = setup_db();
        let provider = SqliteKnownHosts::new(db.clone());

        let info = core_common::HostKeyInfo::new(
            "test.com".into(),
            2222,
            "ssh-ed25519".into(),
            "SHA256:deadbeef".into(),
        );
        provider.store_host_key(&info).unwrap();
        assert!(provider.find_host_key("test.com", 2222, "ssh-ed25519").unwrap().is_some());

        provider.remove_host_key("test.com", 2222).unwrap();
        assert!(provider.find_host_key("test.com", 2222, "ssh-ed25519").unwrap().is_none());
    }
}

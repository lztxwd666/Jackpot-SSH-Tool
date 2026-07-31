//! HostRepository 的 SQLite 持久化实现
//! 将主机配置信息存储到 hosts 表中

use std::sync::Arc;

use core_common::{CoreResult, Host, HostId, HostRepository};
use crate::db::Database;

/// 基于 SQLite 的 HostRepository 实现
pub struct SqliteHostRepository {
    db: Arc<Database>,
}

impl SqliteHostRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    fn row_to_host(row: &rusqlite::Row<'_>) -> rusqlite::Result<Host> {
        let id_str: String = row.get(0)?;
        let uuid_val = uuid::Uuid::parse_str(&id_str)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
        let host_id = host_id_from_uuid(uuid_val);
        let favorite_int: i32 = row.get(6)?;
        Ok(Host {
            id: host_id,
            name: row.get(1)?,
            address: row.get(2)?,
            port: row.get::<_, i32>(3)? as u16,
            username: row.get(4)?,
            auth_type: row.get(5)?,
            group_name: row.get(7)?,
            favorite: favorite_int != 0,
            notes: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    }
}

/// 从 UUID 值重建 HostId（绕过 private 字段访问）
/// 通过 serde 序列化/反序列化来实现转换
fn host_id_from_uuid(u: uuid::Uuid) -> HostId {
    let json = serde_json::to_string(&u).unwrap();
    serde_json::from_str(&json).unwrap()
}

impl HostRepository for SqliteHostRepository {
    fn list_all(&self) -> CoreResult<Vec<Host>> {
        self.db.execute(|conn| {
            let mut stmt = conn
                .prepare("SELECT id, name, address, port, username, auth_type, favorite, group_name, notes, created_at, updated_at FROM hosts ORDER BY name")
                .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;

            let rows = stmt
                .query_map([], Self::row_to_host)
                .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;

            let mut hosts = Vec::new();
            for row in rows {
                let host = row.map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
                hosts.push(host);
            }
            Ok(hosts)
        })
    }

    fn find_by_id(&self, id: &HostId) -> CoreResult<Option<Host>> {
        let id_str = id.to_string();
        self.db.execute(|conn| {
            let mut stmt = conn
                .prepare("SELECT id, name, address, port, username, auth_type, favorite, group_name, notes, created_at, updated_at FROM hosts WHERE id = ?1")
                .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;

            let mut rows = stmt
                .query_map(rusqlite::params![id_str], Self::row_to_host)
                .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;

            match rows.next() {
                Some(row) => {
                    let host = row.map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
                    Ok(Some(host))
                }
                None => Ok(None),
            }
        })
    }

    fn save(&self, host: &Host) -> CoreResult<()> {
        let id = host.id.to_string();
        let name = host.name.clone();
        let address = host.address.clone();
        let port = host.port as i32;
        let username = host.username.clone();
        let auth_type = host.auth_type.clone();
        let group_name = host.group_name.clone();
        let favorite = if host.favorite { 1 } else { 0 };
        let notes = host.notes.clone();
        let now = chrono_now();
        let created_at = if host.created_at.is_empty() { now.clone() } else { host.created_at.clone() };
        let updated_at = now;

        self.db.execute(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO hosts (id, name, address, port, username, auth_type, favorite, group_name, notes, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![id, name, address, port, username, auth_type, favorite, group_name, notes, created_at, updated_at],
            )
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
            Ok(())
        })
    }

    fn delete(&self, id: &HostId) -> CoreResult<()> {
        let id_str = id.to_string();
        self.db.execute(move |conn| {
            conn.execute("DELETE FROM hosts WHERE id = ?1", rusqlite::params![id_str])
                .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
            Ok(())
        })
    }

    fn search(&self, query: &str) -> CoreResult<Vec<Host>> {
        let pattern = format!("%{}%", query);
        self.db.execute(|conn| {
            let mut stmt = conn
                .prepare("SELECT id, name, address, port, username, auth_type, favorite, group_name, notes, created_at, updated_at FROM hosts WHERE name LIKE ?1 OR address LIKE ?1")
                .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;

            let rows = stmt
                .query_map(rusqlite::params![pattern], Self::row_to_host)
                .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;

            let mut hosts = Vec::new();
            for row in rows {
                let host = row.map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
                hosts.push(host);
            }
            Ok(hosts)
        })
    }
}

/// 生成当前 UTC 时间的 ISO 8601 字符串（SQLite 兼容）
fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // 简单的 UTC ISO 8601 格式：YYYY-MM-DDTHH:MM:SS
    let total_days = (secs / 86400) as i64;
    let time_of_day = (secs % 86400) as u32;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // 计算日期从 UNIX epoch 开始的天数转换到年月日
    let (year, month, day) = days_to_ymd(total_days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_ymd(total_days: i64) -> (i64, u32, u32) {
    let days = total_days + 719468; // 偏移到 0000-03-01
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = days - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    let y_adj = y + (if m <= 2 { 1 } else { 0 });
    (y_adj, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn setup_db() -> Arc<Database> {
        let dir = std::env::temp_dir().join(format!("jackpot-test-hostrepo-{}", uuid::Uuid::new_v4()));
        let db = Database::open(&dir).unwrap();
        db.migrate().unwrap();
        Arc::new(db)
    }

    fn make_host(name: &str, address: &str) -> Host {
        Host {
            id: HostId::new(),
            name: name.to_string(),
            address: address.to_string(),
            port: 22,
            username: "root".to_string(),
            auth_type: "password".to_string(),
            group_name: "".to_string(),
            favorite: false,
            notes: "".to_string(),
            created_at: "".to_string(),
            updated_at: "".to_string(),
        }
    }

    #[test]
    fn test_save_and_list() {
        let db = setup_db();
        let repo = SqliteHostRepository::new(db.clone());

        let host = make_host("test-server", "10.0.0.1");
        repo.save(&host).unwrap();

        let list = repo.list_all().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-server");
        assert_eq!(list[0].address, "10.0.0.1");
    }

    #[test]
    fn test_save_and_find_by_id() {
        let db = setup_db();
        let repo = SqliteHostRepository::new(db.clone());

        let host = make_host("web-01", "192.168.1.10");
        let id = host.id;
        repo.save(&host).unwrap();

        let found = repo.find_by_id(&id).unwrap().unwrap();
        assert_eq!(found.name, "web-01");
    }

    #[test]
    fn test_delete() {
        let db = setup_db();
        let repo = SqliteHostRepository::new(db.clone());

        let host = make_host("delete-me", "10.0.0.99");
        let id = host.id;
        repo.save(&host).unwrap();
        assert!(repo.find_by_id(&id).unwrap().is_some());

        repo.delete(&id).unwrap();
        assert!(repo.find_by_id(&id).unwrap().is_none());
    }

    #[test]
    fn test_search() {
        let db = setup_db();
        let repo = SqliteHostRepository::new(db.clone());

        repo.save(&make_host("production-db", "10.1.1.1")).unwrap();
        repo.save(&make_host("staging-db", "10.2.2.2")).unwrap();
        repo.save(&make_host("web-server", "10.3.3.3")).unwrap();

        let results = repo.search("db").unwrap();
        assert_eq!(results.len(), 2);

        let results = repo.search("10.3").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "web-server");
    }
}

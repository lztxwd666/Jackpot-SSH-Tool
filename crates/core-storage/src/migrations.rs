//! Schema 迁移模块
//! 基于版本号递增执行 SQL，支持增量升级而不破坏已有数据

use core_common::CoreResult;

/// 当前数据库 schema 版本号
const SCHEMA_VERSION: i32 = 4;

/// V1 初始 schema：hosts 表存储 SSH 主机信息，config 表存储键值对配置
/// version 设为主键，防止重复执行迁移时插入重复版本行
const MIGRATION_V1: &str = "
CREATE TABLE IF NOT EXISTS _schema_version (
    version INTEGER PRIMARY KEY NOT NULL
);

CREATE TABLE IF NOT EXISTS hosts (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    address TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 22,
    username TEXT NOT NULL DEFAULT '',
    auth_type TEXT NOT NULL DEFAULT 'password',
    group_name TEXT NOT NULL DEFAULT '',
    favorite INTEGER NOT NULL DEFAULT 0,
    notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
";

/// V2 已知主机密钥表（created_at 与 host_repo 的 ISO 格式一致：T 分隔、UTC、无时区后缀）
const MIGRATION_V2: &str = "
CREATE TABLE IF NOT EXISTS known_hosts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 22,
    key_type TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S', 'now')),
    UNIQUE(host, port)
);
";

/// V3：hosts 表增加 save_password 列（保存密码勾选标志；密码本体在 OS 凭据库，绝不在 SQLite）
const MIGRATION_V3: &str = "
ALTER TABLE hosts ADD COLUMN save_password INTEGER NOT NULL DEFAULT 0;
";

/// V4：known_hosts 唯一键扩展为 (host, port, key_type)，支持同主机多密钥类型并存
/// （旧约束下 ssh-rsa 与 ssh-ed25519 互相覆盖，密钥轮换被误报 Changed）；
/// 同时把旧行 created_at 的空格分隔格式统一为 T 分隔（与 host_repo 一致）
const MIGRATION_V4: &str = "
CREATE TABLE known_hosts_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 22,
    key_type TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S', 'now')),
    UNIQUE(host, port, key_type)
);
INSERT INTO known_hosts_new (host, port, key_type, fingerprint, created_at)
SELECT host, port, key_type, fingerprint,
    CASE WHEN instr(created_at, 'T') > 0 THEN created_at
         ELSE replace(created_at, ' ', 'T') END
FROM known_hosts;
DROP TABLE known_hosts;
ALTER TABLE known_hosts_new RENAME TO known_hosts;
";

/// 运行所有必要的迁移脚本，将 schema 从当前版本升级到 SCHEMA_VERSION
/// 仅在数据库版本低于目标版本时才执行迁移
/// 整体包在事务内：DDL 与版本行写入原子提交，中断（断电/崩溃）不会留下
/// "列已加但版本号未记录"的半迁移态（重跑时 ALTER TABLE 非幂等会失败卡死数据库）
pub fn run_migrations(conn: &mut rusqlite::Connection) -> CoreResult<()> {
    let current_version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let tx = conn
        .transaction()
        .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;

    if current_version < 1 {
        tx.execute_batch(MIGRATION_V1)
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
        tx.execute(
            "INSERT OR REPLACE INTO _schema_version (version) VALUES (?1)",
            [1],
        )
        .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
        tracing::info!("database migrated to version 1");
    }

    if current_version < 2 {
        tx.execute_batch(MIGRATION_V2)
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
        tx.execute(
            "INSERT OR REPLACE INTO _schema_version (version) VALUES (?1)",
            [2],
        )
        .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
        tracing::info!("database migrated to version 2");
    }

    if current_version < 3 {
        tx.execute_batch(MIGRATION_V3)
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
        tx.execute(
            "INSERT OR REPLACE INTO _schema_version (version) VALUES (?1)",
            [3],
        )
        .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
        tracing::info!("database migrated to version 3");
    }

    if current_version < 4 {
        tx.execute_batch(MIGRATION_V4)
            .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
        tx.execute(
            "INSERT OR REPLACE INTO _schema_version (version) VALUES (?1)",
            [SCHEMA_VERSION],
        )
        .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
        tracing::info!("database migrated to version 4");
    }

    tx.commit()
        .map_err(|e| core_common::CoreError::Storage(Box::new(e)))?;
    Ok(())
}

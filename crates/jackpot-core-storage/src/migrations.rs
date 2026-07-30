use jackpot_core_common::CoreResult;

const SCHEMA_VERSION: i32 = 1;

const MIGRATION_V1: &str = "
CREATE TABLE IF NOT EXISTS _schema_version (
    version INTEGER NOT NULL
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

pub fn run_migrations(conn: &rusqlite::Connection) -> CoreResult<()> {
    let current_version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if current_version < 1 {
        conn.execute_batch(MIGRATION_V1)
            .map_err(|e| jackpot_core_common::CoreError::Storage(Box::new(e)))?;
        conn.execute("INSERT INTO _schema_version (version) VALUES (?1)", [SCHEMA_VERSION])
            .map_err(|e| jackpot_core_common::CoreError::Storage(Box::new(e)))?;
        tracing::info!("database migrated to version 1");
    }

    Ok(())
}

//! core-storage: 持久化存储模块
//! 基于 SQLite + WAL 模式，提供数据库连接管理和 schema 迁移

pub mod db;
pub mod host_repo;
pub mod knownhosts;
pub mod migrations;

pub use db::Database;
pub use host_repo::SqliteHostRepository;
pub use knownhosts::SqliteKnownHosts;

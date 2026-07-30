//! core-storage: 持久化存储模块
//! 基于 SQLite + WAL 模式，提供数据库连接管理和 schema 迁移

pub mod db;
pub mod migrations;
pub mod knownhosts;

pub use db::Database;
pub use knownhosts::SqliteKnownHosts;

//! SSH 连接引擎模块
//! 封装 ssh2::Session，提供连接、认证和 HostKey 验证功能

pub mod auth;
pub mod connection;
pub mod hostkey;

pub use connection::SshConnection;

//! core-runtime: 运行时核心模块
//! 负责应用生命周期管理、组件注册和事件驱动架构的编排

pub mod channel;
pub mod connection_service;
pub mod keepalive;
pub mod provider;
pub mod reconnect;
pub mod repository;
pub mod runtime;
pub mod service;
pub mod session;
pub mod ssh;

pub use channel::Channel;
pub use connection_service::{ConnectionService, SshConnectionService};
pub use keepalive::spawn_keepalive;
pub use provider::Provider;
pub use reconnect::spawn_reconnect;
pub use repository::Repository;
pub use runtime::CoreRuntime;
pub use service::Service;
pub use session::Session;

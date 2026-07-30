//! core-runtime: 运行时核心模块
//! 负责应用生命周期管理、组件注册和事件驱动架构的编排

pub mod connection_service;
pub mod provider;
pub mod repository;
pub mod runtime;
pub mod service;
pub mod ssh;

pub use connection_service::{ConnectionService, SshConnectionService};
pub use provider::Provider;
pub use repository::Repository;
pub use runtime::CoreRuntime;
pub use service::Service;

//! core-common: 项目的基础类型和工具模块
//! 提供跨 crate 共享的错误类型、ID 定义、配置抽象和日志初始化

pub mod config;
pub mod credential;
pub mod error;
pub mod host;
pub mod id;
pub mod knownhosts;
pub mod ssh;

pub use config::{Config, DefaultConfig};
pub use credential::{ConfigCredentialProvider, CredentialProvider};
pub use error::{CoreError, CoreResult};
pub use host::{Host, HostRepository};
pub use id::{ChannelId, ConnectionId, HostId, SessionId, TransferId};
pub use knownhosts::KnownHostsProvider;
pub use ssh::{AuthMethod, ChannelState, ChannelType, ConnectionConfig, HostKeyInfo, PtySize, ReconnectPolicy, SessionState};

/// 初始化全局 tracing 日志订阅器
/// 优先使用环境变量 RUST_LOG，fallback 到传入的 level 参数
pub fn init_logging(level: &str) {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .init();
}

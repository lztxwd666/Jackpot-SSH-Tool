pub mod config;
pub mod error;
pub mod id;

pub use config::{Config, DefaultConfig};
pub use error::{CoreError, CoreResult};
pub use id::{ChannelId, ConnectionId, HostId, SessionId, TransferId};

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

//! 项目统一错误类型模块
//! 使用 thiserror 派生宏避免手动实现 Display/Error trait

use thiserror::Error;

/// 核心错误枚举，覆盖所有子系统可能产生的错误
/// 各变体携带不同粒度的上下文信息，便于上层展示或记录
#[derive(Error, Debug)]
pub enum CoreError {
    #[error("internal error: {0}")]
    Internal(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("storage error: {0}")]
    Storage(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// 未知主机密钥：连接被中止，等待用户确认后重试
    #[error("host key unknown: {fingerprint}")]
    HostKeyUnknown { fingerprint: String },

    /// 主机密钥已变更，连接被中止
    #[error("host key changed: {fingerprint}")]
    HostKeyChanged { fingerprint: String },
}

/// 项目统一的 Result 别名，简化函数签名
pub type CoreResult<T> = Result<T, CoreError>;

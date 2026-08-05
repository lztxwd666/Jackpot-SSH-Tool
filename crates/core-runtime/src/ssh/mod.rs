//! SSH 连接引擎模块
//! 封装 ssh2::Session，提供连接、认证和 HostKey 验证功能

pub mod auth;
pub mod connection;
pub mod hostkey;
pub mod retry;

pub use connection::SshConnection;
pub(crate) use retry::io_retry;

/// 判断错误是否为非阻塞模式下的 EAGAIN（-37 / Would block）
/// 匹配策略（按稳定性优先级）：
///   1. 数值匹配 ssh2::ErrorCode::Session(-37) —— 依赖错误码而非 Display 格式，库升级更稳
///   2. 沿 io::Error 的 source 链查找 ssh2::Error
///   3. 兜底：Display 字符串匹配（兼容未知包装层级）
pub(crate) fn is_would_block(e: &(dyn std::error::Error + 'static)) -> bool {
    if let Some(ssh_err) = e.downcast_ref::<ssh2::Error>() {
        return matches!(ssh_err.code(), ssh2::ErrorCode::Session(-37));
    }
    let mut source = e.source();
    while let Some(s) = source {
        if let Some(ssh_err) = s.downcast_ref::<ssh2::Error>() {
            return matches!(ssh_err.code(), ssh2::ErrorCode::Session(-37));
        }
        source = s.source();
    }
    let msg = format!("{e}").to_lowercase();
    msg.contains("would block") || msg.contains("-37")
}

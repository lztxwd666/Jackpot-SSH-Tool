//! 非阻塞 ssh2 I/O 的统一重试原语
//! 会话处于非阻塞模式（set_blocking(false)）时，任何操作都可能返回 EAGAIN
//! （libssh2 错误码 -37 / "Would block"）。本模块提供唯一的重试实现，
//! worker 内全部 ssh2 操作（exec / 通道读写 / SFTP）共用，避免重试逻辑散落多处。

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

/// 重试累计时长上限：单次操作的 EAGAIN 重试总时长约 1 秒（与旧 sftp_retry 20×50ms 语义一致）。
/// 达到上限返回当前错误，走调用方统一失败清理路径——避免远端无响应时无限空转。
const RETRY_CAP: Duration = Duration::from_millis(1000);

/// 统一的 EAGAIN 重试：指数退避 2ms → 32ms 封顶；每轮迭代检查取消标志
/// 错误判定用 is_would_block（错误码匹配 + source 链查找，兜底字符串匹配），
/// 兼容 ssh2::Error 与包装它的 io::Error（io::Read/io::Write trait 返回 io::Error）
/// 仅在 worker 线程内调用（sleep 重试不阻塞其他逻辑）
/// 默认累计上限 1s（RETRY_CAP）；需长等待的场景（如 exec 等待远程命令输出）用
/// io_retry_with_cap 指定更大上限
pub(crate) fn io_retry<T, E>(
    op: impl FnMut() -> Result<T, E>,
    cancel: &AtomicBool,
) -> Result<T, E>
where
    E: std::error::Error + 'static,
{
    io_retry_with_cap(op, cancel, RETRY_CAP)
}

/// 带自定义累计等待上限的 io_retry（唯一实现，io_retry 委托本函数，非复制逻辑）
/// 适用场景：exec 等待远程命令输出——命令计算中 EAGAIN 是正常状态（如 sha256sum
/// 计算大文件数秒无输出），1s 上限会误判失败跳过校验；取消标志仍可随时中断
pub(crate) fn io_retry_with_cap<T, E>(
    mut op: impl FnMut() -> Result<T, E>,
    cancel: &AtomicBool,
    cap: Duration,
) -> Result<T, E>
where
    E: std::error::Error + 'static,
{
    let start = Instant::now();
    let mut delay = 2u64;
    loop {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) => {
                if crate::ssh::is_would_block(&e)
                    && !cancel.load(std::sync::atomic::Ordering::Relaxed)
                    && start.elapsed() < cap
                {
                    std::thread::sleep(Duration::from_millis(delay));
                    delay = (delay * 2).min(32);
                    continue;
                }
                return Err(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssh2::{Error, ErrorCode};

    #[test]
    fn test_io_retry_retries_would_block_then_succeeds() {
        // EAGAIN 两次后成功：io_retry 应退避重试直至成功
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let r = io_retry(
            || {
                calls += 1;
                if calls < 3 {
                    Err(Error::new(
                        ErrorCode::Session(-37),
                        "Would block waiting for status message",
                    ))
                } else {
                    Ok(42u64)
                }
            },
            &cancel,
        );
        assert_eq!(r.unwrap(), 42);
        assert_eq!(calls, 3);
    }

    #[test]
    fn test_io_retry_cancel_stops_retrying() {
        // 取消标志置位：EAGAIN 不得继续重试，立即返回错误
        let cancel = AtomicBool::new(true);
        let mut calls = 0;
        let r: Result<u64, Error> = io_retry(
            || {
                calls += 1;
                Err(Error::new(ErrorCode::Session(-37), "would block"))
            },
            &cancel,
        );
        assert!(r.is_err());
        assert_eq!(calls, 1, "cancel 置位时不得重试");
    }

    #[test]
    fn test_io_retry_with_cap_long_wait() {
        // 长等待上限：EAGAIN 持续超过默认 1s 上限（约 1.2s 退避）仍能成功，
        // 模拟 exec 等待远程命令计算输出的场景（1s 默认上限会误判失败）
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let r = io_retry_with_cap(
            || {
                calls += 1;
                if calls < 40 {
                    Err(Error::new(ErrorCode::Session(-37), "Would block"))
                } else {
                    Ok(42u64)
                }
            },
            &cancel,
            Duration::from_secs(60),
        );
        assert!(r.is_ok());
        assert!(calls >= 40, "应容忍长 EAGAIN 等待，实际重试 {calls} 次");
    }

    #[test]
    fn test_io_retry_cap_returns_error() {
        // 持续 EAGAIN 且无取消：累计约 1s 后必须返回错误，不得无限重试
        let cancel = AtomicBool::new(false);
        let start = Instant::now();
        let r: Result<u64, Error> = io_retry(
            || {
                Err(Error::new(
                    ErrorCode::Session(-37),
                    "Would block waiting for status message",
                ))
            },
            &cancel,
        );
        assert!(r.is_err());
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(950),
            "应重试至累计约 1s 上限，实际 {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(2500),
            "不应超过上限过久，实际 {elapsed:?}"
        );
    }
}

//! SSH KeepAlive 后台任务模块
//! 定期发送 SSH keepalive 请求以保持连接活跃并检测死连接
//! 检测到死连接时自动触发 Session 断开

use core_common::CoreResult;
use std::sync::Arc;
use std::time::Duration;

use crate::session::Session;

/// 启动 keepalive 后台任务
/// 每 interval_secs 秒发送一次 SSH keepalive 请求
/// 当 Session 不再处于已连接状态时自动停止
/// 检测到 keepalive 失败时触发 Session 断开
pub fn spawn_keepalive(session: Arc<Session>, interval_secs: u64) -> tokio::task::JoinHandle<()> {
    let duration = Duration::from_secs(interval_secs);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(duration);
        // 首次 tick 立即触发，跳过以避免刚连接就发送 keepalive
        interval.tick().await;

        loop {
            interval.tick().await;

            if !session.is_connected() {
                tracing::debug!(session_id = %session.id, "session not connected, keepalive stopping");
                break;
            }

            let session_clone = session.clone();
            let result = tokio::task::spawn_blocking(move || {
                session_clone.send_keepalive()
            })
            .await;

            match result {
                Ok(Ok(())) => {
                    tracing::trace!(session_id = %session.id, "keepalive ok");
                }
                Ok(Err(e)) => {
                    tracing::warn!(session_id = %session.id, error = %e, "keepalive failed, disconnecting session");
                    let _ = session.disconnect();
                    break;
                }
                Err(e) => {
                    tracing::warn!(session_id = %session.id, error = %e, "spawn_blocking join error in keepalive");
                    let _ = session.disconnect();
                    break;
                }
            }
        }

        tracing::debug!(session_id = %session.id, "keepalive task finished");
    })
}

/// 同步版本：发送单次 keepalive 请求（用于测试）
#[allow(dead_code)]
pub(crate) fn send_keepalive_once(session: &Session) -> CoreResult<()> {
    session.send_keepalive()
}

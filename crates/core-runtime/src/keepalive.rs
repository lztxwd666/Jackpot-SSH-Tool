//! SSH KeepAlive 后台任务模块
//! 定期发送 SSH keepalive 请求以保持连接活跃并检测死连接
//! 检测到死连接时自动触发 Session 断开

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

            // Task 4 将 keepalive 迁移至 worker 内部（通过 do_idle_work 发送）
            // 本任务仅保留循环骨架，不做实际 SSH I/O
            tracing::trace!(session_id = %session.id, "keepalive tick (stub, Task 4 will implement)");
        }

        tracing::debug!(session_id = %session.id, "keepalive task finished");
    })
}

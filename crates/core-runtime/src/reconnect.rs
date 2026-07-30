//! SSH 重连模块
//! 提供指数退避的自动重连功能
//! 当 Session 处于 Disconnected 状态时可触发重连流程

use core_common::{ConnectionConfig, CoreResult, KnownHostsProvider, ReconnectPolicy, SessionState};
use core_event::event::{CoreEvent, SessionEvent};
use std::sync::Arc;
use std::time::Duration;

use crate::session::Session;

/// 启动重连后台任务
/// 在 Session 处于 Disconnected 状态时使用指数退避进行重连
/// get_config 闭包每次调用返回新的连接配置，由外部（如凭据控件）负责配置的获取
pub fn spawn_reconnect<F>(
    session: Arc<Session>,
    policy: ReconnectPolicy,
    get_config: F,
) -> tokio::task::JoinHandle<()>
where
    F: Fn() -> CoreResult<(ConnectionConfig, Option<Arc<dyn KnownHostsProvider>>)> + Send + 'static,
{
    tokio::spawn(async move {
        if !session.is_disconnected() {
            tracing::warn!(
                session_id = %session.id,
                state = ?session.state(),
                "cannot reconnect, session not in disconnected state"
            );
            return;
        }

        if policy.max_retries == 0 {
            tracing::debug!(session_id = %session.id, "max_retries is 0, skipping reconnect");
            return;
        }

        for attempt in 1..=policy.max_retries {
            let delay_secs = policy.delay_for(attempt);
            let delay = Duration::from_secs(delay_secs);

            tracing::info!(
                session_id = %session.id,
                attempt,
                delay_secs,
                "reconnect attempt"
            );

            session.dispatcher().dispatch(CoreEvent::Session(SessionEvent::Reconnecting {
                session_id: session.id,
                attempt,
            }));

            tokio::time::sleep(delay).await;

            match get_config() {
                Ok((config, known_hosts)) => {
                    match session.connect(config, known_hosts) {
                        Ok(()) => {
                            tracing::info!(
                                session_id = %session.id,
                                attempt,
                                "reconnect succeeded"
                            );
                            return;
                        }
                        Err(e) => {
                            tracing::warn!(
                                session_id = %session.id,
                                attempt,
                                error = %e,
                                "reconnect attempt failed"
                            );
                            // 重置状态为 Disconnected 以便下次重试
                            session.set_state(SessionState::Disconnected);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        session_id = %session.id,
                        attempt,
                        error = %e,
                        "failed to get config for reconnect"
                    );
                    session.set_state(SessionState::Disconnected);
                }
            }
        }

        // 达到最大重试次数
        let reason = format!(
            "max retries ({}) exhausted, reconnect failed",
            policy.max_retries
        );

        session.dispatcher().dispatch(CoreEvent::Session(SessionEvent::ReconnectFailed {
            session_id: session.id,
            reason: reason.clone(),
        }));

        tracing::error!(session_id = %session.id, reason = %reason, "reconnect failed permanently");
    })
}

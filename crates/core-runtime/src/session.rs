//! Session 生命周期管理模块
//! Session 是比 SshConnection 更高层的抽象，拥有连接和通道列表
//! 支持断线重连，自身不持有 ConnectionConfig（由外部调用 connect 时传入）

use core_common::{ConnectionConfig, CoreResult, KnownHostsProvider, SessionId, SessionState};
use core_event::event::{CoreEvent, SessionEvent};
use core_event::EventDispatcher;
use std::sync::{Arc, RwLock};

use crate::channel::Channel;

/// SSH 交互 Session，用于一个逻辑会话的完整生命周期管理
/// 内部使用 Mutex/RwLock 提供内部可变性，所有公开方法接收 &self
/// state 使用 std::sync::RwLock 以兼容 async 和 blocking 两种调用上下文
pub struct Session {
    pub id: SessionId,
    state: RwLock<SessionState>,
    connection: std::sync::Mutex<Option<crate::ssh::SshConnection>>,
    channels: RwLock<Vec<Arc<Channel>>>,
    dispatcher: Arc<dyn EventDispatcher>,
    known_hosts: RwLock<Option<Arc<dyn KnownHostsProvider>>>,
}

impl Session {
    /// 创建一个处于 Created 状态的新 Session
    pub fn new(dispatcher: Arc<dyn EventDispatcher>) -> Arc<Self> {
        let session = Arc::new(Self {
            id: SessionId::new(),
            state: RwLock::new(SessionState::Created),
            connection: std::sync::Mutex::new(None),
            channels: RwLock::new(Vec::new()),
            dispatcher: dispatcher.clone(),
            known_hosts: RwLock::new(None),
        });

        dispatcher.dispatch(CoreEvent::Session(SessionEvent::Created {
            session_id: session.id,
        }));

        session
    }

    /// 设置 KnownHostsProvider，用于后续连接时的主机密钥验证
    pub fn set_known_hosts(&self, known_hosts: Arc<dyn KnownHostsProvider>) {
        let mut guard = crate::rw_write(&self.known_hosts);
        *guard = Some(known_hosts);
    }

    /// 获取当前 KnownHostsProvider 的克隆引用
    pub fn get_known_hosts(&self) -> Option<Arc<dyn KnownHostsProvider>> {
        crate::rw_read(&self.known_hosts).clone()
    }

    /// 使用指定的配置和已知主机信息建立 SSH 连接
    /// 连接成功则状态转为 Connected，失败保持 Connecting 状态
    /// 重连场景下先清理旧连接和通道
    pub fn connect(
        &self,
        config: ConnectionConfig,
        known_hosts: Option<Arc<dyn KnownHostsProvider>>,
    ) -> CoreResult<()> {
        {
            let state = crate::rw_read(&self.state);
            if *state == SessionState::Closed {
                return Err(core_common::CoreError::Internal(
                    "session is closed, cannot connect".into(),
                ));
            }
        }

        // 清理旧连接和通道（重连场景）
        self.close_all_channels();
        {
            let mut guard = self.connection.lock().map_err(|e| {
                core_common::CoreError::Internal(format!("lock connection mutex failed: {e}"))
            })?;
            *guard = None;
        }

        {
            let mut state = crate::rw_write(&self.state);
            *state = SessionState::Connecting;
        }

        let host = config.host.clone();
        let port = config.port;

        self.dispatcher
            .dispatch(CoreEvent::Session(SessionEvent::Connecting {
                session_id: self.id,
                host: host.clone(),
                port,
            }));

        if let Some(ref kh) = known_hosts {
            let mut guard = crate::rw_write(&self.known_hosts);
            *guard = Some(kh.clone());
        }

        let kh_for_connect = crate::rw_read(&self.known_hosts).clone();
        let mut conn =
            crate::ssh::SshConnection::new(config, self.dispatcher.clone(), kh_for_connect);

        conn.connect()?;

        let mut guard = self.connection.lock().map_err(|e| {
            core_common::CoreError::Internal(format!("lock connection mutex failed: {e}"))
        })?;
        *guard = Some(conn);

        {
            let mut state = crate::rw_write(&self.state);
            *state = SessionState::Connected;
        }

        self.dispatcher
            .dispatch(CoreEvent::Session(SessionEvent::Connected {
                session_id: self.id,
            }));

        tracing::info!(session_id = %self.id, host = %host, "session connected");
        Ok(())
    }

    /// 断开 SSH 连接，关闭所有通道，状态转为 Disconnected（可重连）
    pub fn disconnect(&self) -> CoreResult<()> {
        self.close_all_channels();

        let mut guard = self.connection.lock().map_err(|e| {
            core_common::CoreError::Internal(format!("lock connection mutex failed: {e}"))
        })?;

        if let Some(mut conn) = guard.take() {
            let _ = conn.disconnect();
        }

        {
            let mut state = crate::rw_write(&self.state);
            *state = SessionState::Disconnected;
        }

        self.dispatcher
            .dispatch(CoreEvent::Session(SessionEvent::Disconnected {
                session_id: self.id,
            }));

        tracing::info!(session_id = %self.id, "session disconnected");
        Ok(())
    }

    /// 永久关闭 Session，不可再连接
    pub fn close(&self) -> CoreResult<()> {
        self.close_all_channels();

        let mut guard = self.connection.lock().map_err(|e| {
            core_common::CoreError::Internal(format!("lock connection mutex failed: {e}"))
        })?;

        if let Some(mut conn) = guard.take() {
            let _ = conn.disconnect();
        }

        {
            let mut state = crate::rw_write(&self.state);
            *state = SessionState::Closed;
        }

        self.dispatcher
            .dispatch(CoreEvent::Session(SessionEvent::Closed {
                session_id: self.id,
            }));

        tracing::info!(session_id = %self.id, "session closed permanently");
        Ok(())
    }

    /// 获取当前 Session 状态
    pub fn state(&self) -> SessionState {
        *crate::rw_read(&self.state)
    }

    /// 检查 Session 是否已连接
    pub fn is_connected(&self) -> bool {
        *crate::rw_read(&self.state) == SessionState::Connected
    }

    /// 检查 Session 是否处于 Disconnected 状态（可重连）
    pub fn is_disconnected(&self) -> bool {
        *crate::rw_read(&self.state) == SessionState::Disconnected
    }

    /// 获取事件分发器的引用
    pub fn dispatcher(&self) -> Arc<dyn EventDispatcher> {
        self.dispatcher.clone()
    }

    /// 打开一个 Shell 通道
    pub fn open_shell(&self) -> CoreResult<Arc<Channel>> {
        self.open_channel(Channel::open_shell)
    }

    /// 打开一个 SFTP 通道
    pub fn open_sftp(&self) -> CoreResult<Arc<Channel>> {
        self.open_channel(Channel::open_sftp)
    }

    /// 通用的通道打开方法，封装连接锁和验证逻辑
    fn open_channel<F>(&self, factory: F) -> CoreResult<Arc<Channel>>
    where
        F: FnOnce(SessionId, &ssh2::Session, Arc<dyn EventDispatcher>) -> CoreResult<Arc<Channel>>,
    {
        let guard = self.connection.lock().map_err(|e| {
            core_common::CoreError::Internal(format!("lock connection mutex failed: {e}"))
        })?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| core_common::CoreError::Internal("no active connection".into()))?;
        let ssh_session = conn
            .session()
            .ok_or_else(|| core_common::CoreError::Internal("ssh session not available".into()))?;

        let channel = factory(self.id, ssh_session, self.dispatcher.clone())?;

        let mut channels = crate::rw_write(&self.channels);
        channels.push(channel.clone());

        Ok(channel)
    }

    /// 获取当前所有通道的快照
    pub fn channels(&self) -> Vec<Arc<Channel>> {
        crate::rw_read(&self.channels).clone()
    }

    /// 计算远程文件的 SHA-256 校验和（通过 exec sha256sum）
    pub fn remote_sha256(&self, path: &str) -> CoreResult<String> {
        let guard = self.connection.lock().map_err(|e| {
            core_common::CoreError::Internal(format!("lock connection mutex failed: {e}"))
        })?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| core_common::CoreError::Internal("no active connection".into()))?;
        // POSIX 单引号转义，防止路径中的特殊字符
        let escaped = path.replace('\'', "'\\''");
        let out = conn.exec_command(&format!("sha256sum '{}'", escaped))?;
        // 输出格式: "<hash>  <path>"
        let hash = out
            .split_whitespace()
            .next()
            .ok_or_else(|| core_common::CoreError::Internal("empty sha256sum output".into()))?;
        Ok(hash.to_string())
    }

    /// 发送 SSH keepalive 请求，检测连接是否存活
    pub(crate) fn send_keepalive(&self) -> CoreResult<()> {
        let guard = self.connection.lock().map_err(|e| {
            core_common::CoreError::Internal(format!("lock connection mutex failed: {e}"))
        })?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| core_common::CoreError::Internal("no active connection".into()))?;
        let session = conn
            .session()
            .ok_or_else(|| core_common::CoreError::Internal("ssh session not available".into()))?;

        session
            .keepalive_send()
            .map_err(|e| core_common::CoreError::Internal(format!("keepalive failed: {e}")))?;

        Ok(())
    }

    /// 设置 Session 状态（供 keepalive/reconnect 模块内部使用）
    /// 守卫：Closed 是终态，不允许被其他状态覆盖（防止重连任务"复活"已关闭的会话）
    pub(crate) fn set_state(&self, new_state: SessionState) {
        let mut state = crate::rw_write(&self.state);
        if *state == SessionState::Closed && new_state != SessionState::Closed {
            tracing::warn!(
                session_id = %self.id,
                ?new_state,
                "ignored state transition: session is closed (terminal state)"
            );
            return;
        }
        *state = new_state;
    }

    /// 关闭所有通道并从通道列表中清除
    /// 先取出列表再释放锁，避免在持有 channels 写锁期间执行阻塞的 close()
    fn close_all_channels(&self) {
        let to_close = {
            let mut channels = crate::rw_write(&self.channels);
            channels.drain(..).collect::<Vec<_>>()
        };
        for channel in to_close {
            if let Err(e) = channel.close() {
                tracing::warn!(channel_id = %channel.id, error = %e, "failed to close channel");
            }
        }
    }
}

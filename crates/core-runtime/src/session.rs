//! Session 生命周期管理模块
//! Session 是比 SshConnection 更高层的抽象，拥有连接和通道列表
//! 支持断线重连，自身不持有 ConnectionConfig（由外部调用 connect 时传入）

use core_common::{ConnectionConfig, CoreResult, KnownHostsProvider, SessionId, SessionState};
use core_event::event::{CoreEvent, SessionEvent};
use core_event::EventDispatcher;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::channel::Channel;

/// SSH 交互 Session，用于一个逻辑会话的完整生命周期管理
/// 内部使用 Mutex/RwLock 提供内部可变性，所有公开方法接收 &self
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
        let mut guard = self.known_hosts.blocking_write();
        *guard = Some(known_hosts);
    }

    /// 获取当前 KnownHostsProvider 的克隆引用
    pub fn get_known_hosts(&self) -> Option<Arc<dyn KnownHostsProvider>> {
        self.known_hosts.blocking_read().clone()
    }

    /// 使用指定的配置和已知主机信息建立 SSH 连接
    /// 连接成功则状态转为 Connected，失败保持 Connecting 状态
    pub fn connect(
        &self,
        config: ConnectionConfig,
        known_hosts: Option<Arc<dyn KnownHostsProvider>>,
    ) -> CoreResult<()> {
        {
            let state = self.state.blocking_read();
            if *state == SessionState::Closed {
                return Err(core_common::CoreError::Internal(
                    "session is closed, cannot connect".into(),
                ));
            }
        }

        {
            let mut state = self.state.blocking_write();
            *state = SessionState::Connecting;
        }

        let host = config.host.clone();
        let port = config.port;

        self.dispatcher.dispatch(CoreEvent::Session(SessionEvent::Connecting {
            session_id: self.id,
            host: host.clone(),
            port,
        }));

        if let Some(ref kh) = known_hosts {
            let mut guard = self.known_hosts.blocking_write();
            *guard = Some(kh.clone());
        }

        let kh_for_connect = self.known_hosts.blocking_read().clone();
        let mut conn =
            crate::ssh::SshConnection::new(config, self.dispatcher.clone(), kh_for_connect);

        conn.connect()?;

        let mut guard = self.connection.lock().map_err(|e| {
            core_common::CoreError::Internal(format!("lock connection mutex failed: {e}"))
        })?;
        *guard = Some(conn);

        {
            let mut state = self.state.blocking_write();
            *state = SessionState::Connected;
        }

        self.dispatcher.dispatch(CoreEvent::Session(SessionEvent::Connected {
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
            let mut state = self.state.blocking_write();
            *state = SessionState::Disconnected;
        }

        self.dispatcher.dispatch(CoreEvent::Session(SessionEvent::Disconnected {
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
            let mut state = self.state.blocking_write();
            *state = SessionState::Closed;
        }

        self.dispatcher.dispatch(CoreEvent::Session(SessionEvent::Closed {
            session_id: self.id,
        }));

        tracing::info!(session_id = %self.id, "session closed permanently");
        Ok(())
    }

    /// 获取当前 Session 状态
    pub fn state(&self) -> SessionState {
        self.state.blocking_read().clone()
    }

    /// 检查 Session 是否已连接
    pub fn is_connected(&self) -> bool {
        *self.state.blocking_read() == SessionState::Connected
    }

    /// 检查 Session 是否处于 Disconnected 状态（可重连）
    pub fn is_disconnected(&self) -> bool {
        *self.state.blocking_read() == SessionState::Disconnected
    }

    /// 获取事件分发器的引用
    pub fn dispatcher(&self) -> Arc<dyn EventDispatcher> {
        self.dispatcher.clone()
    }

    /// 打开一个 Shell 通道
    pub fn open_shell(&self) -> CoreResult<Arc<Channel>> {
        let guard = self.connection.lock().map_err(|e| {
            core_common::CoreError::Internal(format!("lock connection mutex failed: {e}"))
        })?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| core_common::CoreError::Internal("no active connection".into()))?;
        let ssh_session = conn.session().ok_or_else(|| {
            core_common::CoreError::Internal("ssh session not available".into())
        })?;

        let channel =
            Channel::open_shell(self.id, ssh_session, self.dispatcher.clone())?;

        let mut channels = self.channels.blocking_write();
        channels.push(channel.clone());

        Ok(channel)
    }

    /// 打开一个 SFTP 通道
    pub fn open_sftp(&self) -> CoreResult<Arc<Channel>> {
        let guard = self.connection.lock().map_err(|e| {
            core_common::CoreError::Internal(format!("lock connection mutex failed: {e}"))
        })?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| core_common::CoreError::Internal("no active connection".into()))?;
        let ssh_session = conn.session().ok_or_else(|| {
            core_common::CoreError::Internal("ssh session not available".into())
        })?;

        let channel =
            Channel::open_sftp(self.id, ssh_session, self.dispatcher.clone())?;

        let mut channels = self.channels.blocking_write();
        channels.push(channel.clone());

        Ok(channel)
    }

    /// 获取当前所有通道的快照
    pub fn channels(&self) -> Vec<Arc<Channel>> {
        self.channels.blocking_read().clone()
    }

    /// 发送 SSH keepalive 请求，检测连接是否存活
    pub(crate) fn send_keepalive(&self) -> CoreResult<()> {
        let guard = self.connection.lock().map_err(|e| {
            core_common::CoreError::Internal(format!("lock connection mutex failed: {e}"))
        })?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| core_common::CoreError::Internal("no active connection".into()))?;
        let session = conn.session().ok_or_else(|| {
            core_common::CoreError::Internal("ssh session not available".into())
        })?;

        session
            .keepalive_send()
            .map_err(|e| core_common::CoreError::Internal(format!("keepalive failed: {e}")))?;

        Ok(())
    }

    /// 设置 Session 状态（供 keepalive/reconnect 模块内部使用）
    pub(crate) fn set_state(&self, new_state: SessionState) {
        let mut state = self.state.blocking_write();
        *state = new_state;
    }

    /// 关闭所有通道并从通道列表中清除
    fn close_all_channels(&self) {
        let mut channels = self.channels.blocking_write();
        for channel in channels.drain(..) {
            let _ = channel.close();
        }
    }
}

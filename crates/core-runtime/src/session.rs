//! Session 生命周期管理模块
//! Session 是比 SshConnection 更高层的抽象，拥有连接和通道列表
//! 支持断线重连，自身不持有 ConnectionConfig（由外部调用 connect 时传入）
//! Stage 6: 每 Session 一个 worker 线程（Active Object 模式），SSH 操作经 mpsc 串行执行

use core_common::{ConnectionConfig, CoreError, CoreResult, KnownHostsProvider, ReconnectPolicy, SessionId, SessionState};
use core_event::EventDispatcher;
use core_event::event::{CoreEvent, SessionEvent};
use std::sync::Arc;
use tokio::sync::oneshot;

use crate::channel::Channel;
use crate::worker::{self, WorkerHandle, WorkerCommand};

/// 不含任何凭据的连接配置骨架（重连用，Task 6 启用）
pub struct HostConnectionProfile {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_kind: ProfileAuthKind,
    pub timeout_secs: u64,
}

pub enum ProfileAuthKind {
    Password,
    PrivateKey { path: std::path::PathBuf, needs_passphrase: bool },
}

/// SSH 交互 Session，用于一个逻辑会话的完整生命周期管理
/// 内部使用 Mutex/RwLock 提供内部可变性，所有公开方法接收 &self
/// state 使用 std::sync::RwLock 以兼容 async 和 blocking 两种调用上下文
/// Stage 6: 引入 worker 线程，所有 ssh2 操作经 WorkerHandle 投递到单线程串行执行
pub struct Session {
    pub id: SessionId,
    /// 与 worker 共享的状态（worker 单写者，外部只读）
    state: Arc<std::sync::RwLock<SessionState>>,
    /// worker 句柄：投递命令的唯一入口
    worker: Arc<WorkerHandle>,
    dispatcher: Arc<dyn EventDispatcher>,
    known_hosts: std::sync::RwLock<Option<Arc<dyn KnownHostsProvider>>>,
    #[allow(dead_code)]
    host_config: std::sync::RwLock<Option<HostConnectionProfile>>,
    reconnect_policy: std::sync::RwLock<Option<ReconnectPolicy>>,
    // —— 以下旧字段由后续任务移除 ——
    // connection: Task 3 迁移 open_channel 后与 channels 一起删除
    // 当前仅 open_channel/open_shell/open_sftp 通过此字段访问（connect 已走 worker）
    connection: std::sync::Mutex<Option<crate::ssh::SshConnection>>,
    channels: std::sync::RwLock<Vec<Arc<Channel>>>,
}

impl Session {
    /// 创建一个处于 Created 状态的新 Session，同时启动 worker 线程
    pub fn new(dispatcher: Arc<dyn EventDispatcher>) -> Arc<Self> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = Arc::new(std::sync::RwLock::new(SessionState::Created));
        let session_id = SessionId::new();
        let session = Arc::new(Self {
            id: session_id,
            state: state.clone(),
            worker: Arc::new(WorkerHandle::new(tx)),
            dispatcher: dispatcher.clone(),
            known_hosts: std::sync::RwLock::new(None),
            host_config: std::sync::RwLock::new(None),
            reconnect_policy: std::sync::RwLock::new(None),
            connection: std::sync::Mutex::new(None),
            channels: std::sync::RwLock::new(Vec::new()),
        });

        // 启动 worker 线程（持 dispatcher 与共享状态；不持 Session 引用，避免循环引用）
        let dispatch_clone = dispatcher.clone();
        let join = std::thread::spawn(move || {
            worker::run_loop(rx, state, dispatch_clone, session_id);
        });
        session.worker.set_join(join);

        session.dispatcher
            .dispatch(CoreEvent::Session(SessionEvent::Created {
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

    /// 设置重连策略
    pub fn set_reconnect_policy(&self, policy: ReconnectPolicy) {
        *crate::rw_write(&self.reconnect_policy) = Some(policy);
    }

    /// 获取重连策略的克隆
    pub fn reconnect_policy(&self) -> Option<ReconnectPolicy> {
        crate::rw_read(&self.reconnect_policy).clone()
    }

    /// 保存连接配置骨架（剥离凭据），供重连使用
    #[allow(dead_code)]
    pub(crate) fn save_host_config(&self, config: &ConnectionConfig) {
        let profile = HostConnectionProfile {
            host: config.host.clone(),
            port: config.port,
            username: config.username.clone(),
            auth_kind: match &config.auth_method {
                core_common::AuthMethod::Password(_) => ProfileAuthKind::Password,
                core_common::AuthMethod::PrivateKey { path, passphrase } => ProfileAuthKind::PrivateKey {
                    path: path.clone(),
                    needs_passphrase: passphrase.is_some(),
                },
                core_common::AuthMethod::Agent => ProfileAuthKind::Password,
            },
            timeout_secs: config.timeout_secs,
        };
        *crate::rw_write(&self.host_config) = Some(profile);
    }

    /// 建立 SSH 连接（阻塞至完成；内部投递 Connect 命令并等待回执）
    /// 注：profile 保存逻辑在 Task 6 迁移至 worker 内部（profile_from_config）；
    /// 本任务的 save_host_config 为过渡实现，Task 6 时随 Session::host_config 字段一并移除
    pub fn connect(
        &self,
        config: ConnectionConfig,
        known_hosts: Option<Arc<dyn KnownHostsProvider>>,
    ) -> CoreResult<()> {
        if self.state() == SessionState::Closed {
            return Err(CoreError::Internal("session is closed, cannot connect".into()));
        }
        // 保存 known_hosts 引用供后续使用
        if let Some(ref kh) = known_hosts {
            *crate::rw_write(&self.known_hosts) = Some(kh.clone());
        }
        let kh_for_connect = crate::rw_read(&self.known_hosts).clone();
        let (reply_tx, reply_rx) = oneshot::channel();
        self.save_host_config(&config);
        self.worker.call(
            WorkerCommand::Connect { config, known_hosts: kh_for_connect, reply: reply_tx },
            reply_rx,
        )
    }

    /// 断开连接（可重连），投递 Disconnect 命令到 worker
    pub fn disconnect(&self) -> CoreResult<()> {
        self.worker.send(WorkerCommand::Disconnect)
    }

    /// 永久关闭 Session，不可再连接；投递 Close 命令到 worker（worker 线程随后退出）
    pub fn close(&self) -> CoreResult<()> {
        self.worker.send(WorkerCommand::Close)
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

    /// 计算远程文件的 SHA-256 校验和（通过 worker Exec sha256sum）
    pub fn remote_sha256(&self, path: &str) -> CoreResult<String> {
        let escaped = path.replace('\'', "'\\''");
        let (reply_tx, reply_rx) = oneshot::channel();
        let reply = self.worker.call(
            WorkerCommand::Exec { command: format!("sha256sum '{}'", escaped), reply: reply_tx },
            reply_rx,
        )?;
        let hash = reply.split_whitespace().next()
            .ok_or_else(|| CoreError::Internal("empty sha256sum output".into()))?;
        Ok(hash.to_string())
    }

    /// 关闭所有通道并从通道列表中清除
    /// 先取出列表再释放锁，避免在持有 channels 写锁期间执行阻塞的 close()
    #[allow(dead_code)]
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

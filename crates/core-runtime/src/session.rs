//! Session 生命周期管理模块
//! Session 是比 SshConnection 更高层的抽象，拥有连接和通道列表
//! 自身不持有 ConnectionConfig（由外部调用 connect 时传入）
//! Stage 6: 每 Session 一个 worker 线程（Active Object 模式），SSH 操作经 mpsc 串行执行

use core_common::{ConnectionConfig, CoreError, CoreResult, KnownHostsProvider, SessionId, SessionState};
use core_event::EventDispatcher;
use core_event::event::{CoreEvent, SessionEvent};
use std::sync::Arc;
use tokio::sync::oneshot;

use crate::channel::Channel;
use crate::worker::{self, WorkerHandle, WorkerCommand};

/// SSH 交互 Session，用于一个逻辑会话的完整生命周期管理
/// 内部使用 RwLock 提供内部可变性（状态/known_hosts 的原子读），所有公开方法接收 &self
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

    /// 建立 SSH 连接（阻塞至完成；内部投递 Connect 命令并等待回执）
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

    /// 打开一个 Shell 通道（投递 OpenChannel 命令到 worker，构造 Channel 句柄）
    pub fn open_shell(&self) -> CoreResult<Arc<Channel>> {
        self.open_channel(core_common::ChannelType::Shell)
    }

    /// 打开一个 SFTP 通道（投递 OpenChannel 命令到 worker，构造 Channel 句柄）
    pub fn open_sftp(&self) -> CoreResult<Arc<Channel>> {
        self.open_channel(core_common::ChannelType::Sftp)
    }

    /// 打开通道的通用方法：投递 OpenChannel 命令到 worker 并构造纯句柄 Channel
    fn open_channel(&self, ctype: core_common::ChannelType) -> CoreResult<Arc<Channel>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let cid = self.worker.call(
            WorkerCommand::OpenChannel { ctype, reply: reply_tx },
            reply_rx,
        )?;
        Ok(Channel::new(cid, ctype, self.id, self.worker.clone()))
    }

    /// 计算远程文件的 SHA-256 校验和；远端无可用哈希命令或 exec 失败时返回 None（跳过校验）
    /// 命令探测与解析在 worker 内完成（会话级缓存），输出格式按命令区分解析
    pub fn remote_sha256(&self, path: &str) -> CoreResult<Option<String>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.worker.call(
            WorkerCommand::RemoteSha256 { path: path.to_string(), reply: reply_tx },
            reply_rx,
        )
    }

    /// 关闭所有通道（投递 CloseAllChannels 到 worker）
    pub fn close_all_channels(&self) -> CoreResult<()> {
        self.worker.send(WorkerCommand::CloseAllChannels)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // 回收 worker 线程：Session 被废弃（如连接失败后未 close）时投递 Close，
        // worker 完成清理后经 drain_nested_commands 的 Disconnected 分支自然退出。
        // Session 不持 worker 的接收端，无循环引用；与 terminal_close 的重复 Close 幂等安全
        let _ = self.worker.send(WorkerCommand::Close);
    }
}

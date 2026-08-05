//! SSH 执行线程模块
//! 每 Session 一个 worker 线程，串行执行全部 ssh2 操作（Active Object 模式）
//! 事件循环: try_recv 处理命令 → 空闲工作（keepalive / shell 读轮询）→ sleep 25ms

#![allow(dead_code)]

use crate::channel::{self, ChannelInner};
use crate::ssh::SshConnection;
use core_common::{
    ChannelId, ChannelType, ConnectionConfig, CoreError, CoreResult, FileEntry, KnownHostsProvider,
    SessionId, SessionState,
};
use core_event::EventDispatcher;
use core_event::event::{ChannelEvent, CoreEvent, SessionEvent};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use tokio::sync::oneshot;

/// worker 空闲工作间隔（毫秒）：shell 读轮询与命令响应延迟的上限
const IDLE_INTERVAL_MS: u64 = 25;

/// keepalive 发送间隔：连接空闲时每 30 秒发送一次 SSH keepalive 请求以保持连接活跃
const KEEPALIVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// keepalive 间隔判定：now 距 last 达到 interval 返回 true
fn keepalive_due(
    last: Option<std::time::Instant>,
    now: std::time::Instant,
    interval: std::time::Duration,
) -> bool {
    match last {
        None => false, // 从未发送：连接刚建立，首轮不立即发送
        Some(l) => now.duration_since(l) >= interval,
    }
}

/// 会话级命令（Active Object 的 Method Request）
/// 所有变体经 mpsc 队列串行到达 worker，回执用 oneshot
pub(crate) enum WorkerCommand {
    Connect {
        config: ConnectionConfig,
        known_hosts: Option<Arc<dyn KnownHostsProvider>>,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    Disconnect,
    Close,
    OpenChannel {
        ctype: ChannelType,
        reply: oneshot::Sender<CoreResult<ChannelId>>,
    },
    ChannelWrite {
        channel: ChannelId,
        data: Vec<u8>,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    ChannelResize {
        channel: ChannelId,
        cols: u32,
        rows: u32,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    ChannelClose {
        channel: ChannelId,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    Exec {
        command: String,
        reply: oneshot::Sender<CoreResult<String>>,
    },
    SftpReadDir {
        path: String,
        reply: oneshot::Sender<CoreResult<Vec<FileEntry>>>,
    },
    SftpCreateDir {
        path: String,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    SftpRemove {
        path: String,
        is_dir: bool,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    SftpRename {
        old: String,
        new: String,
        reply: oneshot::Sender<CoreResult<()>>,
    },
    SftpTransfer {
        kind: TransferKind,
        remote: String,
        local: String,
        progress: tokio::sync::mpsc::UnboundedSender<(u64, u64)>,
        reply: oneshot::Sender<CoreResult<u64>>,
    },
    ProvideCredential {
        secret: String,
    },
    CloseAllChannels,
}

/// 传输方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferKind {
    Download,
    Upload,
}

/// worker 句柄：外部通过它投递命令
/// tx 为 UnboundedSender（Send+Sync），Session/Channel 可无锁共享
pub(crate) struct WorkerHandle {
    tx: tokio::sync::mpsc::UnboundedSender<WorkerCommand>,
    join: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl WorkerHandle {
    pub(crate) fn new(tx: tokio::sync::mpsc::UnboundedSender<WorkerCommand>) -> Self {
        Self {
            tx,
            join: std::sync::Mutex::new(None),
        }
    }

    pub(crate) fn set_join(&self, join: std::thread::JoinHandle<()>) {
        *self.join.lock().unwrap() = Some(join);
    }

    /// 投递命令（不等待回执的变体）
    pub(crate) fn send(&self, cmd: WorkerCommand) -> CoreResult<()> {
        self.tx
            .send(cmd)
            .map_err(|_| core_common::CoreError::Internal("worker thread is gone".into()))
    }

    /// 投递命令并等待回执（调用方需处于阻塞上下文，如 spawn_blocking）
    pub(crate) fn call<T>(
        &self,
        cmd: WorkerCommand,
        rx: oneshot::Receiver<CoreResult<T>>,
    ) -> CoreResult<T> {
        self.send(cmd)?;
        rx.blocking_recv()
            .map_err(|_| core_common::CoreError::Internal("worker reply channel closed".into()))?
    }
}

/// worker 主循环入口（std::thread）
/// state 与 Session 共享（外部查询状态）；重连所需配置由命令携带（后续任务扩展）
pub(crate) fn run_loop(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<WorkerCommand>,
    state: Arc<std::sync::RwLock<SessionState>>,
    dispatcher: Arc<dyn EventDispatcher>,
    session_id: SessionId,
) {
    let mut w = Worker::new(state, dispatcher, session_id);
    loop {
        // 处理所有已到达的命令（传输执行中会嵌套处理，见 Task 5）
        while let Ok(cmd) = rx.try_recv() {
            if !w.handle(cmd) {
                return; // Close 完成，结束线程
            }
        }
        w.do_idle_work();
        std::thread::sleep(std::time::Duration::from_millis(IDLE_INTERVAL_MS));
    }
}

/// worker 内部状态：全部 ssh2 相关状态移入此处（无锁，单线程独占）
struct Worker {
    state: Arc<std::sync::RwLock<SessionState>>,
    dispatcher: Arc<dyn EventDispatcher>,
    closed: bool,
    session_id: Option<SessionId>,     // run_loop 启动时注入
    connection: Option<SshConnection>, // ssh2 连接所有权（Task 2 引入）
    last_keepalive: Option<Instant>,   // Task 4 使用
    cancel: AtomicBool,                // Task 5 传输取消使用
    raw_channels: HashMap<ChannelId, ChannelInner>, // worker 内通道注册表
    channels: Vec<ChannelId>,          // 已打开通道 ID 列表
}

impl Worker {
    fn new(
        state: Arc<std::sync::RwLock<SessionState>>,
        dispatcher: Arc<dyn EventDispatcher>,
        session_id: SessionId,
    ) -> Self {
        Self {
            state,
            dispatcher,
            session_id: Some(session_id),
            connection: None,
            last_keepalive: None,
            closed: false,
            cancel: AtomicBool::new(false),
            raw_channels: HashMap::new(),
            channels: Vec::new(),
        }
    }

    /// 写入状态，守卫 Closed 终态（Closed 状态下不允许被非 Closed 覆盖）
    fn write_state(&self, s: SessionState) -> CoreResult<()> {
        let mut guard = self
            .state
            .write()
            .map_err(|e| CoreError::Internal(format!("state lock poisoned: {e}")))?;
        if *guard == SessionState::Closed && s != SessionState::Closed {
            return Err(CoreError::Internal(
                "session is closed (terminal state)".into(),
            ));
        }
        *guard = s;
        Ok(())
    }

    /// 分发事件（worker 内单线程，无需同步）
    fn dispatch(&self, event: CoreEvent) {
        self.dispatcher.dispatch(event);
    }

    /// 建立 SSH 连接（worker 内执行）
    fn connect_inner(
        &mut self,
        config: ConnectionConfig,
        known_hosts: Option<Arc<dyn KnownHostsProvider>>,
    ) -> CoreResult<()> {
        self.write_state(SessionState::Connecting)?;
        let host = config.host.clone();
        let port = config.port;
        self.dispatch(CoreEvent::Session(SessionEvent::Connecting {
            session_id: self.session_id(),
            host: host.clone(),
            port,
        }));
        let mut conn = SshConnection::new(config, self.dispatcher.clone(), known_hosts);
        match conn.connect() {
            Ok(()) => {
                self.connection = Some(conn);
                self.write_state(SessionState::Connected)?;
                self.dispatch(CoreEvent::Session(SessionEvent::Connected {
                    session_id: self.session_id(),
                }));
                Ok(())
            }
            Err(e) => {
                let _ = self.write_state(SessionState::Disconnected);
                Err(e)
            }
        }
    }

    /// 断开 SSH 连接（worker 内执行），状态转为 Disconnected（可重连）
    fn disconnect_inner(&mut self) {
        self.close_all_channels_inner();
        if let Some(mut conn) = self.connection.take() {
            let _ = conn.disconnect();
        }
        if self.write_state(SessionState::Disconnected).is_ok() {
            self.dispatch(CoreEvent::Session(SessionEvent::Disconnected {
                session_id: self.session_id(),
            }));
        }
    }

    /// 执行远程命令并返回 stdout（worker 内执行）
    fn exec_inner(&self, command: &str) -> CoreResult<String> {
        let conn = self
            .connection
            .as_ref()
            .ok_or_else(|| CoreError::Internal("no active connection".into()))?;
        conn.exec_command(command)
    }

    /// 获取当前 worker 绑定的 session_id（构造时注入，始终有值）
    fn session_id(&self) -> SessionId {
        self.session_id
            .expect("session_id always set at construction")
    }

    /// 打开通道（shell 或 sftp），返回 ChannelId
    fn open_channel_inner(&mut self, ctype: ChannelType) -> CoreResult<ChannelId> {
        let conn = self
            .connection
            .as_ref()
            .ok_or_else(|| CoreError::Internal("no active connection".into()))?;
        let ssh_session = conn
            .session()
            .ok_or_else(|| CoreError::Internal("ssh session not available".into()))?;
        let channel_id = ChannelId::new();
        let sid = self.session_id();
        let dispatcher = self.dispatcher.clone();
        let inner = match ctype {
            ChannelType::Shell => {
                let ch = channel::open_shell_raw(ssh_session, sid, channel_id, dispatcher.clone())?;
                ChannelInner::Session(ch)
            }
            ChannelType::Sftp => {
                let sftp =
                    channel::open_sftp_raw(ssh_session, sid, channel_id, dispatcher.clone())?;
                ChannelInner::Sftp(sftp)
            }
        };
        self.raw_channels.insert(channel_id, inner);
        self.channels.push(channel_id);
        self.dispatch(CoreEvent::Channel(ChannelEvent::Opened {
            session_id: sid,
            channel_id,
        }));
        Ok(channel_id)
    }

    /// 向通道写入数据（处理部分写入 + EAGAIN 重试）
    fn channel_write_inner(&mut self, channel: ChannelId, data: &[u8]) -> CoreResult<()> {
        let inner = self
            .raw_channels
            .get_mut(&channel)
            .ok_or_else(|| CoreError::Internal("channel not found".into()))?;
        match inner {
            ChannelInner::Session(ch) => {
                let mut written = 0;
                while written < data.len() {
                    match ch.write(&data[written..]) {
                        Ok(n) => written += n,
                        Err(e) if crate::ssh::is_would_block(&e) => {
                            std::thread::sleep(std::time::Duration::from_millis(2));
                        }
                        Err(e) => {
                            return Err(CoreError::Internal(format!("channel write failed: {e}")));
                        }
                    }
                }
                Ok(())
            }
            ChannelInner::Sftp(_) => Err(CoreError::Internal(
                "write not supported on sftp channel".into(),
            )),
        }
    }

    /// 调整 PTY 尺寸（EAGAIN 重试最多 20 次）
    fn channel_resize_inner(&mut self, channel: ChannelId, cols: u32, rows: u32) -> CoreResult<()> {
        let inner = self
            .raw_channels
            .get_mut(&channel)
            .ok_or_else(|| CoreError::Internal("channel not found".into()))?;
        match inner {
            ChannelInner::Session(ch) => {
                let mut retries = 0;
                loop {
                    match ch.request_pty_size(cols, rows, Some(cols * 8), Some(rows * 16)) {
                        Ok(()) => return Ok(()),
                        Err(e) if crate::ssh::is_would_block(&e) && retries < 20 => {
                            retries += 1;
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                        Err(e) => {
                            return Err(CoreError::Internal(format!("resize pty failed: {e}")));
                        }
                    }
                }
            }
            ChannelInner::Sftp(_) => Err(CoreError::Internal(
                "resize not supported on sftp channel".into(),
            )),
        }
    }

    /// 关闭通道（清理 ssh2 资源并广播 Closed 事件）
    fn channel_close_inner(&mut self, channel: ChannelId) -> CoreResult<()> {
        if let Some(inner) = self.raw_channels.remove(&channel) {
            match inner {
                ChannelInner::Session(mut ch) => {
                    let _ = ch.close();
                    let _ = ch.wait_close();
                }
                ChannelInner::Sftp(_) => {}
            }
        }
        self.channels.retain(|c| *c != channel);
        self.dispatch(CoreEvent::Channel(ChannelEvent::Closed {
            session_id: self.session_id(),
            channel_id: channel,
        }));
        Ok(())
    }

    /// 关闭全部通道
    fn close_all_channels_inner(&mut self) {
        let ids: Vec<ChannelId> = self.channels.to_vec();
        for cid in ids {
            let _ = self.channel_close_inner(cid);
        }
    }

    /// 处理单条命令；返回 false 表示应结束线程
    /// 已实现：Connect / Disconnect / Close / Exec
    /// 其余命令后续任务逐个替换为真实现
    fn handle(&mut self, cmd: WorkerCommand) -> bool {
        match cmd {
            WorkerCommand::Connect {
                config,
                known_hosts,
                reply,
            } => {
                let r = self.connect_inner(config, known_hosts);
                let _ = reply.send(r);
            }
            WorkerCommand::Disconnect => {
                self.disconnect_inner();
            }
            WorkerCommand::Close => {
                self.disconnect_inner();
                self.closed = true;
                let _ = self.write_state(SessionState::Closed);
                self.dispatch(CoreEvent::Session(SessionEvent::Closed {
                    session_id: self.session_id(),
                }));
                return false; // 结束线程
            }
            WorkerCommand::Exec { command, reply } => {
                let r = self.exec_inner(&command);
                let _ = reply.send(r);
            }
            WorkerCommand::OpenChannel { ctype, reply } => {
                let r = self.open_channel_inner(ctype);
                let _ = reply.send(r);
            }
            WorkerCommand::ChannelWrite {
                channel,
                data,
                reply,
            } => {
                let r = self.channel_write_inner(channel, &data);
                let _ = reply.send(r);
            }
            WorkerCommand::ChannelResize {
                channel,
                cols,
                rows,
                reply,
            } => {
                let r = self.channel_resize_inner(channel, cols, rows);
                let _ = reply.send(r);
            }
            WorkerCommand::ChannelClose { channel, reply } => {
                let r = self.channel_close_inner(channel);
                let _ = reply.send(r);
            }
            WorkerCommand::CloseAllChannels => {
                self.close_all_channels_inner();
            }
            // 其余命令 Task 5-6 实现，先回执错误（保证调用方不挂起）
            WorkerCommand::SftpReadDir { reply, .. } => {
                let _ = reply.send(Err(CoreError::Internal(
                    "command not implemented yet".into(),
                )));
            }
            WorkerCommand::SftpCreateDir { reply, .. } => {
                let _ = reply.send(Err(CoreError::Internal(
                    "command not implemented yet".into(),
                )));
            }
            WorkerCommand::SftpRemove { reply, .. } => {
                let _ = reply.send(Err(CoreError::Internal(
                    "command not implemented yet".into(),
                )));
            }
            WorkerCommand::SftpRename { reply, .. } => {
                let _ = reply.send(Err(CoreError::Internal(
                    "command not implemented yet".into(),
                )));
            }
            WorkerCommand::SftpTransfer { reply, .. } => {
                let _ = reply.send(Err(CoreError::Internal(
                    "command not implemented yet".into(),
                )));
            }
            WorkerCommand::ProvideCredential { .. } => {}
        }
        !self.closed
    }

    /// 空闲工作：keepalive 发送与 shell 通道非阻塞轮询读
    fn do_idle_work(&mut self) {
        if self.closed || self.connection.is_none() {
            return; // 未连接不工作
        }
        // keepalive：传输进行中 worker 忙，此函数不会执行，语义天然正确
        if keepalive_due(
            self.last_keepalive,
            std::time::Instant::now(),
            KEEPALIVE_INTERVAL,
        ) {
            let result = self
                .connection
                .as_ref()
                .and_then(|conn| conn.session())
                .map(|s| s.keepalive_send());
            match result {
                Some(Ok(_)) => {
                    self.last_keepalive = Some(std::time::Instant::now());
                }
                Some(Err(e)) => {
                    tracing::warn!(session_id = ?self.session_id(), error = %e, "keepalive failed, disconnecting");
                    self.disconnect_inner();
                    return;
                }
                None => {}
            }
        }
        // shell 通道轮询读
        let ids: Vec<ChannelId> = self.channels.clone();
        for id in ids {
            let _ = self.poll_shell_read(id);
        }
    }

    /// 非阻塞读一次 shell 通道；有数据则 dispatch DataReceived
    fn poll_shell_read(&mut self, channel: ChannelId) -> CoreResult<()> {
        let inner = match self.raw_channels.get_mut(&channel) {
            Some(i) => i,
            None => return Ok(()), // 已关闭
        };
        let ChannelInner::Session(ch) = inner else {
            return Ok(());
        };
        let mut buf = [0u8; 4096];
        match ch.read(&mut buf) {
            Ok(0) | Err(_) => Ok(()), // EAGAIN 或无数据：无事
            Ok(n) => {
                let data = Vec::from(&buf[..n]);
                self.dispatch(CoreEvent::Channel(ChannelEvent::DataReceived {
                    session_id: self.session_id(),
                    channel_id: channel,
                    data,
                }));
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_common::{ConnectionConfig, SessionId, SessionState};
    use core_event::LoggingDispatcher;

    fn spawn_test_worker() -> (Arc<WorkerHandle>, Arc<std::sync::RwLock<SessionState>>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = Arc::new(std::sync::RwLock::new(SessionState::Created));
        let state_clone = state.clone();
        let dispatcher: Arc<dyn EventDispatcher> = Arc::new(LoggingDispatcher);
        let session_id = SessionId::new();
        let join = std::thread::spawn(move || run_loop(rx, state_clone, dispatcher, session_id));
        let handle = Arc::new(WorkerHandle::new(tx));
        handle.set_join(join);
        (handle, state)
    }

    #[test]
    fn test_worker_alive_after_spawn() {
        let (handle, _) = spawn_test_worker();
        // 未连接时开启通道必须回执"no active connection"错误（不可挂起）
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        handle
            .send(WorkerCommand::OpenChannel {
                ctype: core_common::ChannelType::Shell,
                reply: reply_tx,
            })
            .unwrap();
        let result = reply_rx
            .blocking_recv()
            .expect("worker 必须回执，不允许挂起");
        assert!(result.is_err(), "未连接时打开通道应返回错误");
    }

    #[test]
    fn test_open_channel_without_connection_fails() {
        let (handle, _) = spawn_test_worker();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        handle
            .send(WorkerCommand::OpenChannel {
                ctype: core_common::ChannelType::Shell,
                reply: reply_tx,
            })
            .unwrap();
        let result = reply_rx.blocking_recv().unwrap();
        let err = result.expect_err("未连接时打开通道应返回错误");
        assert!(
            err.to_string().contains("no active connection"),
            "错误消息应明确说明无连接，而非骨架阶段的 not implemented 兜底。实际消息: {}",
            err,
        );
    }

    #[test]
    fn test_close_terminates_worker() {
        let (handle, _) = spawn_test_worker();
        handle.send(WorkerCommand::Close).unwrap();
        // 关闭命令后 worker 应退出（join 可完成）
        let join = handle.join.lock().unwrap().take().unwrap();
        join.join().expect("worker thread should exit after Close");
    }

    #[test]
    fn test_connect_rejects_invalid_host() {
        let (handle, state) = spawn_test_worker();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let config = ConnectionConfig::new(
            "nonexistent.invalid".into(),
            "root".into(),
            core_common::AuthMethod::Password("x".into()),
        )
        .with_timeout(1); // 1 秒超时，测试不挂起
        handle
            .send(WorkerCommand::Connect {
                config,
                known_hosts: None,
                reply: reply_tx,
            })
            .unwrap();
        let result = reply_rx.blocking_recv().unwrap();
        assert!(result.is_err(), "连接无效主机应返回错误");
        assert_eq!(*state.read().unwrap(), SessionState::Disconnected);
    }

    #[test]
    fn test_keepalive_due() {
        let start = std::time::Instant::now();
        assert!(!keepalive_due(
            None,
            start,
            std::time::Duration::from_secs(30)
        ));
        assert!(keepalive_due(
            Some(start - std::time::Duration::from_secs(31)),
            std::time::Instant::now(),
            std::time::Duration::from_secs(30)
        ));
        assert!(!keepalive_due(
            Some(start - std::time::Duration::from_secs(29)),
            std::time::Instant::now(),
            std::time::Duration::from_secs(30)
        ));
    }
}

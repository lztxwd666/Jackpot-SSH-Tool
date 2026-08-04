//! SSH 执行线程模块
//! 每 Session 一个 worker 线程，串行执行全部 ssh2 操作（Active Object 模式）
//! 事件循环: try_recv 处理命令 → 空闲工作（keepalive / shell 读轮询）→ sleep 25ms

#![allow(dead_code)]

use core_common::{ChannelId, ChannelType, ConnectionConfig, CoreError, CoreResult, FileEntry, KnownHostsProvider};
use core_event::EventDispatcher;
use std::sync::Arc;
use tokio::sync::oneshot;

/// worker 空闲工作间隔（毫秒）：shell 读轮询与命令响应延迟的上限
const IDLE_INTERVAL_MS: u64 = 25;

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
    ProvideCredential { secret: String },
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
        Self { tx, join: std::sync::Mutex::new(None) }
    }

    pub(crate) fn set_join(&self, join: std::thread::JoinHandle<()>) {
        *self.join.lock().unwrap() = Some(join);
    }

    /// 投递命令（不等待回执的变体）
    pub(crate) fn send(&self, cmd: WorkerCommand) -> CoreResult<()> {
        self.tx.send(cmd).map_err(|_| {
            core_common::CoreError::Internal("worker thread is gone".into())
        })
    }

    /// 投递命令并等待回执（调用方需处于阻塞上下文，如 spawn_blocking）
    pub(crate) fn call<T>(&self, cmd: WorkerCommand, rx: oneshot::Receiver<CoreResult<T>>) -> CoreResult<T> {
        self.send(cmd)?;
        rx.blocking_recv()
            .map_err(|_| core_common::CoreError::Internal("worker reply channel closed".into()))?
    }
}

/// worker 主循环入口（std::thread）
/// state 与 Session 共享（外部查询状态）；重连所需配置由命令携带（后续任务扩展）
pub(crate) fn run_loop(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<WorkerCommand>,
    state: Arc<std::sync::RwLock<core_common::SessionState>>,
    dispatcher: Arc<dyn EventDispatcher>,
) {
    let mut w = Worker::new(state, dispatcher);
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
    state: Arc<std::sync::RwLock<core_common::SessionState>>,
    dispatcher: Arc<dyn EventDispatcher>,
    closed: bool,
}

impl Worker {
    fn new(state: Arc<std::sync::RwLock<core_common::SessionState>>, dispatcher: Arc<dyn EventDispatcher>) -> Self {
        Self { state, dispatcher, closed: false }
    }

    /// 处理单条命令；返回 false 表示应结束线程
    /// 骨架阶段：所有带回执的命令回执 Err("not implemented")，保证调用方不挂起；
    /// 后续任务逐个替换为真实现
    fn handle(&mut self, cmd: WorkerCommand) -> bool {
        match cmd {
            WorkerCommand::Connect { reply, .. } => { let _ = reply.send(Err(CoreError::Internal("command not implemented yet".into()))); }
            WorkerCommand::Disconnect => {}
            WorkerCommand::Close => { self.closed = true; return false; }
            WorkerCommand::OpenChannel { reply, .. } => { let _ = reply.send(Err(CoreError::Internal("command not implemented yet".into()))); }
            WorkerCommand::ChannelWrite { reply, .. } => { let _ = reply.send(Err(CoreError::Internal("command not implemented yet".into()))); }
            WorkerCommand::ChannelResize { reply, .. } => { let _ = reply.send(Err(CoreError::Internal("command not implemented yet".into()))); }
            WorkerCommand::ChannelClose { reply, .. } => { let _ = reply.send(Err(CoreError::Internal("command not implemented yet".into()))); }
            WorkerCommand::Exec { reply, .. } => { let _ = reply.send(Err(CoreError::Internal("command not implemented yet".into()))); }
            WorkerCommand::SftpReadDir { reply, .. } => { let _ = reply.send(Err(CoreError::Internal("command not implemented yet".into()))); }
            WorkerCommand::SftpCreateDir { reply, .. } => { let _ = reply.send(Err(CoreError::Internal("command not implemented yet".into()))); }
            WorkerCommand::SftpRemove { reply, .. } => { let _ = reply.send(Err(CoreError::Internal("command not implemented yet".into()))); }
            WorkerCommand::SftpRename { reply, .. } => { let _ = reply.send(Err(CoreError::Internal("command not implemented yet".into()))); }
            WorkerCommand::SftpTransfer { reply, .. } => { let _ = reply.send(Err(CoreError::Internal("command not implemented yet".into()))); }
            WorkerCommand::ProvideCredential { .. } => {}
            WorkerCommand::CloseAllChannels => {}
        }
        !self.closed
    }

    /// 空闲工作：Task 4 起实现 keepalive 与 shell 读轮询
    fn do_idle_work(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_common::SessionState;
    use core_event::LoggingDispatcher;

    fn spawn_test_worker() -> (Arc<WorkerHandle>, Arc<std::sync::RwLock<SessionState>>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let state = Arc::new(std::sync::RwLock::new(SessionState::Created));
        let state_clone = state.clone();
        let dispatcher: Arc<dyn EventDispatcher> = Arc::new(LoggingDispatcher);
        let join = std::thread::spawn(move || run_loop(rx, state_clone, dispatcher));
        let handle = Arc::new(WorkerHandle::new(tx));
        handle.set_join(join);
        (handle, state)
    }

    #[test]
    fn test_worker_alive_after_spawn() {
        let (handle, _) = spawn_test_worker();
        // 骨架阶段：未实现命令必须回执错误而非挂起（否则调用方 blocking_recv 无限等待）
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        handle.send(WorkerCommand::OpenChannel {
            ctype: core_common::ChannelType::Shell,
            reply: reply_tx,
        }).unwrap();
        let result = reply_rx.blocking_recv().expect("worker 必须回执，不允许挂起");
        assert!(result.is_err(), "骨架阶段未实现命令应返回错误");
    }

    #[test]
    fn test_close_terminates_worker() {
        let (handle, _) = spawn_test_worker();
        handle.send(WorkerCommand::Close).unwrap();
        // 关闭命令后 worker 应退出（join 可完成）
        let join = handle.join.lock().unwrap().take().unwrap();
        join.join().expect("worker thread should exit after Close");
    }
}

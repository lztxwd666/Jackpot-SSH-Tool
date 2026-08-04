//! SSH 通道管理模块（Stage 6: 纯句柄）
//! Channel 不再持有 ssh2 资源，所有 SSH I/O 经 worker 线程串行执行

use core_common::{ChannelId, ChannelState, ChannelType, CoreError, CoreResult, PtySize, SessionId};
use core_event::event::{ChannelEvent, CoreEvent};
use core_event::EventDispatcher;
use std::sync::Arc;
use crate::worker::{WorkerCommand, WorkerHandle};

/// 内部通道类型（pub(crate) 供 worker 使用）
#[allow(dead_code)]
pub(crate) enum ChannelInner {
    Session(ssh2::Channel),
    Sftp(ssh2::Sftp),
}

/// SSH 数据通道（纯句柄：不持有 ssh2 资源，所有 I/O 经 worker 串行执行）
pub struct Channel {
    pub id: ChannelId,
    pub channel_type: ChannelType,
    pub session_id: SessionId,
    worker: Arc<WorkerHandle>,
}

/// PTY 终端模式 opcode 常量（参照 libssh2 定义）
const ECHO: u8 = 53;
const ICANON: u8 = 2;
const ISIG: u8 = 3;
const ICRNL: u8 = 36;
const ONLCR: u8 = 72;
const OPOST: u8 = 70;

impl Channel {
    pub(crate) fn new(
        id: ChannelId,
        channel_type: ChannelType,
        session_id: SessionId,
        worker: Arc<WorkerHandle>,
    ) -> Arc<Self> {
        Arc::new(Self { id, channel_type, session_id, worker })
    }

    /// 写入数据（异步：投递命令 + await oneshot）
    pub async fn write(&self, data: Vec<u8>) -> CoreResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.worker.send(WorkerCommand::ChannelWrite {
            channel: self.id, data, reply: reply_tx,
        })?;
        reply_rx.await
            .map_err(|_| CoreError::Internal("worker reply channel closed".into()))?
    }

    /// 调整 PTY 尺寸
    pub async fn resize_pty(&self, cols: u32, rows: u32) -> CoreResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.worker.send(WorkerCommand::ChannelResize {
            channel: self.id, cols, rows, reply: reply_tx,
        })?;
        reply_rx.await
            .map_err(|_| CoreError::Internal("worker reply channel closed".into()))?
    }

    /// 关闭通道（同步：调用方须处于阻塞上下文）
    pub fn close(&self) -> CoreResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.worker.call(
            WorkerCommand::ChannelClose { channel: self.id, reply: reply_tx },
            reply_rx,
        )
    }

    /// 启动后台读循环（worker 模型下读循环由 do_idle_work 承担，此为兼容 no-op）
    pub fn start_read_loop(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async {})
    }

    /// 读取通道数据（worker 模型下由 do_idle_work 承担，此为兼容 no-op）
    pub async fn read(&self, _len: usize) -> CoreResult<usize> {
        Ok(0)
    }

    pub fn state(&self) -> ChannelState {
        ChannelState::Open
    }

    pub fn is_open(&self) -> bool {
        true
    }

    // ========== SFTP 方法（Task 5 迁移至 worker，当前为兼容性桩） ==========

    pub fn sftp_read_dir(&self, _path: &str) -> CoreResult<Vec<core_common::FileEntry>> {
        Err(CoreError::Internal("SFTP not yet migrated to worker (Task 5)".into()))
    }

    pub fn sftp_create_dir(&self, _path: &str) -> CoreResult<()> {
        Err(CoreError::Internal("SFTP not yet migrated to worker (Task 5)".into()))
    }

    pub fn sftp_remove_file(&self, _path: &str) -> CoreResult<()> {
        Err(CoreError::Internal("SFTP not yet migrated to worker (Task 5)".into()))
    }

    pub fn sftp_remove_dir(&self, _path: &str) -> CoreResult<()> {
        Err(CoreError::Internal("SFTP not yet migrated to worker (Task 5)".into()))
    }

    pub fn sftp_rename(&self, _old: &str, _new: &str) -> CoreResult<()> {
        Err(CoreError::Internal("SFTP not yet migrated to worker (Task 5)".into()))
    }

    pub fn sftp_download_file<F>(&self, _remote_path: &str, _local_path: &str, _on_progress: F) -> CoreResult<u64>
    where F: FnMut(u64, u64)
    {
        Err(CoreError::Internal("SFTP not yet migrated to worker (Task 5)".into()))
    }

    pub fn sftp_upload_file<F>(&self, _remote_path: &str, _local_path: &str, _on_progress: F) -> CoreResult<u64>
    where F: FnMut(u64, u64)
    {
        Err(CoreError::Internal("SFTP not yet migrated to worker (Task 5)".into()))
    }
}

/// 创建 Shell 通道的原始 ssh2::Channel（供 worker 调用，不注册到外部队列）
pub(crate) fn open_shell_raw(
    ssh_session: &ssh2::Session,
    session_id: SessionId,
    channel_id: ChannelId,
    dispatcher: Arc<dyn EventDispatcher>,
) -> CoreResult<ssh2::Channel> {
    dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::Opening {
        session_id,
        channel_id,
        channel_type: ChannelType::Shell,
    }));

    let mut inner_ch = ssh_session.channel_session().map_err(|e| {
        CoreError::Internal(format!("open channel session failed: {e}"))
    })?;

    // 设置标准 PTY 终端模式（与 OpenSSH 行为对齐）
    let mut modes = ssh2::PtyModes::new();
    modes.set_boolean(ECHO, true);
    modes.set_boolean(ICANON, true);
    modes.set_boolean(ISIG, true);
    modes.set_boolean(ICRNL, true);
    modes.set_boolean(ONLCR, true);
    modes.set_boolean(OPOST, true);

    let pty = PtySize::default();
    inner_ch
        .request_pty(
            "xterm-256color",
            Some(modes),
            Some((pty.cols, pty.rows, pty.width_px, pty.height_px)),
        )
        .map_err(|e| CoreError::Internal(format!("request pty failed: {e}")))?;

    inner_ch
        .shell()
        .map_err(|e| CoreError::Internal(format!("start shell failed: {e}")))?;

    // 切换到非阻塞模式
    ssh_session.set_blocking(false);

    tracing::info!(%channel_id, %session_id, "shell channel opened (nonblocking)");
    Ok(inner_ch)
}

/// 创建 SFTP 通道的原始 ssh2::Sftp（供 worker 调用，不注册到外部队列）
pub(crate) fn open_sftp_raw(
    ssh_session: &ssh2::Session,
    session_id: SessionId,
    channel_id: ChannelId,
    dispatcher: Arc<dyn EventDispatcher>,
) -> CoreResult<ssh2::Sftp> {
    dispatcher.dispatch(CoreEvent::Channel(ChannelEvent::Opening {
        session_id,
        channel_id,
        channel_type: ChannelType::Sftp,
    }));

    // SFTP 初始化需要在阻塞模式下完成
    ssh_session.set_blocking(true);
    let sftp = ssh_session
        .sftp()
        .map_err(|e| CoreError::Internal(format!("open sftp failed: {e}")))?;
    ssh_session.set_blocking(false);

    tracing::info!(%channel_id, %session_id, "sftp channel opened");
    Ok(sftp)
}

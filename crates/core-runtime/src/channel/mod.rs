//! SSH 通道管理模块（Stage 6: 纯句柄）
//! Channel 不再持有 ssh2 资源，所有 SSH I/O 经 worker 线程串行执行

mod sftp;

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

/// PTY 终端模式 opcode 常量（RFC 4254 §8 定义；ICANON=51、ISIG=50，非 2/3）
const ECHO: u8 = 53;
const ICANON: u8 = 51;
const ISIG: u8 = 50;
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

    // ========== SFTP 方法（Task 5 迁移至 worker：投递命令 + 等待回执，见 sftp.rs） ==========
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

    // SFTP 初始化需要在阻塞模式下完成；失败时同样恢复非阻塞
    // （阻塞模式遗留会使 io_retry 的非阻塞假设失效，worker 可能被单次操作卡住）
    ssh_session.set_blocking(true);
    let result = ssh_session.sftp();
    ssh_session.set_blocking(false);
    let sftp = result.map_err(|e| CoreError::Internal(format!("open sftp failed: {e}")))?;

    tracing::info!(%channel_id, %session_id, "sftp channel opened");
    Ok(sftp)
}

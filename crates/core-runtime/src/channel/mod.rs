//! SSH 通道管理模块（Stage 6: 纯句柄）
//! Channel 不再持有 ssh2 资源，所有 SSH I/O 经 worker 线程串行执行
//! 依赖方向：本模块单向依赖 worker（投递命令）；原始 ssh2 通道的创建与
//! 注册表（ChannelInner）归属 worker，避免 worker ↔ channel 模块级循环依赖

mod sftp;

use core_common::{ChannelId, ChannelType, CoreError, CoreResult, SessionId};
use std::sync::Arc;
use crate::worker::{WorkerCommand, WorkerHandle};

/// SSH 数据通道（纯句柄：不持有 ssh2 资源，所有 I/O 经 worker 串行执行）
pub struct Channel {
    pub id: ChannelId,
    pub channel_type: ChannelType,
    pub session_id: SessionId,
    worker: Arc<WorkerHandle>,
}

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

    // SFTP 方法（Task 5 迁移至 worker：投递命令 + 等待回执，见 sftp.rs）
}

//! SFTP 操作子模块（Stage 6 Task 5：方法改投递命令到 worker 串行执行）
//! 实际 SFTP 执行在 worker 线程内；本模块的 Channel 方法为同步投递 + 回执等待，
//! 调用方须处于阻塞上下文（如 spawn_blocking）。旧"通道级锁 + EAGAIN 重试"
//! 模型已删除：worker 单线程无锁，重试逻辑内聚在 worker（sftp_retry_worker）。

use super::Channel;
use core_common::{CoreResult, FileEntry};
use crate::worker::{TransferKind, WorkerCommand};

impl Channel {
    /// 列出远程目录（同步：调用方须处于阻塞上下文）
    pub fn sftp_read_dir(&self, path: &str) -> CoreResult<Vec<FileEntry>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.worker.call(
            WorkerCommand::SftpReadDir {
                path: path.to_string(),
                reply: reply_tx,
            },
            reply_rx,
        )
    }

    /// 创建远程目录（权限 0o755）
    pub fn sftp_create_dir(&self, path: &str) -> CoreResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.worker.call(
            WorkerCommand::SftpCreateDir {
                path: path.to_string(),
                reply: reply_tx,
            },
            reply_rx,
        )
    }

    /// 删除远程文件
    pub fn sftp_remove_file(&self, path: &str) -> CoreResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.worker.call(
            WorkerCommand::SftpRemove {
                path: path.to_string(),
                is_dir: false,
                reply: reply_tx,
            },
            reply_rx,
        )
    }

    /// 删除远程目录
    pub fn sftp_remove_dir(&self, path: &str) -> CoreResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.worker.call(
            WorkerCommand::SftpRemove {
                path: path.to_string(),
                is_dir: true,
                reply: reply_tx,
            },
            reply_rx,
        )
    }

    /// 重命名远程文件或目录
    pub fn sftp_rename(&self, old: &str, new: &str) -> CoreResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.worker.call(
            WorkerCommand::SftpRename {
                old: old.to_string(),
                new: new.to_string(),
                reply: reply_tx,
            },
            reply_rx,
        )
    }

    /// 流式下载（同步）：投递 SftpTransfer 命令，进度经守护线程转发到 on_progress
    /// 回调约束 Send + 'static：守护线程独立调用（desktop 层回调为 tauri emit，线程安全）
    pub fn sftp_download_file<F>(
        &self,
        remote_path: &str,
        local_path: &str,
        on_progress: F,
    ) -> CoreResult<u64>
    where
        F: FnMut(u64, u64) + Send + 'static,
    {
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        // 守护线程：转发进度到回调（传输完成后由 drop 关闭通道使其退出，随后 join）
        let guard = std::thread::spawn(move || {
            let mut cb = on_progress;
            while let Some((done, total)) = progress_rx.blocking_recv() {
                cb(done, total);
            }
        });
        let r = self.worker.call(
            WorkerCommand::SftpTransfer {
                kind: TransferKind::Download,
                remote: remote_path.to_string(),
                local: local_path.to_string(),
                progress: progress_tx.clone(),
                reply: reply_tx,
            },
            reply_rx,
        );
        drop(progress_tx); // 关闭进度通道，守护线程消费完剩余消息后退出
        let _ = guard.join();
        if let Err(e) = &r {
            // 清理不完整的本地文件（旧实现行为保持：任何失败路径删除半成品）
            let _ = std::fs::remove_file(local_path);
            tracing::warn!(%remote_path, %local_path, error = %e, "download failed, partial local file removed");
        }
        r
    }

    /// 流式上传（同步）：投递 SftpTransfer 命令，进度经守护线程转发到 on_progress
    /// 失败时半成品远端文件由 worker 侧清理（transfer_upload_inner）
    pub fn sftp_upload_file<F>(
        &self,
        remote_path: &str,
        local_path: &str,
        on_progress: F,
    ) -> CoreResult<u64>
    where
        F: FnMut(u64, u64) + Send + 'static,
    {
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        // 守护线程：转发进度到回调（传输完成后由 drop 关闭通道使其退出，随后 join）
        let guard = std::thread::spawn(move || {
            let mut cb = on_progress;
            while let Some((done, total)) = progress_rx.blocking_recv() {
                cb(done, total);
            }
        });
        let r = self.worker.call(
            WorkerCommand::SftpTransfer {
                kind: TransferKind::Upload,
                remote: remote_path.to_string(),
                local: local_path.to_string(),
                progress: progress_tx.clone(),
                reply: reply_tx,
            },
            reply_rx,
        );
        drop(progress_tx); // 关闭进度通道，守护线程消费完剩余消息后退出
        let _ = guard.join();
        r
    }
}

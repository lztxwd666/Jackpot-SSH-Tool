//! worker 的传输实现（同模块多文件：impl Worker 方法跨文件可见，无模块级循环）
//! 流式单文件与目录递归传输：每 chunk 后嵌套处理命令队列，Locked/Unlocked 事件包裹

use super::{SFTP_CHUNK_SIZE, TransferKind, Worker, sftp_retry_io, write_full};
use crate::ssh::io_retry; // 统一的 EAGAIN 重试原语（定义在 ssh::retry，供全部 ssh2 操作共用）
use core_common::{CoreError, CoreResult};
use core_event::event::{CoreEvent, TransferDirection, TransferEvent};
use std::io::{Read, Write};
use tokio::sync::oneshot;

impl Worker {
    /// 传输：流式读写 + 每 chunk 后嵌套处理非传输命令
    /// 进入时广播 Transfer Locked，退出（成功/失败/取消）时广播 Unlocked
    pub(super) fn handle_transfer_inner(
        &mut self,
        kind: TransferKind,
        remote: &str,
        local: &str,
        progress: tokio::sync::mpsc::UnboundedSender<(u64, u64, String)>,
    ) -> CoreResult<u64> {
        self.run_transfer(kind, move |w| match kind {
            TransferKind::Download => w.transfer_download_inner(remote, local, &progress),
            TransferKind::Upload => w.transfer_upload_inner(remote, local, &progress),
        })
    }

    /// 目录递归传输（单文件命令与目录命令共用的 Locked/Unlocked 包裹）
    pub(super) fn handle_transfer_tree_inner(
        &mut self,
        kind: TransferKind,
        remote: &str,
        local: &str,
        progress: tokio::sync::mpsc::UnboundedSender<(u64, u64, String)>,
    ) -> CoreResult<u64> {
        self.run_transfer(kind, move |w| match kind {
            TransferKind::Download => w.transfer_tree_download_inner(remote, local, &progress),
            TransferKind::Upload => w.transfer_tree_upload_inner(remote, local, &progress),
        })
    }

    /// 传输执行包裹：广播 Locked/Unlocked、置位/复位 transferring 标志
    fn run_transfer<F>(&mut self, kind: TransferKind, f: F) -> CoreResult<u64>
    where
        F: FnOnce(&mut Self) -> CoreResult<u64>,
    {
        let sftp_channel_id = self
            .sftp_channel_id()
            .ok_or_else(|| CoreError::Internal("sftp channel not found".into()))?;
        self.transferring = true;
        self.dispatch(CoreEvent::Transfer(TransferEvent::Locked {
            session_id: self.session_id(),
            channel_id: sftp_channel_id,
            direction: match kind {
                TransferKind::Download => TransferDirection::Download,
                TransferKind::Upload => TransferDirection::Upload,
            },
        }));
        let result = f(self);
        self.dispatch(CoreEvent::Transfer(TransferEvent::Unlocked {
            session_id: self.session_id(),
            channel_id: sftp_channel_id,
        }));
        self.transferring = false;
        result
    }

    /// 传输命令公共处理（单文件/目录共用）：拒绝嵌套传输 → 执行 → 冲刷挂起的
    /// 断开/关闭（传输栈弹出后延迟释放连接，防 use-after-free）→ 回执
    pub(super) fn run_transfer_cmd(
        &mut self,
        reply: oneshot::Sender<CoreResult<u64>>,
        run: impl FnOnce(&mut Self) -> CoreResult<u64>,
    ) {
        if self.transferring {
            let _ = reply.send(Err(CoreError::Internal(
                "transfer already in progress".into(),
            )));
            return;
        }
        let r = run(self);
        self.flush_pending();
        let _ = reply.send(r);
    }

    /// 下载循环：每 chunk 后嵌套处理队列（终端输入/断开等立即响应）
    fn transfer_download_inner(
        &mut self,
        remote_path: &str,
        local_path: &str,
        progress: &tokio::sync::mpsc::UnboundedSender<(u64, u64, String)>,
    ) -> CoreResult<u64> {
        // 单文件命令：实时进度 + 空文件名；核心逻辑在 transfer_one_download（目录传输复用）
        self.transfer_one_download(remote_path, local_path, |done, total| {
            let _ = progress.send((done, total, String::new()));
        })
    }

    /// 单文件下载核心（不依赖命令上下文）：目录传输逐文件复用
    /// on_progress 由调用方决定进度上报（单文件实时 / 目录聚合）
    fn transfer_one_download<F>(
        &mut self,
        remote_path: &str,
        local_path: &str,
        mut on_progress: F,
    ) -> CoreResult<u64>
    where
        F: FnMut(u64, u64),
    {
        // 获取远程文件大小（用于进度条）
        let total = sftp_retry_io(&mut self.raw_channels, &self.cancel, "stat", |sftp| {
            sftp.stat(std::path::Path::new(remote_path))
        })?
        .size
        .unwrap_or(0);
        // 创建本地文件
        let mut local_file = std::fs::File::create(local_path)
            .map_err(|e| CoreError::Internal(format!("create local file failed: {e}")))?;
        // 打开远程文件（File 持有会话引用，不借用 sftp 句柄）
        let mut file = sftp_retry_io(&mut self.raw_channels, &self.cancel, "open", |sftp| {
            sftp.open(std::path::Path::new(remote_path))
        })?;
        let mut done: u64 = 0;
        let mut chunk = [0u8; SFTP_CHUNK_SIZE];
        loop {
            // 嵌套处理已到达命令（断开/输入/列目录等立即响应）
            self.drain_nested_commands();
            if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                // 取消（断开/Close）：显式错误走统一失败清理，避免 total==0 时误报成功
                return Err(CoreError::Internal("transfer cancelled".into()));
            }
            // 读块（EAGAIN 退避统一走 io_retry：libssh2 通道顺序约束，退避期间不得处理其他通道）
            let n = io_retry(|| file.read(&mut chunk), &self.cancel)
                .map_err(|e| CoreError::Internal(format!("sftp read failed: {e}")))?;
            if n == 0 {
                break;
            }
            local_file
                .write_all(&chunk[..n])
                .map_err(|e| CoreError::Internal(format!("local write failed: {e}")))?;
            done += n as u64;
            on_progress(done, total);
        }
        // 完整性校验：写入字节数必须与远端文件大小一致
        if total > 0 && done != total {
            return Err(CoreError::Internal(format!(
                "download size mismatch: got {done} bytes, expected {total}"
            )));
        }
        Ok(done)
    }

    // 注意（libssh2 通道顺序约束）：非阻塞模式下，一个通道的操作未完成（EAGAIN）时
    // 不得操作其他通道（"operations on one channel should complete before operations on
    // another begin"）——EAGAIN 退避期间若处理命令队列（如终端输入写 shell 通道），
    // 会破坏 libssh2 内部状态机导致连接错误（实测 SOCKET_SEND 失败断连）。
    // 因此传输的读/写统一走 io_retry（哑退避），命令队列只在 chunk 边界（无未完成操作）处理。
    // 传输期间终端输入的网络发送可能因带宽竞争排队（SSH 传输特性），属正常现象。

    /// 上传循环：每 chunk 后嵌套处理队列（断开/输入等立即响应）
    fn transfer_upload_inner(
        &mut self,
        remote_path: &str,
        local_path: &str,
        progress: &tokio::sync::mpsc::UnboundedSender<(u64, u64, String)>,
    ) -> CoreResult<u64> {
        // 单文件命令：实时进度 + 空文件名；核心逻辑在 transfer_one_upload（目录传输复用）
        self.transfer_one_upload(remote_path, local_path, |done, total| {
            let _ = progress.send((done, total, String::new()));
        })
    }

    /// 单文件上传核心（不依赖命令上下文）：目录传输逐文件复用
    /// 失败时清理不完整的远端文件（任何失败路径删除半成品）
    fn transfer_one_upload<F>(
        &mut self,
        remote_path: &str,
        local_path: &str,
        mut on_progress: F,
    ) -> CoreResult<u64>
    where
        F: FnMut(u64, u64),
    {
        // 获取本地文件大小（用于进度条）
        let total = std::fs::metadata(local_path)
            .map_err(|e| CoreError::Internal(format!("local metadata failed: {e}")))?
            .len();
        // 打开本地文件
        let mut local_file = std::fs::File::open(local_path)
            .map_err(|e| CoreError::Internal(format!("open local file failed: {e}")))?;
        // 创建远程文件
        let mut file = sftp_retry_io(&mut self.raw_channels, &self.cancel, "create", |sftp| {
            sftp.create(std::path::Path::new(remote_path))
        })?;
        let mut done: u64 = 0;
        let mut chunk = [0u8; SFTP_CHUNK_SIZE];
        let result = (|| -> CoreResult<u64> {
            loop {
                // 嵌套处理已到达命令（断开/输入/列目录等立即响应）
                self.drain_nested_commands();
                if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    // 取消（断开/Close）：显式错误走统一失败清理，避免 total==0 时误报成功
                    return Err(CoreError::Internal("transfer cancelled".into()));
                }
                let n = local_file
                    .read(&mut chunk)
                    .map_err(|e| CoreError::Internal(format!("local read failed: {e}")))?;
                if n == 0 {
                    break;
                }
                // 写入完整块（EAGAIN 与零进度统一由 write_full 处理；退避期间不得处理其他通道）
                write_full(&self.cancel, |buf| file.write(buf), &chunk[..n])
                    .map_err(|e| CoreError::Internal(format!("sftp write failed: {e}")))?;
                done += n as u64;
                on_progress(done, total);
            }
            // 完整性校验：远端文件大小必须与本地一致
            let remote_size =
                sftp_retry_io(&mut self.raw_channels, &self.cancel, "stat", |sftp| {
                    sftp.stat(std::path::Path::new(remote_path))
                })?
                .size
                .unwrap_or(0);
            if remote_size != done {
                return Err(CoreError::Internal(format!(
                    "upload size mismatch: remote has {remote_size} bytes, expected {done}"
                )));
            }
            Ok(done)
        })();
        if result.is_err() {
            // 清理不完整的远端文件（旧实现行为保持：任何失败路径删除半成品）
            if let Ok(sftp) = self.get_sftp() {
                let _ = sftp.unlink(std::path::Path::new(remote_path));
            }
            tracing::warn!(%remote_path, %local_path, "upload failed, partial remote file removed");
        }
        result
    }

    /// 目录递归传输（下载）：枚举远程目录树 → 逐文件复用 transfer_one_download，
    /// 进度按文件粒度上报聚合字节数（done/total 为全任务累计，第三位为当前文件相对路径）
    fn transfer_tree_download_inner(
        &mut self,
        remote_dir: &str,
        local_dir: &str,
        progress: &tokio::sync::mpsc::UnboundedSender<(u64, u64, String)>,
    ) -> CoreResult<u64> {
        // 枚举远程目录树（含空目录项）
        let mut entries = Vec::new();
        self.collect_remote_entries(remote_dir, "", &mut entries)?;
        // 空目录：本地创建根目录（无条目时循环不执行，必须显式创建）
        if entries.is_empty() {
            std::fs::create_dir_all(local_dir)
                .map_err(|e| CoreError::Internal(format!("create local dir failed: {e}")))?;
        }
        let total: u64 = entries.iter().filter(|e| !e.3).map(|e| e.2).sum();
        let mut done: u64 = 0;
        for (remote, rel, _size, is_dir) in entries {
            if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(CoreError::Internal("transfer cancelled".into()));
            }
            let local = std::path::Path::new(local_dir).join(&rel);
            if is_dir {
                // 空目录：本地创建对应目录（父目录在文件处理时已建）
                std::fs::create_dir_all(&local)
                    .map_err(|e| CoreError::Internal(format!("create local dir failed: {e}")))?;
                continue;
            }
            // 创建父目录后传输单个文件（实时进度不在此上报，文件完成上报聚合）
            if let Some(parent) = local.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| CoreError::Internal(format!("create local dir failed: {e}")))?;
            }
            let local_str = local.to_string_lossy().to_string();
            let n = match self.transfer_one_download(&remote, &local_str, |_d, _t| {}) {
                Ok(n) => n,
                Err(e) => {
                    // 失败清理当前文件半成品（与单文件下载路径行为一致）
                    let _ = std::fs::remove_file(&local_str);
                    return Err(e);
                }
            };
            done += n;
            let _ = progress.send((done, total, rel));
        }
        Ok(done)
    }

    /// 目录递归传输（上传）：枚举本地目录树 → 逐级创建远端目录 → 逐文件复用
    /// transfer_one_upload，进度按文件粒度上报聚合字节数
    /// 参数顺序统一为 (remote, local)（与下载一致，remote 为目标、local 为源）
    fn transfer_tree_upload_inner(
        &mut self,
        remote_dir: &str,
        local_dir: &str,
        progress: &tokio::sync::mpsc::UnboundedSender<(u64, u64, String)>,
    ) -> CoreResult<u64> {
        // 枚举本地目录树（含空目录项）
        let mut entries = Vec::new();
        self.collect_local_entries(std::path::Path::new(local_dir), "", &mut entries)?;
        // 目标根目录必须存在：先创建（已存在容忍），否则顶层文件 create 报父目录缺失
        self.sftp_mkdir_tolerant(remote_dir)?;
        let total: u64 = entries.iter().filter(|e| !e.3).map(|e| e.2).sum();
        let mut done: u64 = 0;
        for (local, rel, _size, is_dir) in entries {
            if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(CoreError::Internal("transfer cancelled".into()));
            }
            let remote = format!("{}/{}", remote_dir.trim_end_matches('/'), rel);
            if is_dir {
                // 远端创建目录（已存在容忍）
                self.sftp_mkdir_tolerant(&remote)?;
                continue;
            }
            let n = self.transfer_one_upload(&remote, &local, |_d, _t| {})?;
            done += n;
            let _ = progress.send((done, total, rel));
        }
        Ok(done)
    }

    /// 创建远端目录，已存在时容忍（stat 确认），其他错误传播
    fn sftp_mkdir_tolerant(&mut self, path: &str) -> CoreResult<()> {
        let mk = sftp_retry_io(&mut self.raw_channels, &self.cancel, "mkdir", |sftp| {
            sftp.mkdir(std::path::Path::new(path), 0o755)
        });
        if mk.is_ok() {
            return Ok(());
        }
        // mkdir 失败：stat 确认目标已存在（目录结构已就绪则容忍，否则传播原错误）
        let exists = sftp_retry_io(&mut self.raw_channels, &self.cancel, "stat", |sftp| {
            sftp.stat(std::path::Path::new(path))
        })
        .map(|s| s.is_dir())
        .unwrap_or(false);
        if exists {
            return Ok(());
        }
        mk
    }

    /// 递归枚举远程目录：条目为 (远程路径, 相对路径, 大小, 是否目录)
    /// 符号链接跳过（不跟随，防循环与目录误传）；每轮检查取消标志（大目录枚举可中断）
    fn collect_remote_entries(
        &mut self,
        dir: &str,
        rel: &str,
        out: &mut Vec<(String, String, u64, bool)>,
    ) -> CoreResult<()> {
        let entries = sftp_retry_io(&mut self.raw_channels, &self.cancel, "readdir", |sftp| {
            sftp.readdir(std::path::Path::new(dir))
        })?;
        for (p, stat) in entries {
            if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(CoreError::Internal("transfer cancelled".into()));
            }
            // 符号链接跳过（file_type 不跟随，防循环与目录误传）
            if matches!(stat.file_type(), ssh2::FileType::Symlink) {
                continue;
            }
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() || name == "." || name == ".." {
                continue;
            }
            let remote = format!("{}/{}", dir.trim_end_matches('/'), name);
            let rel_path = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if stat.is_dir() {
                out.push((remote.clone(), rel_path.clone(), 0, true));
                self.collect_remote_entries(&remote, &rel_path, out)?;
            } else {
                out.push((remote, rel_path, stat.size.unwrap_or(0), false));
            }
        }
        Ok(())
    }

    /// 递归枚举本地目录：条目为 (本地路径, 相对路径, 大小, 是否目录)
    /// 符号链接跳过（file_type 不跟随，防循环）；每轮检查取消标志（大目录枚举可中断）
    fn collect_local_entries(
        &self,
        dir: &std::path::Path,
        rel: &str,
        out: &mut Vec<(String, String, u64, bool)>,
    ) -> CoreResult<()> {
        let entries = std::fs::read_dir(dir)
            .map_err(|e| CoreError::Internal(format!("read local dir failed: {e}")))?;
        for entry in entries {
            if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(CoreError::Internal("transfer cancelled".into()));
            }
            let entry = entry
                .map_err(|e| CoreError::Internal(format!("read local entry failed: {e}")))?;
            let name = entry.file_name().to_string_lossy().to_string();
            let rel_path = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let ft = entry
                .file_type()
                .map_err(|e| CoreError::Internal(format!("local file type failed: {e}")))?;
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                out.push((entry.path().to_string_lossy().to_string(), rel_path.clone(), 0, true));
                self.collect_local_entries(&entry.path(), &rel_path, out)?;
            } else {
                out.push((
                    entry.path().to_string_lossy().to_string(),
                    rel_path,
                    entry.metadata().map(|m| m.len()).unwrap_or(0),
                    false,
                ));
            }
        }
        Ok(())
    }
}

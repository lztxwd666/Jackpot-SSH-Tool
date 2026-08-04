//! SFTP 操作子模块
//! 提供 Channel 的 SFTP 文件传输操作方法
//! 非阻塞模式下 SFTP 操作会返回 EAGAIN，通过 sftp_retry 自动重试

use super::{Channel, ChannelInner};
use core_common::{CoreResult, FileEntry};
use std::io::{Read, Write};

/// 流式传输分块大小（64KB）：减少循环次数加速大文件，兼顾内存占用
const SFTP_CHUNK_SIZE: usize = 65536;

/// 传输中 EAGAIN 重试的基础间隔（毫秒），指数退避上限
const EAGAIN_BASE_DELAY_MS: u64 = 2;
const EAGAIN_MAX_DELAY_MS: u64 = 32;

/// 快速 SFTP 操作（列目录/增删改）的有界锁等待超时（毫秒）
/// 传输进行中时，等待此时间后返回明确错误而非无限阻塞
const SFTP_LOCK_WAIT_MS: u64 = 2000;

impl Channel {
    // 快速操作（列目录/增删改）使用有界锁等待：传输进行中时返回明确错误而不是无限阻塞。
    // 文件传输（下载/上传）流式读写，全程持有锁保证 libssh2 会话串行。

    /// 有界等待 SFTP 锁：传输进行中时最多等待 timeout_ms 后返回明确错误
    fn lock_sftp_bounded(
        &self,
        timeout_ms: u64,
    ) -> CoreResult<std::sync::MutexGuard<'_, Option<ChannelInner>>> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            match self.inner.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(std::sync::TryLockError::WouldBlock) => {
                    if std::time::Instant::now() >= deadline {
                        return Err(core_common::CoreError::Internal(
                            "SFTP busy (transfer in progress), retry later".into(),
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(e) => {
                    return Err(core_common::CoreError::Internal(format!("lock inner: {e}")));
                }
            }
        }
    }

    /// 列出目录内容
    pub fn sftp_read_dir(&self, path: &str) -> CoreResult<Vec<FileEntry>> {
        let guard = self.lock_sftp_bounded(SFTP_LOCK_WAIT_MS)?;
        let sftp = get_sftp(guard.as_ref())?;

        let entries = sftp_retry(|| sftp.readdir(std::path::Path::new(path)))
            .map_err(|e| core_common::CoreError::Internal(format!("sftp readdir failed: {e}")))?;

        // 构建绝对路径
        let base = path.trim_end_matches('/');
        let mut files = Vec::new();
        for (p, stat) in entries {
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            // 过滤特殊条目与无法表示文件名的条目
            if name.is_empty() || name == "." || name == ".." {
                continue;
            }
            files.push(FileEntry {
                name: name.clone(),
                path: format!("{}/{}", base, name),
                size: stat.size.unwrap_or(0),
                is_dir: stat.is_dir(),
                modified: format!("{}", stat.mtime.unwrap_or(0)),
            });
        }
        files.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(files)
    }

    pub fn sftp_create_dir(&self, path: &str) -> CoreResult<()> {
        let guard = self.lock_sftp_bounded(SFTP_LOCK_WAIT_MS)?;
        let sftp = get_sftp(guard.as_ref())?;
        sftp_retry(|| sftp.mkdir(std::path::Path::new(path), 0o755))
            .map_err(|e| core_common::CoreError::Internal(format!("sftp mkdir failed: {e}")))?;
        Ok(())
    }

    pub fn sftp_remove_file(&self, path: &str) -> CoreResult<()> {
        let guard = self.lock_sftp_bounded(SFTP_LOCK_WAIT_MS)?;
        let sftp = get_sftp(guard.as_ref())?;
        sftp_retry(|| sftp.unlink(std::path::Path::new(path)))
            .map_err(|e| core_common::CoreError::Internal(format!("sftp unlink failed: {e}")))?;
        Ok(())
    }

    pub fn sftp_remove_dir(&self, path: &str) -> CoreResult<()> {
        let guard = self.lock_sftp_bounded(SFTP_LOCK_WAIT_MS)?;
        let sftp = get_sftp(guard.as_ref())?;
        sftp_retry(|| sftp.rmdir(std::path::Path::new(path)))
            .map_err(|e| core_common::CoreError::Internal(format!("sftp rmdir failed: {e}")))?;
        Ok(())
    }

    pub fn sftp_rename(&self, old: &str, new: &str) -> CoreResult<()> {
        let guard = self.lock_sftp_bounded(SFTP_LOCK_WAIT_MS)?;
        let sftp = get_sftp(guard.as_ref())?;
        let old_path = std::path::Path::new(old);
        let new_path = std::path::Path::new(new);
        sftp_retry(|| sftp.rename(old_path, new_path, None))
            .map_err(|e| core_common::CoreError::Internal(format!("sftp rename failed: {e}")))?;
        Ok(())
    }

    /// 流式下载：远程文件分块读取，直接写入本地文件
    /// 进度回调 on_progress(done_bytes, total_bytes)，每 ~128KB 或完成时调用
    pub fn sftp_download_file<F>(
        &self,
        remote_path: &str,
        local_path: &str,
        mut on_progress: F,
    ) -> CoreResult<u64>
    where
        F: FnMut(u64, u64),
    {
        // 传输主体包在闭包内：任何错误路径统一在闭包外清理不完整的本地文件
        let result = (|| -> CoreResult<u64> {
            // 获取远程文件大小（用于进度条）
            let total = {
                let guard = self.lock_sftp_bounded(SFTP_LOCK_WAIT_MS)?;
                let sftp = get_sftp(guard.as_ref())?;
                let stat =
                    sftp_retry(|| sftp.stat(std::path::Path::new(remote_path))).map_err(|e| {
                        core_common::CoreError::Internal(format!("sftp stat failed: {e}"))
                    })?;
                stat.size.unwrap_or(0)
            };

            // 创建本地文件
            let mut local = std::fs::File::create(local_path).map_err(|e| {
                core_common::CoreError::Internal(format!("create local file failed: {e}"))
            })?;

            // 流式读取（传输期间持有锁，保证 libssh2 会话串行）
            let guard = self
                .inner
                .lock()
                .map_err(|e| core_common::CoreError::Internal(format!("lock inner: {e}")))?;
            let sftp = get_sftp(guard.as_ref())?;
            let mut file = sftp_retry(|| sftp.open(std::path::Path::new(remote_path)))
                .map_err(|e| core_common::CoreError::Internal(format!("sftp open failed: {e}")))?;

            let mut done: u64 = 0;
            let mut retry_delay: u64 = EAGAIN_BASE_DELAY_MS;
            let mut chunk = [0u8; SFTP_CHUNK_SIZE];
            loop {
                match file.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        retry_delay = EAGAIN_BASE_DELAY_MS; // 传输恢复，重置退避
                        local.write_all(&chunk[..n]).map_err(|e| {
                            core_common::CoreError::Internal(format!("local write failed: {e}"))
                        })?;
                        done += n as u64;
                        on_progress(done, total);
                    }
                    Err(e) => {
                        if crate::ssh::is_would_block(&e) {
                            // 指数退避：2ms → 4ms → ... → 32ms 封顶
                            std::thread::sleep(std::time::Duration::from_millis(retry_delay));
                            retry_delay = (retry_delay * 2).min(EAGAIN_MAX_DELAY_MS);
                            continue;
                        }
                        return Err(core_common::CoreError::Internal(format!(
                            "sftp read failed: {e}"
                        )));
                    }
                }
            }

            // 完整性校验：写入字节数必须与远端文件大小一致
            if total > 0 && done != total {
                return Err(core_common::CoreError::Internal(format!(
                    "download size mismatch: got {done} bytes, expected {total}"
                )));
            }
            Ok(done)
        })();

        if let Err(e) = &result {
            // 清理不完整的本地文件（闭包已返回，文件句柄已释放，可安全删除）
            let _ = std::fs::remove_file(local_path);
            tracing::warn!(%remote_path, %local_path, error = %e, "download failed, partial local file removed");
        }
        result
    }

    /// 流式上传：本地文件分块读取，写入远程文件
    /// 进度回调 on_progress(done_bytes, total_bytes)，每 ~128KB 或完成时调用
    pub fn sftp_upload_file<F>(
        &self,
        remote_path: &str,
        local_path: &str,
        mut on_progress: F,
    ) -> CoreResult<u64>
    where
        F: FnMut(u64, u64),
    {
        // 传输主体包在闭包内：任何错误路径统一在闭包外清理不完整的远端文件
        let result = (|| -> CoreResult<u64> {
            // 获取本地文件大小（用于进度条）
            let total = std::fs::metadata(local_path)
                .map_err(|e| {
                    core_common::CoreError::Internal(format!("local metadata failed: {e}"))
                })?
                .len();

            // 打开本地文件
            let mut local = std::fs::File::open(local_path).map_err(|e| {
                core_common::CoreError::Internal(format!("open local file failed: {e}"))
            })?;

            // 流式写入（传输期间持有锁，保证 libssh2 会话串行）
            let guard = self
                .inner
                .lock()
                .map_err(|e| core_common::CoreError::Internal(format!("lock inner: {e}")))?;
            let sftp = get_sftp(guard.as_ref())?;
            let mut file =
                sftp_retry(|| sftp.create(std::path::Path::new(remote_path))).map_err(|e| {
                    core_common::CoreError::Internal(format!("sftp create failed: {e}"))
                })?;

            let mut done: u64 = 0;
            let mut retry_delay: u64 = EAGAIN_BASE_DELAY_MS;
            let mut chunk = [0u8; SFTP_CHUNK_SIZE];
            loop {
                let n = local.read(&mut chunk).map_err(|e| {
                    core_common::CoreError::Internal(format!("local read failed: {e}"))
                })?;
                if n == 0 {
                    break;
                }
                // 写入完整块（处理 EAGAIN 和部分写入）
                let mut off = 0;
                while off < n {
                    match file.write(&chunk[off..n]) {
                        Ok(w) => off += w,
                        Err(e) => {
                            let msg = format!("{e}").to_lowercase();
                            if msg.contains("would block") || msg.contains("-37") {
                                // 指数退避：2ms → 4ms → ... → 32ms 封顶
                                std::thread::sleep(std::time::Duration::from_millis(retry_delay));
                                retry_delay = (retry_delay * 2).min(EAGAIN_MAX_DELAY_MS);
                                continue;
                            }
                            return Err(core_common::CoreError::Internal(format!(
                                "sftp write failed: {e}"
                            )));
                        }
                    }
                }
                retry_delay = EAGAIN_BASE_DELAY_MS; // 传输恢复，重置退避
                done += n as u64;
                on_progress(done, total);
            }

            // 完整性校验：远端文件大小必须与本地文件一致
            let remote_size = sftp_retry(|| sftp.stat(std::path::Path::new(remote_path)))
                .map_err(|e| core_common::CoreError::Internal(format!("sftp stat failed: {e}")))?
                .size
                .unwrap_or(0);
            if remote_size != done {
                return Err(core_common::CoreError::Internal(format!(
                    "upload size mismatch: remote has {remote_size} bytes, expected {done}"
                )));
            }
            Ok(done)
        })();

        if let Err(e) = &result {
            // 清理不完整的远端文件
            if let Ok(guard) = self.inner.lock()
                && let Some(ChannelInner::Sftp(ref sftp)) = *guard
            {
                let _ = sftp.unlink(std::path::Path::new(remote_path));
            }
            tracing::warn!(%remote_path, %local_path, error = %e, "upload failed, partial remote file removed");
        }
        result
    }
}

/// 从 Option<ChannelInner> 中提取 SFTP 引用
fn get_sftp(inner: Option<&ChannelInner>) -> CoreResult<&ssh2::Sftp> {
    match inner {
        Some(ChannelInner::Sftp(sftp)) => Ok(sftp),
        _ => Err(core_common::CoreError::Internal(
            "channel is not an SFTP channel".into(),
        )),
    }
}

/// SFTP 操作重试包装：非阻塞模式下 SFTP 操作可能返回 EAGAIN
/// 最多重试 20 次，每次间隔 50ms（总计 1 秒）
fn sftp_retry<T, F>(mut f: F) -> Result<T, ssh2::Error>
where
    F: FnMut() -> Result<T, ssh2::Error>,
{
    let mut attempts = 0;
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                let msg = format!("{e}").to_lowercase();
                if (msg.contains("would block") || msg.contains("-37")) && attempts < 20 {
                    attempts += 1;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }
                return Err(e);
            }
        }
    }
}

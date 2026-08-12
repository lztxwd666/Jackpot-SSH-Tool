//! worker 的 sftp 单操作（同模块多文件：impl Worker 方法跨文件可见，无模块级循环）
//! 目录浏览/创建/删除/重命名等一次性操作；流式与目录传输见 transfer.rs

use super::{ChannelInner, Worker, sftp_retry_io};
use core_common::{ChannelId, CoreError, CoreResult, FileEntry};

impl Worker {
    /// 找到 Sftp 通道的原始句柄（每个 session 仅一个 sftp 通道，取第一个）
    pub(super) fn get_sftp(&mut self) -> CoreResult<&mut ssh2::Sftp> {
        for inner in self.raw_channels.values_mut() {
            if let ChannelInner::Sftp(sftp) = inner {
                return Ok(sftp);
            }
        }
        Err(CoreError::Internal("sftp channel not found".into()))
    }

    /// 当前 Sftp 通道 id（与 get_sftp 同源，用于传输事件广播）
    pub(super) fn sftp_channel_id(&self) -> Option<ChannelId> {
        self.raw_channels
            .iter()
            .find_map(|(id, inner)| matches!(inner, ChannelInner::Sftp(_)).then_some(*id))
    }

    /// 列出远程目录（过滤 . ..、构建绝对路径、目录优先排序）
    pub(super) fn sftp_read_dir_inner(&mut self, path: &str) -> CoreResult<Vec<FileEntry>> {
        let entries = sftp_retry_io(&mut self.raw_channels, &self.cancel, "readdir", |sftp| {
            sftp.readdir(std::path::Path::new(path))
        })?;
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

    /// 创建远程目录（权限 0o755）
    pub(super) fn sftp_create_dir_inner(&mut self, path: &str) -> CoreResult<()> {
        sftp_retry_io(&mut self.raw_channels, &self.cancel, "mkdir", |sftp| {
            sftp.mkdir(std::path::Path::new(path), 0o755)
        })?;
        Ok(())
    }

    /// 新建远程空文件（sftp.create 为 WRITE|CREAT|TRUNC：前端冲突检测保证到达
    /// 此处的名字要么唯一（新建）要么用户已确认覆盖（截断符合覆盖语义））
    pub(super) fn sftp_create_file_inner(&mut self, path: &str) -> CoreResult<()> {
        let _file = sftp_retry_io(&mut self.raw_channels, &self.cancel, "create file", |sftp| {
            sftp.create(std::path::Path::new(path))
        })?;
        Ok(())
    }

    /// 删除远程文件或目录（unlink / rmdir）
    pub(super) fn sftp_remove_inner(&mut self, path: &str, is_dir: bool) -> CoreResult<()> {
        if is_dir {
            sftp_retry_io(&mut self.raw_channels, &self.cancel, "rmdir", |sftp| {
                sftp.rmdir(std::path::Path::new(path))
            })?;
        } else {
            sftp_retry_io(&mut self.raw_channels, &self.cancel, "unlink", |sftp| {
                sftp.unlink(std::path::Path::new(path))
            })?;
        }
        Ok(())
    }

    /// 重命名远程文件或目录
    pub(super) fn sftp_rename_inner(&mut self, old: &str, new: &str) -> CoreResult<()> {
        let old_path = std::path::Path::new(old);
        let new_path = std::path::Path::new(new);
        sftp_retry_io(&mut self.raw_channels, &self.cancel, "rename", |sftp| {
            sftp.rename(old_path, new_path, None)
        })?;
        Ok(())
    }
}

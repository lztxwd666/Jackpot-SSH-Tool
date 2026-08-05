//! SSH 连接相关的基础类型定义

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// SSH 连接配置，包含目标主机、端口、用户名和认证方式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
    pub timeout_secs: u64,
}

impl ConnectionConfig {
    /// 创建一个新的连接配置，默认端口 22、超时 30 秒
    pub fn new(host: String, username: String, auth_method: AuthMethod) -> Self {
        Self {
            host,
            port: 22,
            username,
            auth_method,
            timeout_secs: 30,
        }
    }

    /// 设置自定义端口
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// 设置自定义超时
    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.timeout_secs = seconds;
        self
    }
}

/// SSH 认证方式
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "data")]
pub enum AuthMethod {
    /// 密码认证
    Password(String),
    /// 私钥文件认证，可选口令
    PrivateKey {
        path: PathBuf,
        passphrase: Option<String>,
    },
    /// SSH Agent 认证（暂未实现）
    Agent,
}

impl std::fmt::Debug for AuthMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Password(_) => f.debug_tuple("Password").field(&"***").finish(),
            Self::PrivateKey { path, passphrase } => f
                .debug_struct("PrivateKey")
                .field("path", path)
                .field("passphrase", &passphrase.as_ref().map(|_| "***"))
                .finish(),
            Self::Agent => f.debug_tuple("Agent").finish(),
        }
    }
}

/// 主机密钥信息，用于 known_hosts 验证
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostKeyInfo {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
}

impl HostKeyInfo {
    pub fn new(host: String, port: u16, key_type: String, fingerprint: String) -> Self {
        Self {
            host,
            port,
            key_type,
            fingerprint,
        }
    }
}

/// Session 生命周期状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    Created,
    Connecting,
    Connected,
    Disconnected,
    Closed,
}

/// SSH 通道类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelType {
    Shell,
    Sftp,
}

/// 通道生命周期状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelState {
    Opening,
    Open,
    Closing,
    Closed,
}

/// PTY 终端窗口尺寸
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PtySize {
    pub cols: u32,
    pub rows: u32,
    pub width_px: u32,
    pub height_px: u32,
}

impl Default for PtySize {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            width_px: 0,
            height_px: 0,
        }
    }
}

/// SFTP 文件/目录条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub modified: String,
}

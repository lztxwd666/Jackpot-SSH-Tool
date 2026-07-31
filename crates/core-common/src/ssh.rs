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
    PrivateKey { path: PathBuf, passphrase: Option<String> },
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
        Self { host, port, key_type, fingerprint }
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
    Exec,
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
        Self { cols: 80, rows: 24, width_px: 0, height_px: 0 }
    }
}

/// 重连策略配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectPolicy {
    /// 最大重试次数，0 表示不重连
    pub max_retries: u32,
    /// 指数退避的基础延迟（秒）
    pub base_delay_secs: u64,
    /// 最大延迟上限（秒）
    pub max_delay_secs: u64,
}

impl ReconnectPolicy {
    /// 计算第 attempt 次重试的延迟（attempt 从 1 开始）
    pub fn delay_for(&self, attempt: u32) -> u64 {
        let delay = self.base_delay_secs.saturating_mul(2u64.saturating_pow(attempt.saturating_sub(1)));
        delay.min(self.max_delay_secs)
    }
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            max_retries: 5,
            base_delay_secs: 1,
            max_delay_secs: 30,
        }
    }
}

//! Credential Provider 抽象
//! 定义认证凭据的加载、保存、删除接口
//!
//! 实现：OS 凭据管理器（keyring），见 core-runtime::credential。
//! 凭据值绝不应持久化到 SQLite。

use crate::CoreResult;
use serde::{Deserialize, Serialize};

/// 凭据种类（决定 keyring 的 user 前缀与前端文案）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CredentialKind {
    Password,
    Passphrase,
}

/// 一条已保存的凭据（secret 为内存值，不出现在事件与持久化中）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub kind: CredentialKind,
    pub secret: String,
}

/// 提供 SSH 认证所需的凭据（实现：OS 凭据管理器，见 core-runtime::credential）
pub trait CredentialProvider: Send + Sync {
    /// 读取已保存的凭据；未保存返回 None
    fn load_credential(
        &self,
        host: &str,
        port: u16,
        username: &str,
        kind: CredentialKind,
    ) -> CoreResult<Option<Credential>>;
    /// 保存凭据（OS 凭据管理器，加密存储）
    fn save_credential(
        &self,
        host: &str,
        port: u16,
        username: &str,
        credential: &Credential,
    ) -> CoreResult<()>;
    /// 删除已保存的凭据
    fn delete_credential(
        &self,
        host: &str,
        port: u16,
        username: &str,
        kind: CredentialKind,
    ) -> CoreResult<()>;
}

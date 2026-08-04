//! Credential Provider 抽象
//! 定义认证凭据的加载接口
//!
//! 注意：当前实现通过 ConnectionConfig 直接传递凭据（密码/私钥），
//! 此 trait 为未来接入操作系统凭据库（OS Keychain / Credential Manager）预留的扩展点。
//! 凭据值绝不应持久化到 SQLite。

use crate::{AuthMethod, CoreResult};

/// 提供 SSH 认证所需的凭据
pub trait CredentialProvider: Send + Sync {
    fn load_credential(&self, host: &str, username: &str) -> CoreResult<AuthMethod>;
}

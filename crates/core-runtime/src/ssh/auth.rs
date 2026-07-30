//! SSH 认证逻辑模块
//! 支持密码和私钥文件两种认证方式，Agent 认证留待未来实现

use core_common::{AuthMethod, CoreResult};
use ssh2::Session;

/// 使用 ConnectionConfig 中指定的认证方式对 SSH session 进行认证
/// 按顺序尝试密码认证和私钥认证，任一种成功即返回 Ok
pub fn authenticate(session: &Session, username: &str, auth_method: &AuthMethod) -> CoreResult<()> {
    match auth_method {
        AuthMethod::Password(password) => {
            session.userauth_password(username, password)
                .map_err(|e| core_common::CoreError::Internal(format!("password auth failed: {e}")))?;
            tracing::info!(username, "password authentication succeeded");
        }
        AuthMethod::PrivateKey { path, passphrase } => {
            let key_data = std::fs::read_to_string(path)
                .map_err(|e| core_common::CoreError::Internal(format!("failed to read private key {}: {e}", path.display())))?;
            let pass = passphrase.as_deref();
            session.userauth_pubkey_memory(username, None, &key_data, pass)
                .map_err(|e| core_common::CoreError::Internal(format!("publickey auth failed: {e}")))?;
            tracing::info!(username, key_path = %path.display(), "publickey authentication succeeded");
        }
        AuthMethod::Agent => {
            return Err(core_common::CoreError::Internal("SSH agent authentication not yet supported".into()));
        }
    }
    Ok(())
}

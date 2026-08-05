//! OS 凭据管理器实现（keyring crate）
//! 密码/私钥口令加密存储于系统凭据库（Windows Credential Manager / macOS Keychain /
//! Linux Secret Service），SQLite 零密码，符合 ADR-0008"凭据属于操作系统"。
//! 存储键：service = "jackpot-ssh"，user = "{kind}:{host}:{port}:{username}"，
//! 可读性好且按已知键可删。

use core_common::credential::{Credential, CredentialKind, CredentialProvider};
use core_common::CoreResult;
use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "jackpot-ssh";

/// 基于 keyring 的凭据提供者
/// 内部注入 keyring::CredentialBuilder（生产用平台默认，测试用 mock builder）
pub struct KeyringCredentialProvider {
    builder: Box<keyring::CredentialBuilder>,
    /// 按存储键缓存的 Entry：mock 的凭据数据存在 Entry 对象内，必须复用同一对象
    /// 才能跨调用存取；真实 OS 凭据库中同键 Entry 共享底层凭据，缓存无副作用
    entries: Mutex<HashMap<String, keyring::Entry>>,
}

impl KeyringCredentialProvider {
    pub fn new() -> Self {
        Self {
            builder: keyring::default::default_credential_builder(),
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// 测试构造：注入 mock builder，不触碰真实系统凭据库
    #[cfg(test)]
    fn with_mock_store() -> Self {
        Self {
            builder: keyring::mock::default_credential_builder(),
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// 存储键（keyring 的 user 部分）："{kind}:{host}:{port}:{username}"
    fn storage_key(&self, kind: CredentialKind, host: &str, port: u16, username: &str) -> String {
        let kind_prefix = match kind {
            CredentialKind::Password => "password",
            CredentialKind::Passphrase => "passphrase",
        };
        format!("{kind_prefix}:{host}:{port}:{username}")
    }

    /// 取指定键的 Entry（按需创建并缓存），在其上执行闭包
    /// 创建失败（键非法等）映射为 Internal 错误
    fn with_entry<R>(
        &self,
        kind: CredentialKind,
        host: &str,
        port: u16,
        username: &str,
        f: impl FnOnce(&keyring::Entry) -> CoreResult<R>,
    ) -> CoreResult<R> {
        let key = self.storage_key(kind, host, port, username);
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if !entries.contains_key(&key) {
            let credential = self
                .builder
                .build(None, SERVICE, &key)
                .map_err(|e| core_common::CoreError::Internal(format!("credential entry failed: {e}")))?;
            entries.insert(key.clone(), keyring::Entry::new_with_credential(credential));
        }
        f(entries.get(&key).expect("entry 刚插入，必然存在"))
    }
}

impl Default for KeyringCredentialProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialProvider for KeyringCredentialProvider {
    fn load_credential(
        &self,
        host: &str,
        port: u16,
        username: &str,
        kind: CredentialKind,
    ) -> CoreResult<Option<Credential>> {
        self.with_entry(kind, host, port, username, |entry| match entry.get_password() {
            Ok(secret) => Ok(Some(Credential { kind, secret })),
            Err(keyring::Error::NoEntry) => Ok(None), // 未保存：正常路径
            Err(e) => Err(core_common::CoreError::Internal(format!(
                "credential load failed: {e}"
            ))),
        })
    }

    fn save_credential(
        &self,
        host: &str,
        port: u16,
        username: &str,
        credential: &Credential,
    ) -> CoreResult<()> {
        self.with_entry(credential.kind, host, port, username, |entry| {
            entry
                .set_password(&credential.secret)
                .map_err(|e| core_common::CoreError::Internal(format!("credential save failed: {e}")))
        })
    }

    fn delete_credential(
        &self,
        host: &str,
        port: u16,
        username: &str,
        kind: CredentialKind,
    ) -> CoreResult<()> {
        self.with_entry(kind, host, port, username, |entry| match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // 未保存：幂等
            Err(e) => Err(core_common::CoreError::Internal(format!(
                "credential delete failed: {e}"
            ))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> KeyringCredentialProvider {
        KeyringCredentialProvider::with_mock_store()
    }

    #[test]
    fn test_save_and_load_password() {
        let p = provider();
        let host = "192.168.1.10";
        let cred = Credential { kind: CredentialKind::Password, secret: "secret-pw".into() };
        p.save_credential(host, 22, "root", &cred).unwrap();
        let loaded = p.load_credential(host, 22, "root", CredentialKind::Password).unwrap();
        assert_eq!(loaded, Some(cred));
    }

    #[test]
    fn test_load_missing_returns_none() {
        let p = provider();
        let loaded = p.load_credential("10.0.0.1", 22, "root", CredentialKind::Password).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_kind_isolation() {
        // 密码与口令使用不同键，互不干扰
        let p = provider();
        p.save_credential("h", 22, "u", &Credential { kind: CredentialKind::Password, secret: "pw".into() }).unwrap();
        let pass = p.load_credential("h", 22, "u", CredentialKind::Passphrase).unwrap();
        assert!(pass.is_none());
    }

    #[test]
    fn test_delete_credential() {
        let p = provider();
        p.save_credential("h", 22, "u", &Credential { kind: CredentialKind::Password, secret: "pw".into() }).unwrap();
        p.delete_credential("h", 22, "u", CredentialKind::Password).unwrap();
        let loaded = p.load_credential("h", 22, "u", CredentialKind::Password).unwrap();
        assert!(loaded.is_none());
        // 删除未保存的键：幂等
        p.delete_credential("h", 22, "u", CredentialKind::Passphrase).unwrap();
    }
}

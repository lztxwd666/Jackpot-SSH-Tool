//! HostKey 验证模块
//! 从 SSH session 获取远程主机指纹，用于 known_hosts 校验

use core_common::{CoreResult, HostKeyInfo};
use ssh2::HostKeyType;

/// 从已建立 SSH 握手的 session 获取远程主机的密钥信息
/// 返回 Base64 编码的 SHA-256 指纹（与 OpenSSH 格式兼容）
pub fn check_host_key(session: &ssh2::Session, host: &str, port: u16) -> CoreResult<HostKeyInfo> {
    let (_key, raw_key_type) = session.host_key()
        .ok_or_else(|| core_common::CoreError::Internal("no host key available from session".into()))?;
    let key_type = key_to_type_name(raw_key_type);
    let hash = session.host_key_hash(ssh2::HashType::Sha256)
        .ok_or_else(|| core_common::CoreError::Internal("failed to compute host key hash".into()))?;
    let fingerprint = format_fingerprint(hash);
    Ok(HostKeyInfo {
        host: host.to_string(),
        port,
        key_type,
        fingerprint,
    })
}

/// 将 ssh2 的 HostKeyType 转换为字符串表示
fn key_to_type_name(key_type_num: HostKeyType) -> String {
    match key_type_num {
        HostKeyType::Rsa => "ssh-rsa".into(),
        HostKeyType::Dss => "ssh-dss".into(),
        _ => format!("ssh-unknown-{key_type_num:?}"),
    }
}

/// 将字节哈希转换为 Base64 指纹字符串（遵循 OpenSSH 格式: SHA256:xxxxx）
fn format_fingerprint(hash: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = "SHA256:".to_string();
    for byte in hash {
        write!(s, "{byte:02x}").unwrap();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_fingerprint() {
        let hash = [0xffu8; 32];
        let fp = format_fingerprint(&hash);
        assert!(fp.starts_with("SHA256:"));
        assert_eq!(fp.len(), 7 + 64); // "SHA256:" + 32 bytes * 2 hex chars
    }
}

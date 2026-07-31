//! HostKey 验证模块
//! 从 SSH session 获取远程主机指纹，用于 known_hosts 校验

use base64::{engine::general_purpose, Engine as _};
use core_common::{CoreResult, HostKeyInfo};
use ssh2::HostKeyType;

/// 从已建立 SSH 握手的 session 获取远程主机的密钥信息
/// 返回 Base64 编码的 SHA-256 指纹（与 OpenSSH 格式兼容）
pub fn check_host_key(session: &ssh2::Session, host: &str, port: u16) -> CoreResult<HostKeyInfo> {
    let (_key, raw_key_type) = session.host_key().ok_or_else(|| {
        core_common::CoreError::Internal("no host key available from session".into())
    })?;
    let key_type = key_to_type_name(raw_key_type);
    let hash = session
        .host_key_hash(ssh2::HashType::Sha256)
        .ok_or_else(|| {
            core_common::CoreError::Internal("failed to compute host key hash".into())
        })?;
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
        HostKeyType::Ecdsa256 => "ecdsa-sha2-nistp256".into(),
        HostKeyType::Ecdsa384 => "ecdsa-sha2-nistp384".into(),
        HostKeyType::Ecdsa521 => "ecdsa-sha2-nistp521".into(),
        HostKeyType::Ed25519 => "ssh-ed25519".into(),
        _ => format!("ssh-unknown-{key_type_num:?}"),
    }
}

/// 将字节哈希转换为 Base64 指纹字符串（遵循 OpenSSH 格式: SHA256:xxxxx）
fn format_fingerprint(hash: &[u8]) -> String {
    format!("SHA256:{}", general_purpose::STANDARD_NO_PAD.encode(hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_fingerprint() {
        let hash = [0xffu8; 32];
        let fp = format_fingerprint(&hash);
        assert!(fp.starts_with("SHA256:"));
        // 32 字节 → Base64 无填充: ceil(32 * 4 / 3) = 43 字符
        assert_eq!(fp.len(), 7 + 43);
    }
}

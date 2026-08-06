//! KnownHosts Provider 抽象
//! 定义主机密钥的查询、存储和删除操作

use crate::{CoreResult, HostKeyInfo};

/// 主机密钥的持久化存储接口
/// 用于在 SSH 连接建立前验证远程主机的身份
/// key_type（如 ssh-ed25519）参与查找：同主机多密钥类型并存互不覆盖
pub trait KnownHostsProvider: Send + Sync {
    fn find_host_key(&self, host: &str, port: u16, key_type: &str)
        -> CoreResult<Option<HostKeyInfo>>;
    fn store_host_key(&self, info: &HostKeyInfo) -> CoreResult<()>;
    fn remove_host_key(&self, host: &str, port: u16) -> CoreResult<()>;
}

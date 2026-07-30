//! KnownHosts Provider 抽象
//! 定义主机密钥的查询、存储和删除操作

use crate::{CoreResult, HostKeyInfo};

/// 主机密钥的持久化存储接口
/// 用于在 SSH 连接建立前验证远程主机的身份
pub trait KnownHostsProvider: Send + Sync {
    fn find_host_key(&self, host: &str, port: u16) -> CoreResult<Option<HostKeyInfo>>;
    fn store_host_key(&self, info: &HostKeyInfo) -> CoreResult<()>;
    fn remove_host_key(&self, host: &str, port: u16) -> CoreResult<()>;
}

//! 主机模型模块

use serde::{Deserialize, Serialize};
use crate::HostId;

/// 一个已保存的 SSH 主机连接配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Host {
    pub id: HostId,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub auth_type: String,
    pub group_name: String,
    pub favorite: bool,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Host 的持久化存储接口
pub trait HostRepository: Send + Sync {
    fn list_all(&self) -> crate::CoreResult<Vec<Host>>;
    fn find_by_id(&self, id: &HostId) -> crate::CoreResult<Option<Host>>;
    fn save(&self, host: &Host) -> crate::CoreResult<()>;
    fn delete(&self, id: &HostId) -> crate::CoreResult<()>;
    fn search(&self, query: &str) -> crate::CoreResult<Vec<Host>>;
}

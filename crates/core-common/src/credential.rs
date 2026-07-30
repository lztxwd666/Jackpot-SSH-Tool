//! Credential Provider 抽象
//! 定义认证凭据的加载接口

use crate::{AuthMethod, CoreResult};

/// 提供 SSH 认证所需的凭据
pub trait CredentialProvider: Send + Sync {
    fn load_credential(&self, host: &str, username: &str) -> CoreResult<AuthMethod>;
}

/// 基于 ConnectionConfig 的凭据提供实现
/// 直接从连接配置中获取认证方式，不额外查找
pub struct ConfigCredentialProvider;

impl CredentialProvider for ConfigCredentialProvider {
    fn load_credential(&self, _host: &str, _username: &str) -> CoreResult<AuthMethod> {
        Err(crate::CoreError::NotFound("ConfigCredentialProvider delegates to ConnectionConfig".into()))
    }
}

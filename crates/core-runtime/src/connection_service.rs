//! SSH 连接服务层
//! 定义连接服务的 trait 抽象和基于 SshConnection 的实现
//! 集成 KnownHostsProvider 和 CredentialProvider 完成完整的连接流程

use core_common::{ConnectionConfig, CoreResult, CredentialProvider, KnownHostsProvider};
use core_event::EventDispatcher;
use std::sync::{Arc, Mutex};

use crate::ssh::SshConnection;

/// 连接服务抽象，定义 SSH 连接的生命周期管理
pub trait ConnectionService: Send + Sync {
    fn connect(&self, config: ConnectionConfig) -> CoreResult<()>;
    fn disconnect(&self) -> CoreResult<()>;
    fn is_connected(&self) -> bool;
}

/// 基于 SshConnection 的连接服务实现
/// 可选接入 KnownHostsProvider 和 CredentialProvider
pub struct SshConnectionService {
    connection: Mutex<Option<SshConnection>>,
    known_hosts: Option<Arc<dyn KnownHostsProvider>>,
    credential: Option<Arc<dyn CredentialProvider>>,
    dispatcher: Arc<dyn EventDispatcher>,
}

impl SshConnectionService {
    /// 创建新的连接服务实例
    pub fn new(dispatcher: Arc<dyn EventDispatcher>) -> Self {
        Self {
            connection: Mutex::new(None),
            known_hosts: None,
            credential: None,
            dispatcher,
        }
    }

    /// 设置 KnownHostsProvider
    pub fn with_known_hosts(mut self, provider: Arc<dyn KnownHostsProvider>) -> Self {
        self.known_hosts = Some(provider);
        self
    }

    /// 设置 CredentialProvider
    pub fn with_credential(mut self, provider: Arc<dyn CredentialProvider>) -> Self {
        self.credential = Some(provider);
        self
    }
}

impl ConnectionService for SshConnectionService {
    fn connect(&self, config: ConnectionConfig) -> CoreResult<()> {
        let mut guard = self.connection.lock().map_err(|e| {
            core_common::CoreError::Internal(format!("lock connection mutex failed: {e}"))
        })?;

        if guard.is_some() {
            return Err(core_common::CoreError::Internal(
                "a connection already exists, disconnect first".into(),
            ));
        }

        let mut conn = SshConnection::new(config, Arc::clone(&self.dispatcher));
        conn.connect()?;
        *guard = Some(conn);
        Ok(())
    }

    fn disconnect(&self) -> CoreResult<()> {
        let mut guard = self.connection.lock().map_err(|e| {
            core_common::CoreError::Internal(format!("lock connection mutex failed: {e}"))
        })?;

        if let Some(mut conn) = guard.take() {
            conn.disconnect()?;
        }
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connection
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|conn| conn.is_connected()))
            .unwrap_or(false)
    }
}

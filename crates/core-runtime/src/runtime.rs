//! 核心运行时模块
//! CoreRuntime 是应用的中央调度器，管理整个 crate 栈的生命周期和资源

use core_common::config::Config;
use core_common::credential::CredentialProvider;
use core_common::host::HostRepository;
use core_common::knownhosts::KnownHostsProvider;
use core_common::{CoreResult, SessionId};
use core_event::event::{ApplicationEvent, CoreEvent};
use core_event::{ChannelDispatcher, EventDispatcher};
use core_storage::Database;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::Session;
use crate::credential::KeyringCredentialProvider;

/// 运行时核心结构
/// 持有配置、事件分发器、数据库连接、连接服务和 KnownHosts Provider
pub struct CoreRuntime {
    config: Box<dyn Config>,
    dispatcher: Arc<ChannelDispatcher>,
    db: Arc<RwLock<Option<Arc<Database>>>>,
    running: RwLock<bool>,
    known_hosts: RwLock<Option<Arc<dyn KnownHostsProvider>>>,
    host_repo: RwLock<Option<Arc<dyn HostRepository>>>,
    credential_provider: RwLock<Option<Arc<dyn CredentialProvider>>>,
    sessions: RwLock<HashMap<SessionId, Arc<Session>>>,
}

impl CoreRuntime {
    /// 创建运行时实例，初始化分发器和各资源槽位
    pub fn new(config: Box<dyn Config>) -> Self {
        // 容量 4096：高吞吐终端输出时避免 DataReceived 事件被丢弃
        let dispatcher = Arc::new(ChannelDispatcher::new(4096));
        Self {
            config,
            dispatcher,
            db: Arc::new(RwLock::new(None)),
            running: RwLock::new(false),
            known_hosts: RwLock::new(None),
            host_repo: RwLock::new(None),
            credential_provider: RwLock::new(None),
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// 获取事件分发器引用，供外部订阅事件
    pub fn dispatcher(&self) -> Arc<ChannelDispatcher> {
        self.dispatcher.clone()
    }

    /// 获取当前配置的只读引用
    pub fn config(&self) -> &dyn Config {
        self.config.as_ref()
    }

    /// 运行时是否已启动（供状态查询）
    /// async：running 是 tokio RwLock，禁止在 async 上下文调用 blocking_read（会 panic）
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// 启动运行时：打开数据库、执行迁移、初始化连接服务和 KnownHosts Provider
    /// 防止重复启动，二次调用返回 Internal error
    /// 初始化中途失败会回滚 running 标志（否则滞留半初始化态且不可重试）
    pub async fn start(&self) -> CoreResult<()> {
        {
            let mut running = self.running.write().await;
            if *running {
                return Err(core_common::CoreError::Internal(
                    "runtime already started".into(),
                ));
            }
            *running = true;
        }

        let result = self.start_inner().await;
        if result.is_err() {
            let mut running = self.running.write().await;
            *running = false;
        }
        result
    }

    /// start 的实际初始化逻辑（running 标志已置位；失败由 start 回滚）
    async fn start_inner(&self) -> CoreResult<()> {
        self.dispatcher
            .dispatch(CoreEvent::Application(ApplicationEvent::Started));

        let db = Database::open(self.config.app_data_dir())?;
        db.migrate()?;

        let db_arc = Arc::new(db);
        let known_hosts: Arc<dyn KnownHostsProvider> =
            Arc::new(core_storage::SqliteKnownHosts::new(db_arc.clone()));

        let host_repo: Arc<dyn HostRepository> =
            Arc::new(core_storage::SqliteHostRepository::new(db_arc.clone()));

        let credential_provider: Arc<dyn CredentialProvider> =
            Arc::new(KeyringCredentialProvider::new());

        {
            let mut db_lock = self.db.write().await;
            *db_lock = Some(db_arc.clone());
        }
        {
            let mut kh_lock = self.known_hosts.write().await;
            *kh_lock = Some(known_hosts);
        }
        {
            let mut hr_lock = self.host_repo.write().await;
            *hr_lock = Some(host_repo);
        }
        {
            let mut cp_lock = self.credential_provider.write().await;
            *cp_lock = Some(credential_provider);
        }

        self.dispatcher
            .dispatch(CoreEvent::Application(ApplicationEvent::Ready));

        tracing::info!("core runtime started");
        Ok(())
    }

    /// 获取 KnownHosts Provider 的引用（仅在 start() 后有效）
    pub async fn known_hosts(&self) -> Option<Arc<dyn KnownHostsProvider>> {
        self.known_hosts.read().await.clone()
    }

    /// 获取 HostRepository 的引用（仅在 start() 后有效）
    pub async fn host_repo(&self) -> Option<Arc<dyn HostRepository>> {
        self.host_repo.read().await.clone()
    }

    /// 获取 CredentialProvider 的引用（仅在 start() 后有效）
    pub async fn credential_provider(&self) -> Option<Arc<dyn CredentialProvider>> {
        self.credential_provider.read().await.clone()
    }

    /// 创建一个新的 Session 并注册到会话管理器中
    pub async fn create_session(&self) -> CoreResult<Arc<Session>> {
        let session = Session::new(self.dispatcher.clone());
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(session.id, session.clone());
        }
        tracing::info!(session_id = %session.id, "session created and registered");
        Ok(session)
    }

    /// 根据 ID 获取已注册的 Session
    pub async fn get_session(&self, id: &SessionId) -> Option<Arc<Session>> {
        self.sessions.read().await.get(id).cloned()
    }

    /// 从会话管理器中移除指定 Session
    pub async fn remove_session(&self, id: &SessionId) {
        let mut sessions = self.sessions.write().await;
        sessions.remove(id);
        tracing::info!(session_id = %id, "session removed");
    }

    /// 优雅关闭运行时：断开连接、销毁 Provider、关闭数据库
    pub async fn shutdown(&self) {
        self.dispatcher
            .dispatch(CoreEvent::Application(ApplicationEvent::ShutdownRequested));

        // 释放各 Provider
        {
            let mut hr_lock = self.host_repo.write().await;
            *hr_lock = None;
        }
        {
            let mut kh_lock = self.known_hosts.write().await;
            *kh_lock = None;
        }
        {
            let mut cp_lock = self.credential_provider.write().await;
            *cp_lock = None;
        }

        // 再关闭数据库
        {
            let mut db_lock = self.db.write().await;
            if let Some(db_arc) = db_lock.take() {
                if let Ok(db) = Arc::try_unwrap(db_arc) {
                    let _ = db.close();
                } else {
                    tracing::warn!("Database Arc still has references during shutdown");
                }
            }
        }

        {
            let mut running = self.running.write().await;
            *running = false;
        }

        self.dispatcher
            .dispatch(CoreEvent::Application(ApplicationEvent::ShutdownCompleted));

        tracing::info!("core runtime shut down");
    }
}

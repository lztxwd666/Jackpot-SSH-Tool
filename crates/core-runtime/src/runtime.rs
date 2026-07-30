//! 核心运行时模块
//! CoreRuntime 是应用的中央调度器，管理整个 crate 栈的生命周期和资源

use core_common::config::Config;
use core_common::knownhosts::KnownHostsProvider;
use core_common::CoreResult;
use core_event::event::{ApplicationEvent, CoreEvent};
use core_event::{ChannelDispatcher, EventDispatcher};
use core_storage::Database;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::connection_service::{ConnectionService, SshConnectionService};

/// 运行时核心结构
/// 持有配置、事件分发器、数据库连接、连接服务和 KnownHosts Provider
pub struct CoreRuntime {
    config: Box<dyn Config>,
    dispatcher: Arc<ChannelDispatcher>,
    db: Arc<RwLock<Option<Database>>>,
    running: RwLock<bool>,
    connection_service: RwLock<Option<Arc<dyn ConnectionService>>>,
    known_hosts: RwLock<Option<Arc<dyn KnownHostsProvider>>>,
}

impl CoreRuntime {
    /// 创建运行时实例，初始化分发器和各资源槽位
    pub fn new(config: Box<dyn Config>) -> Self {
        let dispatcher = Arc::new(ChannelDispatcher::new(256));
        Self {
            config,
            dispatcher,
            db: Arc::new(RwLock::new(None)),
            running: RwLock::new(false),
            connection_service: RwLock::new(None),
            known_hosts: RwLock::new(None),
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

    /// 启动运行时：打开数据库、执行迁移、初始化连接服务和 KnownHosts Provider
    /// 反防护重复启动，二次调用返回 Internal error
    pub async fn start(&self) -> CoreResult<()> {
        {
            let running = self.running.read().await;
            if *running {
                return Err(core_common::CoreError::Internal(
                    "runtime already started".into(),
                ));
            }
        }
        {
            let mut running = self.running.write().await;
            *running = true;
        }

        self.dispatcher
            .dispatch(CoreEvent::Application(ApplicationEvent::Started));

        let db = Database::open(self.config.app_data_dir())?;
        db.migrate()?;

        let db_arc = Arc::new(db);
        let known_hosts: Arc<dyn KnownHostsProvider> =
            Arc::new(core_storage::SqliteKnownHosts::new(db_arc.clone()));

        let conn_service: Arc<dyn ConnectionService> = Arc::new(
            SshConnectionService::new(self.dispatcher.clone())
                .with_known_hosts(known_hosts.clone()),
        );

        {
            let mut db_lock = self.db.write().await;
            *db_lock = Some(Arc::try_unwrap(db_arc).unwrap_or_else(|_arc| {
                panic!("Database Arc still has references")
            }));
        }
        {
            let mut kh_lock = self.known_hosts.write().await;
            *kh_lock = Some(known_hosts);
        }
        {
            let mut cs_lock = self.connection_service.write().await;
            *cs_lock = Some(conn_service);
        }

        self.dispatcher
            .dispatch(CoreEvent::Application(ApplicationEvent::Ready));

        tracing::info!("core runtime started");
        Ok(())
    }

    /// 获取连接服务的引用（仅在 start() 后有效）
    pub async fn connection_service(&self) -> Option<Arc<dyn ConnectionService>> {
        self.connection_service.read().await.clone()
    }

    /// 获取 KnownHosts Provider 的引用（仅在 start() 后有效）
    pub async fn known_hosts(&self) -> Option<Arc<dyn KnownHostsProvider>> {
        self.known_hosts.read().await.clone()
    }

    /// 优雅关闭运行时：断开连接、销毁 Provider、关闭数据库
    pub async fn shutdown(&self) {
        self.dispatcher
            .dispatch(CoreEvent::Application(ApplicationEvent::ShutdownRequested));

        // 先断开连接
        {
            let cs_lock = self.connection_service.read().await;
            if let Some(ref cs) = *cs_lock {
                let _ = cs.disconnect();
            }
        }
        {
            let mut cs_lock = self.connection_service.write().await;
            *cs_lock = None;
        }
        {
            let mut kh_lock = self.known_hosts.write().await;
            *kh_lock = None;
        }

        // 再关闭数据库
        {
            let mut db_lock = self.db.write().await;
            if let Some(db) = db_lock.take() {
                let _ = db.close();
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

use core_common::config::Config;
use core_common::CoreResult;
use core_event::event::{ApplicationEvent, CoreEvent};
use core_event::{ChannelDispatcher, EventDispatcher};
use core_storage::Database;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct CoreRuntime {
    config: Box<dyn Config>,
    dispatcher: Arc<ChannelDispatcher>,
    db: Arc<RwLock<Option<Database>>>,
    running: RwLock<bool>,
}

impl CoreRuntime {
    pub fn new(config: Box<dyn Config>) -> Self {
        let dispatcher = Arc::new(ChannelDispatcher::new(256));
        Self {
            config,
            dispatcher,
            db: Arc::new(RwLock::new(None)),
            running: RwLock::new(false),
        }
    }

    pub fn dispatcher(&self) -> Arc<ChannelDispatcher> {
        self.dispatcher.clone()
    }

    pub fn config(&self) -> &dyn Config {
        self.config.as_ref()
    }

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
        {
            let mut db_lock = self.db.write().await;
            *db_lock = Some(db);
        }

        self.dispatcher
            .dispatch(CoreEvent::Application(ApplicationEvent::Ready));

        tracing::info!("core runtime started");
        Ok(())
    }

    pub async fn shutdown(&self) {
        self.dispatcher
            .dispatch(CoreEvent::Application(ApplicationEvent::ShutdownRequested));

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

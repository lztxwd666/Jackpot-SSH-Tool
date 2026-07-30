use crate::event::CoreEvent;
use std::sync::Arc;

pub trait EventDispatcher: Send + Sync {
    fn dispatch(&self, event: CoreEvent);
}

pub type SharedEventDispatcher = Arc<dyn EventDispatcher>;

pub struct LoggingDispatcher;

impl EventDispatcher for LoggingDispatcher {
    fn dispatch(&self, event: CoreEvent) {
        tracing::info!(?event, "event dispatched");
    }
}

pub struct ChannelDispatcher {
    sender: tokio::sync::broadcast::Sender<CoreEvent>,
}

impl ChannelDispatcher {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(capacity);
        Self { sender }
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<CoreEvent> {
        self.sender.subscribe()
    }
}

impl EventDispatcher for ChannelDispatcher {
    fn dispatch(&self, event: CoreEvent) {
        let _ = self.sender.send(event);
    }
}

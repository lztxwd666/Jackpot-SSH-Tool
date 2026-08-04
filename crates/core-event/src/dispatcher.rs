//! 事件分发器模块
//! 提供 broadcast channel 和 logging 两种实现，统一通过 trait object 使用

use crate::event::CoreEvent;
use std::sync::Arc;

/// 事件分发 trait，所有分发器必须实现此接口
pub trait EventDispatcher: Send + Sync {
    fn dispatch(&self, event: CoreEvent);
}

/// 线程安全的共享分发器引用
pub type SharedEventDispatcher = Arc<dyn EventDispatcher>;

/// 仅打印日志的分发器实现，用于测试或调试场景
pub struct LoggingDispatcher;

impl EventDispatcher for LoggingDispatcher {
    fn dispatch(&self, event: CoreEvent) {
        tracing::info!(?event, "event dispatched");
    }
}

/// 基于 tokio broadcast channel 的多播分发器
/// 每个 subscribe() 调用产生一个独立的 Receiver，支持 multiple-producer 模式
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
        // send 失败仅表示当前无接收者（正常情况，无订阅者时不产生事件处理）
        // 慢接收者导致的事件丢弃在 recv 侧检测（RecvError::Lagged），由订阅方负责告警
        let _ = self.sender.send(event);
    }
}

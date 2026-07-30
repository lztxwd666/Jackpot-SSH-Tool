//! core-event: 事件定义与分发模块
//! CoreEvent 是各层间解耦通信的唯一媒介，通过广播通道推送给所有订阅者

pub mod dispatcher;
pub mod event;

pub use dispatcher::{ChannelDispatcher, EventDispatcher, LoggingDispatcher, SharedEventDispatcher};
pub use event::{ApplicationEvent, CoreEvent, SystemEvent};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum CoreEvent {
    Application(ApplicationEvent),
    System(SystemEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ApplicationEvent {
    Started,
    Ready,
    ShutdownRequested,
    ShutdownCompleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum SystemEvent {
    DatabaseOpened,
    DatabaseError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::EventDispatcher;

    #[test]
    fn test_event_serialization() {
        let event = CoreEvent::Application(ApplicationEvent::Started);
        let json = serde_json::to_string(&event).unwrap();
        let parsed: CoreEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            CoreEvent::Application(ApplicationEvent::Started)
        ));
    }

    #[test]
    fn test_dispatcher_send_receive() {
        let dispatcher = crate::dispatcher::ChannelDispatcher::new(16);
        let mut rx = dispatcher.subscribe();
        dispatcher.dispatch(CoreEvent::Application(ApplicationEvent::Ready));
        let received = rx.try_recv().unwrap();
        assert!(matches!(
            received,
            CoreEvent::Application(ApplicationEvent::Ready)
        ));
    }
}

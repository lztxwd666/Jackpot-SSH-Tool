//! 事件枚举定义模块
//! 所有事件均不可变，描述已经发生的事情，携带最小必要 payload

use serde::{Deserialize, Serialize};

/// 顶层事件枚举，按来源划分为 Application 和 System 两类
/// 序列化使用 serde 的 tag-based JSON 格式，方便前端 pattern match
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum CoreEvent {
    Application(ApplicationEvent),
    System(SystemEvent),
    Connection(ConnectionEvent),
    HostKey(HostKeyEvent),
    Credential(CredentialEvent),
}

/// 应用生命周期事件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ApplicationEvent {
    Started,
    Ready,
    ShutdownRequested,
    ShutdownCompleted,
}

/// 系统基础设施事件（数据库、文件系统等）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum SystemEvent {
    DatabaseOpened,
    DatabaseError(String),
}

/// SSH 连接生命周期事件
/// 按顺序: Connecting -> TcpConnected -> HandshakeStarted -> HostKeyVerifying -> Authenticated -> Ready
/// 异常路径: Connecting -> Failed 或任意阶段 -> Disconnected
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum ConnectionEvent {
    /// TCP 连接已发起，携带目标主机信息
    Connecting { host: String, port: u16 },
    /// TCP socket 已建立
    TcpConnected,
    /// SSH 协议握手开始
    HandshakeStarted,
    /// 正在进行 HostKey 验证，等待用户决策或自动校验
    HostKeyVerifying,
    /// 认证成功，连接可用
    Authenticated,
    /// 连接完全就绪，可以打开 Channel
    Ready,
    /// 连接已正常断开
    Disconnected,
    /// 连接失败，携带原因描述
    Failed { reason: String },
}

/// HostKey 验证事件
/// 用于通知前端出现未知主机或主机密钥变更的情况
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum HostKeyEvent {
    /// 发现未知主机，需要用户确认
    Unknown { host: String, fingerprint: String },
    /// 主机密钥已变更，可能存在中间人攻击
    Changed { host: String, old_fingerprint: String, new_fingerprint: String },
    /// 主机密钥已被接受
    Accepted,
    /// 用户拒绝该主机密钥
    Rejected,
}

/// 凭据操作事件
/// 凭据值绝不出现在事件中，仅携带操作结果状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum CredentialEvent {
    /// 凭据加载成功
    Loaded,
    /// 未找到指定凭据
    NotFound(String),
    /// 凭据访问被拒绝（权限问题）
    AccessDenied(String),
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

    #[test]
    fn test_connection_event_roundtrip() {
        let event = CoreEvent::Connection(ConnectionEvent::Connecting {
            host: "example.com".into(),
            port: 22,
        });
        let json = serde_json::to_string(&event).unwrap();
        let parsed: CoreEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            CoreEvent::Connection(ConnectionEvent::Connecting { ref host, port: 22 })
            if host == "example.com"
        ));
    }
}

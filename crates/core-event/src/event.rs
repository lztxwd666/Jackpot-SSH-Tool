//! 事件枚举定义模块
//! 所有事件均不可变，描述已经发生的事情，携带最小必要 payload

use core_common::{ChannelId, ChannelType, HostId, SessionId};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

/// 顶层事件枚举，按来源划分
/// 序列化使用 serde 的 tag-based JSON 格式，方便前端 pattern match
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum CoreEvent {
    Application(ApplicationEvent),
    Connection(ConnectionEvent),
    HostKey(HostKeyEvent),
    Session(SessionEvent),
    Channel(ChannelEvent),
    Transfer(TransferEvent),
    Host(HostEvent),
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
/// key_type（ssh-rsa/ssh-ed25519 等）必须透传：V4 起 known_hosts 按 (host, port, key_type)
/// 匹配，approve 时需按真实类型存储，否则确认过的密钥永远无法命中
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum HostKeyEvent {
    /// 发现未知主机，需要用户确认
    Unknown {
        host: String,
        key_type: String,
        fingerprint: String,
    },
    /// 主机密钥已变更，可能存在中间人攻击
    Changed {
        host: String,
        key_type: String,
        old_fingerprint: String,
        new_fingerprint: String,
    },
    /// 主机密钥已被接受
    Accepted,
    /// 用户拒绝该主机密钥
    Rejected,
}

/// Session 生命周期事件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum SessionEvent {
    /// Session 已创建
    Created { session_id: SessionId },
    /// 正在连接远端主机
    Connecting {
        session_id: SessionId,
        host: String,
        port: u16,
    },
    /// 连接成功，可以打开 Channel
    Connected { session_id: SessionId },
    /// 连接已断开（可重连，重连由用户手动发起）；reason 为断开原因（异常断开时供 UI 展示）
    Disconnected {
        session_id: SessionId,
        reason: String,
    },
    /// Session 永久关闭，不可再连接
    Closed { session_id: SessionId },
}

/// SSH 通道生命周期事件
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum ChannelEvent {
    /// 通道正在打开
    Opening {
        session_id: SessionId,
        channel_id: ChannelId,
        channel_type: ChannelType,
    },
    /// 通道已打开，可以读写
    Opened {
        session_id: SessionId,
        channel_id: ChannelId,
    },
    /// 收到远端数据
    /// data 使用 base64 编码传输：相比 JSON 数字数组（每字节 ~5 字符）压缩到 ~1.33x，
    /// 显著降低高吞吐终端输出时的 IPC 带宽与序列化开销
    DataReceived {
        session_id: SessionId,
        channel_id: ChannelId,
        #[serde_as(as = "serde_with::base64::Base64")]
        data: Vec<u8>,
    },
    /// 通道已关闭
    Closed {
        session_id: SessionId,
        channel_id: ChannelId,
    },
}

/// 主机管理事件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum HostEvent {
    Created { host_id: HostId, name: String },
    Updated { host_id: HostId, name: String },
    Deleted { host_id: HostId, name: String },
}

/// 文件传输方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    Download,
    Upload,
}

/// 文件传输状态事件（Transfer 领域）
/// Stage 6 落地传输窗口标记：Locked/Unlocked 描述 sftp 通道的传输占用期
/// （worker 单线程内同一时刻仅允许一个传输）；细粒度进度暂由桌面层
/// transfer-progress 事件承担，未来迁入本事件域
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum TransferEvent {
    /// 传输开始：sftp 通道锁定
    Locked {
        session_id: SessionId,
        channel_id: ChannelId,
        direction: TransferDirection,
    },
    /// 传输结束（成功/失败/取消）：sftp 通道解锁
    Unlocked {
        session_id: SessionId,
        channel_id: ChannelId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_transfer_event_roundtrip() {
        let event = CoreEvent::Transfer(TransferEvent::Locked {
            session_id: SessionId::new(),
            channel_id: ChannelId::new(),
            direction: TransferDirection::Download,
        });
        let json = serde_json::to_string(&event).unwrap();
        let parsed: CoreEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            CoreEvent::Transfer(TransferEvent::Locked {
                direction: TransferDirection::Download,
                ..
            })
        ));
        let unlocked = CoreEvent::Transfer(TransferEvent::Unlocked {
            session_id: SessionId::new(),
            channel_id: ChannelId::new(),
        });
        let json = serde_json::to_string(&unlocked).unwrap();
        let parsed: CoreEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            CoreEvent::Transfer(TransferEvent::Unlocked { .. })
        ));
    }

    #[test]
    fn test_disconnected_with_reason_roundtrip() {
        let event = CoreEvent::Session(SessionEvent::Disconnected {
            session_id: SessionId::new(),
            reason: "channel write failed: test".into(),
        });
        let json = serde_json::to_string(&event).unwrap();
        let parsed: CoreEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            CoreEvent::Session(SessionEvent::Disconnected { reason, .. })
            if reason == "channel write failed: test"
        ));
    }

}

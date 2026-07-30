//! 强类型 ID 定义模块
//! 通过 macro_rules 自动生成带 UUID 的 newtype，避免裸 Uuid 导致的混淆

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 声明式生成唯一标识符类型
/// 每个类型包装一个 UUID v4，提供 Copy + Display + Serde 支持
macro_rules! define_id {
    ($name:ident) => {
        #[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

// 交互会话的 ID（对应一个 SSH session）
define_id!(SessionId);
// 远程主机的 ID
define_id!(HostId);
// SSH 连接的 ID（一个连接可复用于多个 session）
define_id!(ConnectionId);
// 数据通道的 ID（TCP port forwarding / PTY）
define_id!(ChannelId);
// 文件传输任务的 ID（SFTP 上传/下载）
define_id!(TransferId);

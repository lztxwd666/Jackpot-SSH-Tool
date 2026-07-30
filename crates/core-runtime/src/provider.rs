//! 系统服务提供者层 trait 定义
//! 封装平台相关能力（文件系统、加密、网络），使 core 层不依赖 Tauri/OS

/// 系统服务提供者标记 trait，当前为空骨架，后续会扩展平台抽象方法
pub trait Provider: Send + Sync {}

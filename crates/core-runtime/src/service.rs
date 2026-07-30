//! 业务服务层 trait 定义
//! 各 SSH 功能模块（连接管理、文件传输等）通过实现此 trait 注册到运行时

/// 业务服务标记 trait，当前为空骨架，后续会扩展生命周期方法
pub trait Service: Send + Sync {}

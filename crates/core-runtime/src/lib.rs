//! core-runtime: 运行时核心模块
//! 负责应用生命周期管理、组件注册和事件驱动架构的编排

pub mod channel;
pub mod keepalive;
pub mod reconnect;
pub mod runtime;
pub mod session;
pub mod ssh;
pub mod worker;

pub use channel::Channel;
pub use keepalive::spawn_keepalive;
pub use reconnect::spawn_reconnect;
pub use runtime::CoreRuntime;
pub use session::Session;

pub use core_common::HostRepository;

/// RwLock 中毒恢复读取：读取中毒锁的值是安全的（旧值可能已损坏，但读取不产生新破坏）
/// 避免单个 panic 导致所有后续访问连锁 panic
pub(crate) fn rw_read<T>(lock: &std::sync::RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

/// RwLock 中毒恢复写：写入覆盖旧值，中毒后仍安全
pub(crate) fn rw_write<T>(lock: &std::sync::RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

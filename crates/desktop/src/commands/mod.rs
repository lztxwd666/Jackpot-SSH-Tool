//! Tauri IPC 命令处理模块
//! 定义前端可调用的命令函数和共享状态结构
//! 按职责拆分为子模块：local（本地文件）、host（主机）、session（会话/终端）、sftp（文件传输）

pub mod credential;
pub mod host;
pub mod local;
pub mod session;
pub mod sftp;

// 统一 re-export：lib.rs 与前端通过 commands::xxx 引用所有命令
pub use credential::*;
pub use host::*;
pub use local::*;
pub use session::*;
pub use sftp::*;

use core_common::{ChannelId, SessionId};
use core_runtime::Channel;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// Tauri 全局托管状态，前端通过 State<T> 注入访问
pub struct AppState {
    pub runtime: RwLock<Option<core_runtime::CoreRuntime>>,
    pub channels: RwLock<HashMap<ChannelId, Arc<Channel>>>,
    /// SessionId → SFTP Channel 的映射
    pub sftp_channels: RwLock<HashMap<SessionId, Arc<Channel>>>,
}

/// 按 session_id 获取 SFTP 通道（统一错误处理与锁生命周期）
async fn get_sftp_channel(
    state: &Arc<AppState>,
    session_id: &str,
) -> Result<Arc<Channel>, String> {
    let sid = SessionId::parse(session_id)?;
    let sftp_channels = state.sftp_channels.read().await;
    sftp_channels
        .get(&sid)
        .ok_or_else(|| "SFTP channel not found".to_string())
        .cloned()
}


/// 返回运行时当前状态（running / stopped）
#[tauri::command]
pub async fn get_app_status(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let guard = state.runtime.read().await;
    match guard.as_ref() {
        Some(rt) => Ok(if rt.is_running().await { "running" } else { "stopped" }.to_string()),
        None => Ok("stopped".to_string()),
    }
}


/// 健康检查命令，用于验证前端与后端的 IPC 通道正常
#[tauri::command]
pub async fn ping() -> Result<String, String> {
    Ok("pong".to_string())
}


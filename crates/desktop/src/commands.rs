//! Tauri IPC 命令处理模块
//! 定义前端可调用的命令函数和共享状态结构

use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// Tauri 全局托管状态，前端通过 State<T> 注入访问
pub struct AppState {
    pub runtime: RwLock<Option<core_runtime::CoreRuntime>>,
}

/// 返回运行时当前状态（running / stopped）
#[tauri::command]
pub async fn get_app_status(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let guard = state.runtime.read().await;
    if guard.is_some() {
        Ok("running".to_string())
    } else {
        Ok("stopped".to_string())
    }
}

/// 健康检查命令，用于验证前端与后端的 IPC 通道正常
#[tauri::command]
pub async fn ping() -> Result<String, String> {
    Ok("pong".to_string())
}

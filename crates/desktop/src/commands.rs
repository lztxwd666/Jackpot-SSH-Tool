//! Tauri IPC 命令处理模块
//! 定义前端可调用的命令函数和共享状态结构

use core_common::Host;
use std::str::FromStr;
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

/// 获取所有已保存的主机列表
#[tauri::command]
pub async fn list_hosts(state: State<'_, Arc<AppState>>) -> Result<Vec<Host>, String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let repo = rt.host_repo().await.ok_or("host repository not available")?;
    repo.list_all().map_err(|e| e.to_string())
}

/// 保存或更新一台主机
#[tauri::command]
pub async fn save_host(state: State<'_, Arc<AppState>>, host: Host) -> Result<(), String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let repo = rt.host_repo().await.ok_or("host repository not available")?;
    repo.save(&host).map_err(|e| e.to_string())
}

/// 删除一台主机
#[tauri::command]
pub async fn delete_host(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let repo = rt.host_repo().await.ok_or("host repository not available")?;
    let uuid = uuid::Uuid::from_str(&id).map_err(|e| e.to_string())?;
    let host_id = serde_json::from_str(&serde_json::to_string(&uuid).unwrap()).map_err(|e| e.to_string())?;
    repo.delete(&host_id).map_err(|e| e.to_string())
}

/// 按名称或地址搜索主机
#[tauri::command]
pub async fn search_hosts(state: State<'_, Arc<AppState>>, query: String) -> Result<Vec<Host>, String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let repo = rt.host_repo().await.ok_or("host repository not available")?;
    repo.search(&query).map_err(|e| e.to_string())
}

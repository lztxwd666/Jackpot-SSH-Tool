//! 主机管理命令子模块

use super::AppState;
use core_common::{Host, HostId};
use std::sync::Arc;
use tauri::State;

/// 获取所有已保存的主机列表
#[tauri::command]
pub async fn list_hosts(state: State<'_, Arc<AppState>>) -> Result<Vec<Host>, String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let repo = rt
        .host_repo()
        .await
        .ok_or("host repository not available")?;
    repo.list_all().map_err(|e| e.to_string())
}


/// 保存或更新一台主机
#[tauri::command]
pub async fn save_host(state: State<'_, Arc<AppState>>, host: Host) -> Result<(), String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let repo = rt
        .host_repo()
        .await
        .ok_or("host repository not available")?;
    repo.save(&host).map_err(|e| e.to_string())
}


/// 删除一台主机
#[tauri::command]
pub async fn delete_host(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let repo = rt
        .host_repo()
        .await
        .ok_or("host repository not available")?;
    let host_id = HostId::parse(&id)?;
    repo.delete(&host_id).map_err(|e| e.to_string())
}


/// 用户确认（或更新）主机密钥后存储，供后续连接验证
/// 存储使用 INSERT OR REPLACE，天然支持"信任新密钥"（覆盖旧值）场景
#[tauri::command]
pub async fn approve_host_key(
    state: State<'_, Arc<AppState>>,
    host: String,
    port: u16,
    fingerprint: String,
) -> Result<(), String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let kh = rt.known_hosts().await.ok_or("known hosts provider not available")?;
    // key_type 不影响指纹比对逻辑，统一记录为 user-approved
    let info = core_common::HostKeyInfo::new(host, port, "user-approved".to_string(), fingerprint);
    kh.store_host_key(&info).map_err(|e| e.to_string())
}


/// 按名称或地址搜索主机
#[tauri::command]
pub async fn search_hosts(
    state: State<'_, Arc<AppState>>,
    query: String,
) -> Result<Vec<Host>, String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let repo = rt
        .host_repo()
        .await
        .ok_or("host repository not available")?;
    repo.search(&query).map_err(|e| e.to_string())
}


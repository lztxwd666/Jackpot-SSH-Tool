//! Tauri IPC 命令处理模块
//! 定义前端可调用的命令函数和共享状态结构

use core_common::{ChannelId, ConnectionConfig, Host, SessionId};
use core_runtime::Channel;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// Tauri 全局托管状态，前端通过 State<T> 注入访问
pub struct AppState {
    pub runtime: RwLock<Option<core_runtime::CoreRuntime>>,
    pub channels: RwLock<HashMap<ChannelId, Arc<Channel>>>,
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
    let uuid = uuid::Uuid::from_str(&id).map_err(|e| e.to_string())?;
    let host_id =
        serde_json::from_str(&serde_json::to_string(&uuid).unwrap()).map_err(|e| e.to_string())?;
    repo.delete(&host_id).map_err(|e| e.to_string())
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

/// 创建一个新的 SSH Session，返回 session_id
#[tauri::command]
pub async fn create_session(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let session = rt.create_session().await.map_err(|e| e.to_string())?;
    Ok(session.id.to_string())
}

/// 使用指定配置连接 Session
#[tauri::command]
pub async fn connect_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    host: String,
    port: u16,
    username: String,
    password: String,
) -> Result<(), String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;

    let parsed_id = uuid::Uuid::from_str(&session_id).map_err(|e| e.to_string())?;
    let sid: SessionId = serde_json::from_str(&serde_json::to_string(&parsed_id).unwrap())
        .map_err(|e| e.to_string())?;

    let session = rt.get_session(&sid).await.ok_or("session not found")?;

    let auth = core_common::AuthMethod::Password(password);
    let config = ConnectionConfig::new(host, username, auth).with_port(port);

    let known_hosts = rt.known_hosts().await;

    let session_clone = session.clone();
    tokio::task::spawn_blocking(move || session_clone.connect(config, known_hosts))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// 为指定 Session 打开一个 Shell Channel，返回 channel_id
#[tauri::command]
pub async fn open_shell(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<String, String> {
    let parsed_id = uuid::Uuid::from_str(&session_id).map_err(|e| e.to_string())?;
    let sid: SessionId = serde_json::from_str(&serde_json::to_string(&parsed_id).unwrap())
        .map_err(|e| e.to_string())?;

    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let session = rt.get_session(&sid).await.ok_or("session not found")?;

    let session_clone = session.clone();
    let channel = tokio::task::spawn_blocking(move || session_clone.open_shell())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    let channel_id = channel.id;

    {
        let mut channels = state.channels.write().await;
        channels.insert(channel_id, channel.clone());
    }

    // 不在此启动读循环——等前端 Terminal 组件挂载后通过 start_terminal 命令触发
    // channel.start_read_loop(); // 延迟到 start_terminal 命令

    Ok(channel_id.to_string())
}

/// 前端 Terminal 组件挂载完成后调用，启动数据读取
/// 这确保了事件监听器就绪后才开始读 SSH 数据
#[tauri::command]
pub async fn start_terminal(
    state: State<'_, Arc<AppState>>,
    channel_id: String,
) -> Result<(), String> {
    let parsed_id = uuid::Uuid::from_str(&channel_id).map_err(|e| e.to_string())?;
    let cid: ChannelId = serde_json::from_str(&serde_json::to_string(&parsed_id).unwrap())
        .map_err(|e| e.to_string())?;

    let channels = state.channels.read().await;
    let channel = channels.get(&cid).ok_or("channel not found")?.clone();
    drop(channels);

    channel.start_read_loop();
    tracing::info!(%cid, "terminal read loop started");
    Ok(())
}

/// 向指定 Channel 发送终端输入
#[tauri::command]
pub async fn terminal_send_input(
    state: State<'_, Arc<AppState>>,
    channel_id: String,
    data: String,
) -> Result<(), String> {
    let parsed_id = uuid::Uuid::from_str(&channel_id).map_err(|e| e.to_string())?;
    let cid: ChannelId = serde_json::from_str(&serde_json::to_string(&parsed_id).unwrap())
        .map_err(|e| e.to_string())?;

    let channels = state.channels.read().await;
    let channel = channels.get(&cid).ok_or("channel not found")?;
    channel
        .write(data.into_bytes())
        .await
        .map_err(|e| {
            tracing::error!(%cid, %e, "terminal_send_input failed");
            e.to_string()
        })?;
    Ok(())
}

/// 调整终端 PTY 尺寸
#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, Arc<AppState>>,
    channel_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let parsed_id = uuid::Uuid::from_str(&channel_id).map_err(|e| e.to_string())?;
    let cid: ChannelId = serde_json::from_str(&serde_json::to_string(&parsed_id).unwrap())
        .map_err(|e| e.to_string())?;

    let channels = state.channels.read().await;
    let channel = channels.get(&cid).ok_or("channel not found")?;
    channel.resize_pty(cols, rows).map_err(|e| e.to_string())?;
    Ok(())
}

/// 关闭终端连接，清理 Session 和 Channel 资源
#[tauri::command]
pub async fn terminal_close(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    let parsed_id = uuid::Uuid::from_str(&session_id).map_err(|e| e.to_string())?;
    let sid: SessionId = serde_json::from_str(&serde_json::to_string(&parsed_id).unwrap())
        .map_err(|e| e.to_string())?;

    let guard = state.runtime.read().await;
    let session = if let Some(ref rt) = *guard {
        rt.get_session(&sid).await
    } else {
        None
    };
    drop(guard);

    if let Some(session) = session {
        tokio::task::spawn_blocking(move || {
            let _ = session.close();
        })
        .await
        .map_err(|e| e.to_string())?;
    }

    let mut channels = state.channels.write().await;
    channels.retain(|_, ch| ch.session_id != sid);

    let guard = state.runtime.read().await;
    if let Some(ref rt) = *guard {
        rt.remove_session(&sid).await;
    }

    Ok(())
}

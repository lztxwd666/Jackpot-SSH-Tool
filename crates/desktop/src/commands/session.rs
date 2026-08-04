//! 会话与终端命令子模块

use super::AppState;
use core_common::{ChannelId, ConnectionConfig, SessionId};
use std::sync::Arc;
use tauri::{Emitter, State};

/// 终端读循环异常退出事件 payload
#[derive(Debug, Clone, serde::Serialize)]
pub struct TerminalError {
    pub channel_id: String,
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
/// 支持密码、私钥、Agent 三种认证方式
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn connect_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    host: String,
    port: u16,
    username: String,
    auth_type: String,
    password: Option<String>,
    private_key_path: Option<String>,
    private_key_passphrase: Option<String>,
) -> Result<(), String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;

    let sid = SessionId::parse(&session_id)?;
    let session = rt.get_session(&sid).await.ok_or("session not found")?;

    let auth = match auth_type.as_str() {
        "password" => core_common::AuthMethod::Password(
            password.unwrap_or_default(),
        ),
        "private_key" => {
            let path = private_key_path
                .ok_or("private key path is required")?;
            core_common::AuthMethod::PrivateKey {
                path: std::path::PathBuf::from(path),
                passphrase: private_key_passphrase,
            }
        }
        "agent" => core_common::AuthMethod::Agent,
        _ => return Err(format!("unsupported auth type: {auth_type}")),
    };
    let config = ConnectionConfig::new(host, username, auth).with_port(port);

    // known_hosts 为 None 表示运行时尚未完成初始化：拒绝连接而非跳过主机密钥校验
    let known_hosts = rt
        .known_hosts()
        .await
        .ok_or("runtime not ready: known hosts provider unavailable")?;

    let session_clone = session.clone();
    tokio::task::spawn_blocking(move || session_clone.connect(config, Some(known_hosts)))
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
    let sid = SessionId::parse(&session_id)?;

    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let session = rt.get_session(&sid).await.ok_or("session not found")?;

    // 合并为单次 spawn_blocking：shell 与 sftp 通道共享同一连接锁，串行执行减少线程池往返
    let session_clone = session.clone();
    let (shell_result, sftp_result) = tokio::task::spawn_blocking(move || {
        let shell = session_clone.open_shell();
        let sftp = session_clone.open_sftp();
        (shell, sftp)
    })
    .await
    .map_err(|e| e.to_string())?;

    // Shell 失败是致命错误，直接返回
    let channel = shell_result.map_err(|e| e.to_string())?;
    let channel_id = channel.id;

    {
        let mut channels = state.channels.write().await;
        channels.insert(channel_id, channel.clone());
    }

    // 读循环延迟到前端 Terminal 挂载后通过 start_terminal 触发

    // SFTP 通道即使失败也不影响 Shell 功能，只记录日志
    match sftp_result {
        Ok(ch) => {
            tracing::info!(channel_id = %ch.id, session_id = %sid, "sftp channel opened");
            let mut sftp_channels = state.sftp_channels.write().await;
            sftp_channels.insert(sid, ch);
        }
        Err(e) => {
            tracing::warn!(session_id = %sid, error = %e, "sftp channel open failed, file transfer unavailable");
        }
    }

    // 启动心跳保活（每 30 秒发送一次 keepalive 请求），并监督任务退出
    let kh_handle = core_runtime::spawn_keepalive(session, 30);
    tokio::spawn(async move {
        if let Err(e) = kh_handle.await {
            tracing::error!(%channel_id, error = %e, "keepalive task panicked");
        }
    });

    Ok(channel_id.to_string())
}


/// 前端 Terminal 组件挂载完成后调用，启动数据读取
/// 确保事件监听器就绪后才开始读 SSH 数据
#[tauri::command]
pub async fn start_terminal(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    channel_id: String,
) -> Result<(), String> {
    let cid = ChannelId::parse(&channel_id)?;

    let channels = state.channels.read().await;
    let channel = channels.get(&cid).ok_or("channel not found")?.clone();
    drop(channels);

    // 监督 read loop：任务异常退出时记录日志，避免终端静默冻结无感知
    let handle = channel.start_read_loop();
    let app2 = app.clone();
    tokio::spawn(async move {
        if let Err(e) = handle.await {
            tracing::error!(%cid, error = %e, "terminal read loop panicked");
            let _ = app2.emit("terminal-error", TerminalError { channel_id: cid.to_string() });
        }
    });
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
    let cid = ChannelId::parse(&channel_id)?;

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
    let cid = ChannelId::parse(&channel_id)?;

    let channels = state.channels.read().await;
    let channel = channels.get(&cid).ok_or("channel not found")?.clone();
    drop(channels);
    channel.resize_pty(cols, rows).await.map_err(|e| e.to_string())?;
    Ok(())
}


/// 关闭终端连接，清理 Session 和 Channel 资源
#[tauri::command]
pub async fn terminal_close(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    let sid = SessionId::parse(&session_id)?;

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

    // 清理 SFTP 通道
    {
        let mut sftp_channels = state.sftp_channels.write().await;
        sftp_channels.remove(&sid);
    }

    let guard = state.runtime.read().await;
    if let Some(ref rt) = *guard {
        rt.remove_session(&sid).await;
    }

    Ok(())
}


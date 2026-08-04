//! SFTP 文件传输命令子模块
//! 流式传输全程在 Rust 侧完成，避免大文件经 IPC 传输；
//! 传输完成后执行 SHA-256 完整性校验

use super::{get_sftp_channel, local::sha256_file, AppState};
use core_common::{FileEntry, SessionId};
use std::sync::Arc;
use tauri::{Emitter, State};

/// 传输进度事件 payload（前端通过 "transfer-progress" 事件接收）
/// 类型化定义：扩展字段时前后端同步修改，避免手拼 JSON 的字符串约定
#[derive(Debug, Clone, serde::Serialize)]
pub struct TransferProgress {
    pub id: String,
    pub done: u64,
    pub total: u64,
}

/// 列出远程目录内容
#[tauri::command]
pub async fn sftp_list_dir(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    let channel = get_sftp_channel(&state, &session_id).await?;
    tracing::debug!(session_id = %session_id, path = %path, "sftp list dir");

    tokio::task::spawn_blocking(move || channel.sftp_read_dir(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}


/// 创建远程目录
#[tauri::command]
pub async fn sftp_create_dir(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    path: String,
) -> Result<(), String> {
    let channel = get_sftp_channel(&state, &session_id).await?;

    tokio::task::spawn_blocking(move || channel.sftp_create_dir(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}


/// 删除远程文件或目录
#[tauri::command]
pub async fn sftp_delete(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    path: String,
    is_dir: bool,
) -> Result<(), String> {
    let channel = get_sftp_channel(&state, &session_id).await?;

    tokio::task::spawn_blocking(move || {
        if is_dir {
            channel.sftp_remove_dir(&path)
        } else {
            channel.sftp_remove_file(&path)
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}


/// 重命名远程文件
#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    old_path: String,
    new_path: String,
) -> Result<(), String> {
    let channel = get_sftp_channel(&state, &session_id).await?;

    tokio::task::spawn_blocking(move || channel.sftp_rename(&old_path, &new_path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}


/// 校验路径位于期望目录内，且不含父目录跳转组件
/// 在任何文件 IO 之前调用，防止路径穿越
fn validate_path_within(path: &str, expected_dir: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    let d = std::path::Path::new(expected_dir);
    let has_parent_traversal = p
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir));
    if has_parent_traversal {
        return Err("invalid path: contains parent directory traversal".into());
    }
    // 注意：Windows 上 Path::starts_with 按字节比较（大小写敏感），
    // 前端与后端路径来自同一来源，实际使用中大小写一致
    if !p.starts_with(d) {
        return Err("invalid path: target outside allowed directory".into());
    }
    Ok(())
}

/// 流式下载：远程 → 本地，全程在 Rust 侧完成，避免大文件经 IPC 传输
/// 进度通过 "transfer-progress" 事件推送（id 由前端生成用于关联）
/// 完成后执行 SHA-256 完整性校验（远端 sha256sum vs 本地哈希）
#[tauri::command]
pub async fn sftp_download_file(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    remote_path: String,
    local_path: String,
    task_id: String,
    expected_dir: String,
) -> Result<u64, String> {
    let channel = get_sftp_channel(&state, &session_id).await?;

    // 路径校验必须在任何 IO 之前执行：目标路径必须位于期望目录内，且拒绝父目录跳转
    validate_path_within(&local_path, &expected_dir)?;

    let rp = remote_path.clone();
    let lp = local_path.clone();
    let rp_io = rp.clone();
    let lp_io = lp.clone();
    let ch_io = channel.clone();
    let app2 = app.clone();
    let tid = task_id.clone();
    let done = tokio::task::spawn_blocking(move || {
        let mut last_emit: u64 = 0;
        ch_io.sftp_download_file(&rp_io, &lp_io, move |done, total| {
            // 节流：每 ~128KB 或完成时推送一次进度事件
            if done - last_emit >= 131072 || done == total {
                last_emit = done;
                let _ = app2.emit(
                    "transfer-progress",
                    TransferProgress { id: tid.clone(), done, total },
                );
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    // SHA-256 完整性校验：远端 hash vs 本地 hash
    let rt = state.runtime.read().await;
    let rt_ref = rt.as_ref().ok_or("runtime not initialized")?;
    let sid_verify = SessionId::parse(&session_id)?;
    let session = rt_ref.get_session(&sid_verify).await.ok_or("session not found")?;
    let rp2 = rp.clone();
    let lp2 = lp.clone();
    let (remote_hash, local_hash) = tokio::task::spawn_blocking(move || {
        let rh = session.remote_sha256(&rp2);
        let lh = sha256_file(&lp2);
        (rh, lh)
    })
    .await
    .map_err(|e| e.to_string())?;

    let remote_hash = remote_hash.map_err(|e| e.to_string())?;
    let local_hash = local_hash.map_err(|e| e.to_string())?;
    if !remote_hash.eq_ignore_ascii_case(&local_hash) {
        // 校验失败：删除不完整的本地文件并报错
        let _ = std::fs::remove_file(&lp);
        return Err(format!(
            "checksum mismatch: remote {remote_hash}, local {local_hash}"
        ));
    }
    Ok(done)
}


/// 流式上传：本地 → 远程，全程在 Rust 侧完成，避免大文件经 IPC 传输
/// 完成后执行 SHA-256 完整性校验（远端 sha256sum vs 本地哈希）
#[tauri::command]
pub async fn sftp_upload_file(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    remote_path: String,
    local_path: String,
    task_id: String,
    expected_dir: String,
) -> Result<u64, String> {
    let channel = get_sftp_channel(&state, &session_id).await?;

    // 路径校验必须在任何 IO 之前执行：本地源文件必须位于期望目录内
    validate_path_within(&local_path, &expected_dir)?;

    let rp = remote_path.clone();
    let lp = local_path.clone();
    let rp_io = rp.clone();
    let lp_io = lp.clone();
    let ch_io = channel.clone();
    let ch_verify = channel.clone();
    let app2 = app.clone();
    let tid = task_id.clone();
    let done = tokio::task::spawn_blocking(move || {
        let mut last_emit: u64 = 0;
        ch_io.sftp_upload_file(&rp_io, &lp_io, move |done, total| {
            if done - last_emit >= 131072 || done == total {
                last_emit = done;
                let _ = app2.emit(
                    "transfer-progress",
                    TransferProgress { id: tid.clone(), done, total },
                );
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    // SHA-256 完整性校验：远端 hash vs 本地 hash
    let rt = state.runtime.read().await;
    let rt_ref = rt.as_ref().ok_or("runtime not initialized")?;
    let sid_verify = SessionId::parse(&session_id)?;
    let session = rt_ref.get_session(&sid_verify).await.ok_or("session not found")?;
    let rp2 = rp.clone();
    let lp2 = lp.clone();
    let (remote_hash, local_hash) = tokio::task::spawn_blocking(move || {
        let rh = session.remote_sha256(&rp2);
        let lh = sha256_file(&lp2);
        (rh, lh)
    })
    .await
    .map_err(|e| e.to_string())?;

    let remote_hash = remote_hash.map_err(|e| e.to_string())?;
    let local_hash = local_hash.map_err(|e| e.to_string())?;
    if !remote_hash.eq_ignore_ascii_case(&local_hash) {
        // 校验失败：删除损坏的远端文件并报错
        let _ = ch_verify.sftp_remove_file(&rp);
        return Err(format!(
            "checksum mismatch: remote {remote_hash}, local {local_hash}"
        ));
    }
    Ok(done)
}


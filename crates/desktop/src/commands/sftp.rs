//! SFTP 文件传输命令子模块
//! 流式传输全程在 Rust 侧完成，避免大文件经 IPC 传输；
//! 传输完成后执行 SHA-256 完整性校验

use super::{AppState, get_sftp_channel, local::sha256_file};
use core_common::{FileEntry, SessionId};
use std::sync::Arc;
use tauri::{Emitter, State};

/// 传输进度事件 payload（前端通过 "transfer-progress" 事件接收）
/// 类型化定义：扩展字段时前后端同步修改，避免手拼 JSON 的字符串约定
/// verifying：传输完成进入校验阶段（大文件 SHA-256 校验可能耗时数秒，
/// 前端据此显示"校验中"提示，避免用户误以为传输未成功）
/// filename：目录传输当前文件相对路径（单文件传输为空串）
#[derive(Debug, Clone, serde::Serialize)]
pub struct TransferProgress {
    pub id: String,
    pub done: u64,
    pub total: u64,
    #[serde(default)]
    pub verifying: bool,
    #[serde(default)]
    pub filename: String,
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

/// 新建远程空文件（重名冲突由前端检测处理）
#[tauri::command]
pub async fn sftp_create_file(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    path: String,
) -> Result<(), String> {
    let channel = get_sftp_channel(&state, &session_id).await?;

    tokio::task::spawn_blocking(move || channel.sftp_create_file(&path))
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
                    TransferProgress {
                        id: tid.clone(),
                        done,
                        total,
                        verifying: false,
                        filename: String::new(),
                    },
                );
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    // SHA-256 完整性校验：远端 hash vs 本地 hash
    // 错误分级：哈希不匹配才删除文件并报错；远端无哈希命令或 exec 错误 → warn + 跳过校验保留文件
    let rt = state.runtime.read().await;
    let rt_ref = rt.as_ref().ok_or("runtime not initialized")?;
    let sid_verify = SessionId::parse(&session_id)?;
    let Some(session) = rt_ref.get_session(&sid_verify).await else {
        // 会话已在传输完成前被关闭（用户关闭标签/断开）：文件已完整传输，
        // 断开后校验无意义，跳过并成功返回（不报误导性的 session not found）
        tracing::warn!(session_id = %sid_verify, "session closed before checksum, verification skipped");
        return Ok(done);
    };
    drop(rt);
    // 传输完成进入校验：推送 verifying 状态（大文件校验耗时数秒，前端显示"校验中"提示，
    // 期间文件树未刷新（待校验通过），用户不会误以为传输失败）
    let _ = app.emit(
        "transfer-progress",
        TransferProgress {
            id: task_id.clone(),
            done,
            total: done,
            verifying: true,
            filename: String::new(),
        },
    );
    let rp2 = rp.clone();
    let lp2 = lp.clone();
    let (remote_hash, local_hash) = tokio::task::spawn_blocking(move || {
        let rh = session.remote_sha256(&rp2);
        let lh = sha256_file(&lp2);
        (rh, lh)
    })
    .await
    .map_err(|e| e.to_string())?;

    match remote_hash.map_err(|e| e.to_string())? {
        // 远端无哈希命令（如 macOS/BSD）：跳过校验并保留文件
        None => {
            tracing::warn!(%rp, "remote hash command unavailable, checksum skipped");
        }
        Some(rh) => {
            let local_hash = local_hash.map_err(|e| e.to_string())?;
            if !rh.eq_ignore_ascii_case(&local_hash) {
                // 校验失败：删除不完整的本地文件并报错
                // 不回传哈希值（内容指纹可能被离线枚举，非必要信息）
                let _ = std::fs::remove_file(&lp);
                return Err("checksum mismatch, downloaded file removed".into());
            }
        }
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
                    TransferProgress {
                        id: tid.clone(),
                        done,
                        total,
                        verifying: false,
                        filename: String::new(),
                    },
                );
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    // SHA-256 完整性校验：远端 hash vs 本地 hash
    // 错误分级：哈希不匹配才删除远端文件并报错；远端无哈希命令或 exec 错误 → warn + 跳过校验保留文件
    let rt = state.runtime.read().await;
    let rt_ref = rt.as_ref().ok_or("runtime not initialized")?;
    let sid_verify = SessionId::parse(&session_id)?;
    let Some(session) = rt_ref.get_session(&sid_verify).await else {
        // 会话已在传输完成前被关闭：上传已完成，跳过校验并成功返回
        tracing::warn!(session_id = %sid_verify, "session closed before checksum, verification skipped");
        return Ok(done);
    };
    drop(rt);
    // 传输完成进入校验：推送 verifying 状态（同下载侧注释）
    let _ = app.emit(
        "transfer-progress",
        TransferProgress {
            id: task_id.clone(),
            done,
            total: done,
            verifying: true,
            filename: String::new(),
        },
    );
    let rp2 = rp.clone();
    let lp2 = lp.clone();
    let (remote_hash, local_hash) = tokio::task::spawn_blocking(move || {
        let rh = session.remote_sha256(&rp2);
        let lh = sha256_file(&lp2);
        (rh, lh)
    })
    .await
    .map_err(|e| e.to_string())?;

    match remote_hash.map_err(|e| e.to_string())? {
        // 远端无哈希命令（如 macOS/BSD）：跳过校验并保留文件
        None => {
            tracing::warn!(%rp, "remote hash command unavailable, checksum skipped");
        }
        Some(rh) => {
            let local_hash = local_hash.map_err(|e| e.to_string())?;
            if !rh.eq_ignore_ascii_case(&local_hash) {
                // 校验失败：删除损坏的远端文件并报错（不回传哈希值，见下载侧注释）
                // 删除为阻塞 IPC 调用：async 命令内不得直接阻塞 tokio 线程，包 spawn_blocking
                // （与文件内其他阻塞操作的处理一致；删除失败不阻断主错误回报）
                let ch_rm = ch_verify.clone();
                let rp_rm = rp.clone();
                let _ = tokio::task::spawn_blocking(move || ch_rm.sftp_remove_file(&rp_rm)).await;
                return Err("checksum mismatch, uploaded file removed".into());
            }
        }
    }
    Ok(done)
}

/// 目录递归下载：进度按文件粒度上报（filename 为当前文件相对路径）
/// 不做逐文件 SHA-256 校验（目录级校验性能代价大；单文件传输保留校验）
#[tauri::command]
pub async fn sftp_download_tree(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    remote_path: String,
    local_path: String,
    task_id: String,
    expected_dir: String,
) -> Result<u64, String> {
    let channel = get_sftp_channel(&state, &session_id).await?;
    validate_path_within(&local_path, &expected_dir)?;

    let rp = remote_path.clone();
    let lp = local_path.clone();
    let ch_io = channel.clone();
    let app2 = app.clone();
    let tid = task_id.clone();
    tokio::task::spawn_blocking(move || {
        ch_io.sftp_download_tree(&rp, &lp, move |done, total, name| {
            let _ = app2.emit(
                "transfer-progress",
                TransferProgress {
                    id: tid.clone(),
                    done,
                    total,
                    verifying: false,
                    filename: name.to_string(),
                },
            );
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// 目录递归上传：进度按文件粒度上报（filename 为当前文件相对路径），不做逐文件校验
#[tauri::command]
pub async fn sftp_upload_tree(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    remote_path: String,
    local_path: String,
    task_id: String,
    expected_dir: String,
) -> Result<u64, String> {
    let channel = get_sftp_channel(&state, &session_id).await?;
    validate_path_within(&local_path, &expected_dir)?;

    let rp = remote_path.clone();
    let lp = local_path.clone();
    let ch_io = channel.clone();
    let app2 = app.clone();
    let tid = task_id.clone();
    tokio::task::spawn_blocking(move || {
        ch_io.sftp_upload_tree(&rp, &lp, move |done, total, name| {
            let _ = app2.emit(
                "transfer-progress",
                TransferProgress {
                    id: tid.clone(),
                    done,
                    total,
                    verifying: false,
                    filename: name.to_string(),
                },
            );
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

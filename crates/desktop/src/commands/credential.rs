//! 凭据命令子模块（OS 凭据管理器：密码/私钥口令的保存、读取、删除）

use super::AppState;
use core_common::credential::{Credential, CredentialKind};
use std::sync::Arc;
use tauri::State;

/// 解析凭据种类字符串（前端传 "password" | "passphrase"）
fn parse_kind(kind: &str) -> Result<CredentialKind, String> {
    match kind {
        "password" => Ok(CredentialKind::Password),
        "passphrase" => Ok(CredentialKind::Passphrase),
        _ => Err(format!("unsupported credential kind: {kind}")),
    }
}

/// 读取已保存凭据；未保存返回 null
#[tauri::command]
pub async fn load_credential(
    state: State<'_, Arc<AppState>>,
    host: String,
    port: u16,
    username: String,
    kind: String,
) -> Result<Option<String>, String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let provider = rt
        .credential_provider()
        .await
        .ok_or("credential provider not available")?;
    let k = parse_kind(&kind)?;
    let cred = provider
        .load_credential(&host, port, &username, k)
        .map_err(|e| e.to_string())?;
    Ok(cred.map(|c| c.secret))
}

/// 保存凭据（OS 凭据管理器，加密存储）
#[tauri::command]
pub async fn save_credential(
    state: State<'_, Arc<AppState>>,
    host: String,
    port: u16,
    username: String,
    kind: String,
    secret: String,
) -> Result<(), String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let provider = rt
        .credential_provider()
        .await
        .ok_or("credential provider not available")?;
    let k = parse_kind(&kind)?;
    provider
        .save_credential(&host, port, &username, &Credential { kind: k, secret })
        .map_err(|e| e.to_string())
}

/// 删除已保存凭据（幂等：未保存也返回成功）
#[tauri::command]
pub async fn delete_credential(
    state: State<'_, Arc<AppState>>,
    host: String,
    port: u16,
    username: String,
    kind: String,
) -> Result<(), String> {
    let guard = state.runtime.read().await;
    let rt = guard.as_ref().ok_or("runtime not initialized")?;
    let provider = rt
        .credential_provider()
        .await
        .ok_or("credential provider not available")?;
    let k = parse_kind(&kind)?;
    provider
        .delete_credential(&host, port, &username, k)
        .map_err(|e| e.to_string())
}

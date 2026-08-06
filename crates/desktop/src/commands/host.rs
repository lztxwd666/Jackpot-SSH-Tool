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


/// ping 结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct PingResult {
    pub success: bool,
    /// 往返延迟（毫秒）；解析失败时 None
    pub latency_ms: Option<u64>,
}

/// 从系统 ping 输出中解析往返延迟（毫秒）
/// Windows: "时间=12ms TTL=64" / "time=12ms TTL=64"；POSIX: "time=12.3 ms"
/// 解析失败返回 None（不影响 success 判定）
fn parse_ping_latency(output: &str) -> Option<u64> {
    let lower = output.to_lowercase();
    for token in ["time=", "时间=", "time<"] {
        if let Some(idx) = lower.find(token) {
            let rest = &lower[idx + token.len()..];
            let num: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(v) = num.parse::<f64>() {
                // "time<1ms" 这类以 < 表示的上限按 1 处理
                if token == "time<" {
                    return Some(1);
                }
                return Some(v.round() as u64);
            }
        }
    }
    None
}

/// Ping 目标校验：仅允许 hostname/IP 字符集（拒绝以 - 开头被解析为选项的输入，
/// 如 "-t 8.8.8.8" 在 Windows 会进入无限 ping 导致命令永不返回）
fn is_valid_ping_target(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'))
}

/// Ping 主机（诊断工具）：调用系统 ping（Windows -n 1 / 其他 -c 1），解析结果
/// 定位：诊断用途，不阻塞（spawn_blocking）、不持久化
#[tauri::command]
pub async fn ping_host(address: String) -> Result<PingResult, String> {
    if !is_valid_ping_target(&address) {
        return Err("invalid ping target".into());
    }
    let (success, output) = tokio::task::spawn_blocking(move || {
        let output = if cfg!(windows) {
            std::process::Command::new("ping")
                .args(["-n", "1", &address])
                .output()
        } else {
            std::process::Command::new("ping")
                .args(["-c", "1", &address])
                .output()
        };
        match output {
            Ok(out) => (out.status.success(), String::from_utf8_lossy(&out.stdout).to_string()),
            Err(e) => (false, format!("ping spawn failed: {e}")),
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(PingResult {
        success,
        latency_ms: parse_ping_latency(&output),
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ping_latency_windows() {
        assert_eq!(parse_ping_latency("来自 192.168.1.1 的回复: 字节=32 时间=12ms TTL=64"), Some(12));
    }

    #[test]
    fn test_parse_ping_latency_posix() {
        assert_eq!(parse_ping_latency("64 bytes from 1.1.1.1: icmp_seq=1 ttl=57 time=12.3 ms"), Some(12));
    }

    #[test]
    fn test_parse_ping_latency_upper_bound() {
        assert_eq!(parse_ping_latency("time<1ms"), Some(1));
    }

    #[test]
    fn test_parse_ping_latency_failure() {
        assert_eq!(parse_ping_latency("请求超时"), None);
        assert_eq!(parse_ping_latency(""), None);
    }
}


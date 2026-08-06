//! 本地文件系统命令子模块

use core_common::FileEntry;

/// 获取用户主目录（如 C:\Users\Administrator）
/// 修复：之前错误地返回了 app_data_dir
#[tauri::command]
pub async fn get_home_dir() -> Result<String, String> {
    if let Ok(home) = std::env::var("USERPROFILE") {
        return Ok(home);
    }
    if let Ok(home) = std::env::var("HOME") {
        return Ok(home);
    }
    Err("cannot determine user home directory".into())
}


/// 目录列举逻辑（阻塞版，供 spawn_blocking 调用：read_dir/metadata 可能因
/// 网络驱动器/慢盘阻塞数秒，不得占用 tokio worker 线程）
fn read_local_dir_blocking(path: &str) -> Result<Vec<FileEntry>, String> {
    // 空路径或根路径 → 列出驱动器
    if path.is_empty() || path == "\\" || path == "/" {
        return list_drives();
    }
    // "C:" 等驱动器根 → 规范化
    let path = if path.len() == 2 && path.ends_with(':') {
        format!("{}\\", path)
    } else {
        path.to_string()
    };

    let entries = std::fs::read_dir(&path).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with('$') {
            continue;
        }
        let modified = metadata
            .modified()
            .map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs().to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        files.push(FileEntry {
            name: name.clone(),
            path: entry.path().to_string_lossy().to_string(),
            size: metadata.len(),
            is_dir: metadata.is_dir(),
            modified,
        });
    }
    files.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(files)
}

/// 列出本地目录内容
/// 当 path 为空时列出驱动器（Windows）或根目录（其他平台）
#[tauri::command]
pub async fn read_local_dir(path: String) -> Result<Vec<FileEntry>, String> {
    tokio::task::spawn_blocking(move || read_local_dir_blocking(&path))
        .await
        .map_err(|e| e.to_string())?
}

/// 读取本地文件内容（上限 10MB：防御大文件经 IPC 撑爆内存与序列化开销；大文件请走 sftp 传输）
const READ_LOCAL_FILE_CAP: u64 = 10 * 1024 * 1024;

#[tauri::command]
pub async fn read_local_file(path: String) -> Result<Vec<u8>, String> {
    tokio::task::spawn_blocking(move || {
        let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
        if size > READ_LOCAL_FILE_CAP {
            return Err(format!("file too large: {size} bytes (cap 10MB)"));
        }
        std::fs::read(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}


/// 列出可用驱动器（Windows）
/// 注意：对不可用的网络驱动器，Path::exists() 可能阻塞数秒（Windows 重连尝试），
/// 这是 Windows 平台的固有行为，属于可接受的已知限制
#[cfg(windows)]
fn list_drives() -> Result<Vec<FileEntry>, String> {
    let mut drives = Vec::new();
    for letter in b'A'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        if std::path::Path::new(&drive).exists() {
            let name = format!("{}:", letter as char);
            drives.push(FileEntry {
                name,
                path: drive,
                size: 0,
                is_dir: true,
                modified: String::new(),
            });
        }
    }
    Ok(drives)
}


/// 非 Windows 平台：根目录作为唯一条目
#[cfg(not(windows))]
fn list_drives() -> Result<Vec<FileEntry>, String> {
    Ok(vec![FileEntry {
        name: "/".to_string(),
        path: "/".to_string(),
        size: 0,
        is_dir: true,
        modified: String::new(),
    }])
}


/// 写入本地文件（spawn_blocking：文件 IO 不占用 tokio worker 线程）
#[tauri::command]
pub async fn write_local_file(path: String, data: Vec<u8>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, &data).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}


/// 重命名本地文件或目录（spawn_blocking）
#[tauri::command]
pub async fn rename_local_file(old_path: String, new_path: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        std::fs::rename(&old_path, &new_path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}


/// 删除本地文件或目录（spawn_blocking；remove_dir_all 递归删除可能耗时）
#[tauri::command]
pub async fn delete_local_file(path: String, is_dir: bool) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        if is_dir {
            std::fs::remove_dir_all(&path).map_err(|e| e.to_string())
        } else {
            std::fs::remove_file(&path).map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}


/// 创建本地目录（spawn_blocking）
#[tauri::command]
pub async fn create_local_dir(path: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}


/// 计算本地文件 SHA-256 校验和
pub(crate) fn sha256_file(path: &str) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", hasher.finalize()))
}


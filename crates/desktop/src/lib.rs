//! desktop: Tauri v2 桌面应用入口模块
//! 负责初始化 CoreRuntime、注册 IPC 命令、并桥接 Rust 事件到 Vue 前端

mod commands;

use commands::AppState;
use core_common::DefaultConfig;
use std::sync::Arc;
use tauri::{Emitter, Manager};

/// Tauri 应用启动入口
/// setup 阶段创建 CoreRuntime 并启动事件转发循环
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let config = Box::new(DefaultConfig::new(
                app.path().app_data_dir()?,
                "info".to_string(),
            ));

            let runtime = core_runtime::CoreRuntime::new(config);
            let dispatcher = runtime.dispatcher();
            let mut event_rx = dispatcher.subscribe();

            let state = Arc::new(AppState {
                runtime: tokio::sync::RwLock::new(Some(runtime)),
            });
            app.manage(state);

            let app_handle = app.handle().clone();
            // 事件转发循环：CoreRuntime 产生的事件通过 tauri.emit 推送到前端
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = event_rx.recv().await {
                    let payload = serde_json::to_string(&event).unwrap_or_default();
                    let _ = app_handle.emit("core-event", payload);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::get_app_status, commands::ping])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

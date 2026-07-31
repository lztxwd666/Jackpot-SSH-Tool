//! desktop: Tauri v2 桌面应用入口模块
//! 负责初始化 CoreRuntime、注册 IPC 命令、并桥接 Rust 事件到 Vue 前端

mod commands;

use commands::AppState;
use core_common::DefaultConfig;
use std::sync::Arc;
use tauri::{Emitter, Manager};

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
            app.manage(state.clone());

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // 启动 CoreRuntime（初始化数据库、迁移、Provider）
                if let Some(ref rt) = *state.runtime.read().await {
                    if let Err(e) = rt.start().await {
                        tracing::error!(%e, "failed to start core runtime");
                        return;
                    }
                }

                // 事件转发循环：CoreRuntime → Tauri IPC → Vue 前端
                while let Ok(event) = event_rx.recv().await {
                    let payload = serde_json::to_string(&event).unwrap_or_else(|e| {
                        tracing::error!(%e, "failed to serialize event");
                        "{}".into()
                    });
                    if let Err(e) = app_handle.emit("core-event", payload) {
                        tracing::error!(%e, "failed to emit core-event");
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::ping,
            commands::list_hosts,
            commands::save_host,
            commands::delete_host,
            commands::search_hosts,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri application");
}

//! desktop: Tauri v2 桌面应用入口模块
//! 负责初始化 CoreRuntime、注册 IPC 命令、并桥接 Rust 事件到 Vue 前端

mod commands;

use commands::AppState;
use core_common::DefaultConfig;
use core_event::event::{ChannelEvent, CoreEvent};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            core_common::init_logging("info");

            let config = Box::new(DefaultConfig::new(
                app.path().app_data_dir()?,
                "info".to_string(),
            ));

            let runtime = core_runtime::CoreRuntime::new(config);
            let dispatcher = runtime.dispatcher();
            let mut event_rx = dispatcher.subscribe();

            let state = Arc::new(AppState {
                runtime: tokio::sync::RwLock::new(Some(runtime)),
                channels: tokio::sync::RwLock::new(HashMap::new()),
                sftp_channels: tokio::sync::RwLock::new(HashMap::new()),
            });
            app.manage(state.clone());

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // 启动 CoreRuntime（初始化数据库、迁移、Provider）
                if let Some(ref rt) = *state.runtime.read().await
                    && let Err(e) = rt.start().await
                {
                    tracing::error!(%e, "failed to start core runtime");
                    return;
                }

                // 事件转发循环：CoreRuntime → Tauri IPC → Vue 前端
                // 直接 emit 事件对象（Tauri 统一序列化一次；前端拿到已解析对象，无需二次 parse）
                loop {
                    match event_rx.recv().await {
                        Ok(event) => {
                            // 通道关闭事件（shell EOF 远端关闭/断开清理）：同步移除 desktop 层
                            // 通道注册表条目，terminal_send_input 等命令不再对已关闭通道报错刷日志
                            if let CoreEvent::Channel(ChannelEvent::Closed { channel_id, .. }) = &event {
                                state.channels.write().await.remove(channel_id);
                            }
                            if let Err(e) = app_handle.emit("core-event", &event) {
                                tracing::error!(%e, "failed to emit core-event");
                            }
                        }
                        // 接收者消费太慢导致事件被丢弃（如终端高吞吐输出）：记录告警
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(dropped = n, "core-event receiver lagged, events dropped");
                        }
                        // 通道关闭：退出转发循环
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::ping,
            commands::get_home_dir,
            commands::read_local_dir,
            commands::read_local_file,
            commands::write_local_file,
            commands::rename_local_file,
            commands::delete_local_file,
            commands::create_local_dir,
            commands::list_hosts,
            commands::save_host,
            commands::delete_host,
            commands::search_hosts,
            commands::approve_host_key,
            commands::ping_host,
            commands::load_credential,
            commands::save_credential,
            commands::delete_credential,
            commands::create_session,
            commands::connect_session,
            commands::open_shell,
            commands::start_terminal,
            commands::terminal_send_input,
            commands::terminal_resize,
            commands::terminal_close,
            commands::sftp_list_dir,
            commands::sftp_create_dir,
            commands::sftp_create_file,
            commands::sftp_delete,
            commands::sftp_rename,
            commands::sftp_download_file,
            commands::sftp_upload_file,
            commands::sftp_download_tree,
            commands::sftp_upload_tree,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri application");
}

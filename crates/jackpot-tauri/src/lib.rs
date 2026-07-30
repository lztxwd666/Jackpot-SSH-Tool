mod commands;

use commands::AppState;
use jackpot_core_common::DefaultConfig;
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

            let runtime = jackpot_core_runtime::CoreRuntime::new(config);
            let dispatcher = runtime.dispatcher();
            let mut event_rx = dispatcher.subscribe();

            let state = Arc::new(AppState {
                runtime: tokio::sync::RwLock::new(Some(runtime)),
            });
            app.manage(state);

            let app_handle = app.handle().clone();
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

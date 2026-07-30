use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

pub struct AppState {
    pub runtime: RwLock<Option<core_runtime::CoreRuntime>>,
}

#[tauri::command]
pub async fn get_app_status(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let guard = state.runtime.read().await;
    if guard.is_some() {
        Ok("running".to_string())
    } else {
        Ok("stopped".to_string())
    }
}

#[tauri::command]
pub async fn ping() -> Result<String, String> {
    Ok("pong".to_string())
}

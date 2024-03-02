use tauri::State;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::proxy_server::ProxyServer;

#[tauri::command]
pub async fn get_proxy_state(proxy_server: State<'_, Arc<Mutex<ProxyServer>>>) -> Result<bool, String> {
    let server = proxy_server.lock().await;
    Ok(server.get_state())
}

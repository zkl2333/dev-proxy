use crate::proxy_server::ProxyServer;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn get_proxy_state(
    proxy_server: State<'_, Arc<Mutex<ProxyServer>>>,
) -> Result<bool, String> {
    let server = proxy_server.lock().await;
    Ok(server.get_state())
}

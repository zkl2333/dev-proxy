use crate::{proxy_server::ProxyServer, session_handler::SessionHandlerData};
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn get_proxy_connections(
    proxy_server: State<'_, Arc<Mutex<ProxyServer>>>,
) -> Result<Vec<SessionHandlerData>, String> {
    let server = proxy_server.lock().await;
    let sessions = server.get_sessions_serializable().await;
    Ok(sessions)
}

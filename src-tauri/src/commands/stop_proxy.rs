use crate::proxy_server::ProxyServer;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn stop_proxy(
    proxy_server: State<'_, Arc<Mutex<ProxyServer>>>,
) -> Result<String, String> {
    let mut server = proxy_server.lock().await;
    match server.stop() {
        Ok(_) => Ok("代理服务器停止成功".to_string()),
        Err(e) => Err(format!("停止代理服务器失败: {}", e)),
    }
}

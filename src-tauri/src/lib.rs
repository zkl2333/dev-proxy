use std::sync::Arc;
use tokio::sync::Mutex;

pub mod proxy_server;
use proxy_server::ProxyServer;

pub mod commands;
use commands::{get_proxy_connections::*, get_proxy_state::*, start_proxy::*, stop_proxy::*};

pub mod protocol;
pub mod session_handler;
pub mod session_manager;

pub async fn run_proxy_server_app() {
    let subscriber = tracing_subscriber::fmt().with_thread_ids(true).finish();
    tracing::subscriber::set_global_default(subscriber).expect("Unable to set global logger");

    let proxy_server = Arc::new(Mutex::new(ProxyServer::new()));

    {
        let mut server = proxy_server.lock().await;
        let _ = match server.start().await {
            Ok(_) => Ok("Proxy server started successfully".to_string()),
            Err(e) => Err(format!("Failed to start proxy server: {}", e)),
        };
    }

    tauri::Builder::default()
        .manage(proxy_server)
        .invoke_handler(tauri::generate_handler![
            start_proxy,
            stop_proxy,
            get_proxy_state,
            get_proxy_connections
        ])
        .run(tauri::generate_context!())
        .expect("Error running Tauri application");
}

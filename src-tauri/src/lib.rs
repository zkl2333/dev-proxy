use std::sync::Arc;
use tokio::sync::Mutex;

pub mod proxy_server;
use proxy_server::ProxyServer;

pub mod commands;
use commands::{get_proxy_connections::*, get_proxy_state::*, start_proxy::*, stop_proxy::*};

pub mod protocol;
pub mod session_handler;
pub mod session_manager;

mod models;
// use crate::models::app_config::AppConfig;

use tauri::api::path::cache_dir;

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub async fn run_proxy_server_app() {
    let cache_path = cache_dir().expect("无法获取缓存目录");
    let log_file_path = cache_path.join("dev_proxy/logs");
    println!("日志文件路径: {:?}", log_file_path);
    let file_appender = tracing_appender::rolling::daily(log_file_path, "app.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // 创建针对终端的Layer
    let stdout_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_thread_ids(true);

    // 创建针对文件的Layer，使用非阻塞的写入器
    let file_layer = fmt::layer().with_writer(non_blocking).with_thread_ids(true);

    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .init();

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

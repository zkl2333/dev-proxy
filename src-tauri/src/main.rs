// 在Windows的发布版本中防止出现额外的控制台窗口，请不要删除！
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tokio::sync::Mutex;

mod proxy_server;
use proxy_server::ProxyServer;

mod commands;
use commands::{get_proxy_state::*, start_proxy::*, stop_proxy::*,get_proxy_connections::*};

mod session_handler;
use session_handler::SessionHandler;

mod session_manager;
use session_manager::SessionManager;

mod protocol;

#[tokio::main]
async fn main() {
    // 初始化日志记录器
    let subscriber = tracing_subscriber::fmt().with_thread_ids(true).finish();
    tracing::subscriber::set_global_default(subscriber).expect("无法设置全局日志记录器");

    let proxy_server = Arc::new(Mutex::new(ProxyServer::new()));

    {
        let mut server = proxy_server.lock().await;
        let _ = match server.start().await {
            Ok(_) => Ok("代理服务器启动成功".to_string()),
            Err(e) => Err(format!("启动代理服务器失败: {}", e)),
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
        .expect("运行Tauri应用时出错");
}

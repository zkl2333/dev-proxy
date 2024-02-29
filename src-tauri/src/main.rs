// 在Windows的发布版本中防止出现额外的控制台窗口，请不要删除！
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod proxy_control;
mod socks5;
use commands::{get_proxy_connections::*, get_proxy_state::*, start_proxy::*, stop_proxy::*};

#[tokio::main]
async fn main() {
    // 初始化日志记录器
    let subscriber = tracing_subscriber::fmt().with_thread_ids(true).finish();

    // 设置全局日志记录器
    tracing::subscriber::set_global_default(subscriber).expect("无法设置全局日志记录器");

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            start_proxy,
            stop_proxy,
            get_proxy_state,
            get_proxy_connections
        ])
        .run(tauri::generate_context!())
        .expect("运行Tauri应用时出错");
}

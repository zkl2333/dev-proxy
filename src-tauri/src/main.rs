// 在Windows的发布版本中防止出现额外的控制台窗口，请不要删除！
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands {
    pub mod get_proxy_state;
    pub mod start_proxy;
    pub mod stop_proxy;
}

mod proxy_control;
mod socks5;

use crate::proxy_control::PROXY_CONTROL;
use commands::{get_proxy_state::*, start_proxy::*, stop_proxy::*};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, Notify};
use tracing::{error, info};

async fn handle_client(
    _: usize,
    cancel_signal: Arc<Notify>,
    stream: Arc<Mutex<TcpStream>>,
    _: std::net::SocketAddr,
) {
    let connection_result = socks5::Socks5Connection::new(stream.clone(), cancel_signal).await;

    match connection_result {
        Ok(mut connection) => {
            if let Err(e) = connection.serve().await {
                error!("处理连接时发生错误: {}", e);
            } else {
                info!("连接处理完成");
            }
        }
        Err(e) => error!("创建SOCKS5连接失败: {}", e),
    }
}

//
async fn accept_connections(listener: TcpListener) {
    info!("代理已经启动，等待连接...");
    while let Ok((stream, addr)) = listener.accept().await {
        let stream = Arc::new(Mutex::new(stream));
        let proxy_control = PROXY_CONTROL.clone();
        let mut control = proxy_control.lock().await;
        let (id, cancel_signal) = control.add_connection(addr, stream.clone());

        tokio::spawn(async move {
            handle_client(id, cancel_signal, stream, addr).await;

            // 处理完成后，移除连接
            let proxy_control = PROXY_CONTROL.clone();
            let mut control = proxy_control.lock().await;
            control.remove_connection(id).await;
        });
    }
}

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
            get_proxy_state
        ])
        .run(tauri::generate_context!())
        .expect("运行Tauri应用时出错");
}

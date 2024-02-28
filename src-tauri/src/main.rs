// 在Windows的发布版本中防止出现额外的控制台窗口，请不要删除！
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod proxy_control;
mod socks5;

use once_cell::sync::Lazy;
use proxy_control::ProxyControl;
use std::sync::Arc;
use tokio::io;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Mutex};
use tracing::{error, info};

// 使用Mutex包装代理控制实例，以便在异步环境中安全访问
static PROXY_CONTROL: Lazy<Arc<Mutex<ProxyControl>>> =
    Lazy::new(|| Arc::new(Mutex::new(ProxyControl::new())));

// 异步处理客户端连接
async fn handle_client(
    id: usize,
    stream: Arc<Mutex<TcpStream>>,
    addr: std::net::SocketAddr,
) -> io::Result<()> {
    info!("接受到来自 {} 的连接", addr);
    match socks5::Socks5Connection::new(stream).await {
        Ok(mut connection) => {
            if let Err(e) = connection.serve().await {
                error!("处理连接时发生错误: {}", e);
            }
        }
        Err(e) => error!("创建SOCKS5连接失败: {}", e),
    }

    let proxy_control = PROXY_CONTROL.clone();
    let mut control = proxy_control.lock().await;
    control.remove_connection(id).await;

    info!("连接处理完成: {}", addr);
    Ok(())
}

// 一个独立的异步函数，用于接收连接
async fn accept_connections(listener: TcpListener) {
    info!("代理已经启动，等待连接...");
    while let Ok((stream, addr)) = listener.accept().await {
        let stream = Arc::new(Mutex::new(stream));
        let proxy_control = PROXY_CONTROL.clone();
        let mut control = proxy_control.lock().await;
        let id = control.add_connection(addr, stream.clone());
        tokio::spawn(handle_client(id, stream, addr));
    }
}

#[tokio::main]
async fn main() {
    // 初始化日志记录器
    let subscriber = tracing_subscriber::fmt().with_thread_ids(true).finish();

    // 设置全局日志记录器
    tracing::subscriber::set_global_default(subscriber).expect("无法设置全局日志记录器");

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![start_proxy, stop_proxy])
        .run(tauri::generate_context!())
        .expect("运行Tauri应用时出错");
}

#[tauri::command]
async fn start_proxy() -> Result<(), String> {
    let control = PROXY_CONTROL.clone();
    let mut guard = control.lock().await;

    if guard.state {
        return Err("代理已经在运行中".to_string());
    }

    guard.state = true;
    let (stop_sender, stop_receiver) = oneshot::channel::<()>();
    guard.stop_signal_sender = Some(stop_sender);

    // 释放锁，以便在异步操作中允许其他任务获取锁
    drop(guard);

    let listener = TcpListener::bind("127.0.0.1:1080")
        .await
        .map_err(|_| "无法绑定到代理端口".to_string())?;

    tokio::select! {
        _ = accept_connections(listener) => {},
        _ = stop_receiver => {
            println!("收到停止信号，代理即将停止...");
        },
    };

    let mut guard = control.lock().await;
    guard.reset().await;
    println!("代理已经停止。");
    Ok(())
}

#[tauri::command]
async fn stop_proxy() -> Result<String, String> {
    let control = PROXY_CONTROL.clone();
    let mut guard = control.lock().await;

    if let Some(sender) = guard.stop_signal_sender.take() {
        drop(guard); // 在发送信号前释放锁，避免死锁
        sender
            .send(())
            .map_err(|_| "无法发送停止信号，代理可能已经停止。".to_string())?;
        Ok("代理停止信号发送成功。".to_string())
    } else {
        Err("代理没有运行，无法停止。".to_string())
    }
}

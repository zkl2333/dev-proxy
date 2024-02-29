// 在Windows的发布版本中防止出现额外的控制台窗口，请不要删除！
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod proxy_control;
mod socks5;

use once_cell::sync::Lazy;
use proxy_control::ProxyControl;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Mutex, Notify};
use tracing::{error, info};

// 使用Mutex包装代理控制实例，以便在异步环境中安全访问
static PROXY_CONTROL: Lazy<Arc<Mutex<ProxyControl>>> =
    Lazy::new(|| Arc::new(Mutex::new(ProxyControl::new())));

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

#[tauri::command]
async fn start_proxy() -> Result<String, String> {
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

    // 在一个新的异步任务中启动代理服务
    tokio::spawn(async move {
        let listener = match TcpListener::bind("127.0.0.1:1080").await {
            Ok(listener) => listener,
            Err(_) => {
                let mut guard = control.lock().await;
                guard.reset().await;
                error!("无法绑定到代理端口");
                return;
            }
        };

        tokio::select! {
            _ = accept_connections(listener) => {},
            _ = stop_receiver => {
                info!("收到停止信号，代理即将停止...");
            },
        };

        let mut guard = control.lock().await;
        guard.reset().await;
        info!("代理已经停止。");
    });

    // 立即返回代理启动的结果
    Ok("代理启动中...".to_string())
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
        error!("代理没有运行，无法停止。");
        Err("代理没有运行，无法停止。".to_string())
    }
}

#[tauri::command]
async fn get_proxy_state() -> Result<bool, String> {
    let control = PROXY_CONTROL.clone();
    let guard = control.lock().await;
    Ok(guard.state)
}

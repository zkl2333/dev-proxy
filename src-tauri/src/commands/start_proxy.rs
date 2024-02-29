use crate::proxy_control::PROXY_CONTROL;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tracing::{error, info};

#[tauri::command]
pub async fn start_proxy() -> Result<String, String> {
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
            _ = crate::accept_connections(listener) => {},
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

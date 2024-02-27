// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Mutex};
use tracing::{error, info};
// 定义代理状态和控制逻辑的结构体

struct ProxyControl {
    state: bool,
    connections: Vec<Arc<Mutex<TcpStream>>>,
    stop_signal_sender: Option<oneshot::Sender<()>>,
}

impl ProxyControl {
    // 初始化新的代理控制实例
    fn new() -> Self {
        ProxyControl {
            state: false,
            connections: Vec::new(),
            stop_signal_sender: None,
        }
    }

    // 关闭所有活动连接并清空连接列表
    async fn close_and_clear_connections(&mut self) {
        // 遍历所有的连接
        for conn in self.connections.drain(..) {
            // 尝试获取 TcpStream 的锁
            let mut conn = conn.lock().await;
            // 尝试关闭 TcpStream
            if let Err(e) = conn.shutdown().await {
                error!("关闭连接失败: {}", e);
            }
        }
    }

    // 重置代理状态，准备下一次启动
    async fn reset(&mut self) {
        self.close_and_clear_connections().await;
        self.state = false;
        self.connections.clear();
        self.stop_signal_sender = None;
    }
}

// 使用Mutex包装代理控制实例，以便在异步环境中安全访问
static PROXY_CONTROL: Lazy<Arc<Mutex<ProxyControl>>> =
    Lazy::new(|| Arc::new(Mutex::new(ProxyControl::new())));

async fn forward_data(mut src: TcpStream, mut dst: TcpStream) -> io::Result<()> {
    // 使用 copy_bidirectional 进行双向数据转发
    let copy_result = tokio::io::copy_bidirectional(&mut src, &mut dst).await;

    // 检查 copy_bidirectional 的结果
    match copy_result {
        Ok((from_src_to_dst, from_dst_to_src)) => {
            info!(
                "从源到目标转发 {} 字节, 从目标到源转发 {} 字节",
                from_src_to_dst, from_dst_to_src
            );
            let _ = src.shutdown().await;
            let _ = dst.shutdown().await;
            info!("关闭连接");
        }
        Err(e) => {
            info!("数据转发时发生错误: {}", e);
            let _ = src.shutdown().await;
            let _ = dst.shutdown().await;
            info!("关闭连接");
        }
    }

    Ok(())
}

// 异步处理客户端连接
async fn handle_client(mut stream: TcpStream, addr: std::net::SocketAddr) -> io::Result<()> {
    info!("接受到来自 {} 的连接", addr);
    let mut buffer: [u8; 512] = [0u8; 512];

    // 1. 接收客户端的握手请求
    let buf_len = stream.read(&mut buffer).await?;
    let ver: u8 = buffer[0];
    let nmethods: u8 = buffer[1];
    let methods: Vec<u8> = buffer[2..(2 + nmethods as usize)].to_vec();
    info!(
        "握手请求 VER: {}, NMETHODS: {}, METHODS: {:?}",
        ver, nmethods, methods
    );
    if buf_len < 3 || ver != 0x05 {
        return Err(io::Error::new(io::ErrorKind::Other, "错误的握手请求"));
    }

    // 2. 响应握手请求（无需认证）
    stream.write_all(&[0x05, 0x00]).await?;
    info!("握手成功（无需认证）");

    // 3. 接收客户端的连接请求
    let buf_len = stream.read(&mut buffer).await?;
    let ver = buffer[0];
    let cmd = buffer[1];
    // 只实现了 CONNECT
    if buf_len < 7 || ver != 0x05 || cmd != 0x01 {
        error!("错误的连接请求 VER: {}, CMD: {}", ver, cmd);
        return Err(io::Error::new(io::ErrorKind::Other, "错误的连接请求"));
    }

    let atyp = buffer[3];
    let dst_addr: String = match atyp {
        // IPV4
        0x01 => {
            let slice = &buffer[4..8]; // 获取切片
            let bytes: Result<[u8; 4], _> = slice.try_into();
            match bytes {
                Ok(bytes) => {
                    let addr = std::net::Ipv4Addr::from(bytes);
                    addr.to_string()
                }
                Err(_) => {
                    return Err(io::Error::new(io::ErrorKind::Other, "错误的IPV4地址"));
                }
            }
        }
        // 域名
        0x03 => {
            let len = buffer[4] as usize;
            String::from_utf8(buffer[5..5 + len].to_vec()).unwrap()
        }
        // IPV6
        0x04 => {
            let slice = &buffer[3..19]; // 获取切片
            let bytes: Result<[u8; 16], _> = slice.try_into();
            match bytes {
                Ok(bytes) => {
                    let addr = std::net::Ipv6Addr::from(bytes);
                    addr.to_string()
                }
                Err(_) => {
                    return Err(io::Error::new(io::ErrorKind::Other, "错误的IPV6地址"));
                }
            }
        }
        _ => {
            return Err(io::Error::new(io::ErrorKind::Other, "不支持的地址类型"));
        }
    };
    let dst_port = ((buffer[buf_len - 2] as u16) << 8) | (buffer[buf_len - 1] as u16);
    info!(
        "连接请求 CMD: {}, ATYP: {}, DST_ADDR: {}, DST_PORT: {}",
        cmd, atyp, dst_addr, dst_port
    );

    let target_addr = format!("{}:{}", dst_addr, dst_port);
    info!("连接到: {}", target_addr);

    // 4. 尝试连接到目标服务器
    let target_stream = TcpStream::connect(target_addr).await?;
    info!("连接成功: {}", target_stream.peer_addr()?);

    // 5. 收到客户端的连接请求后，需要返回一个响应
    stream
        .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    info!("响应连接成功消息给客户端");

    // 使用forward_data函数转发数据
    let _ = forward_data(stream, target_stream).await;

    Ok(())
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
    let mut control = PROXY_CONTROL.lock().await;
    if control.state {
        error!("代理已经在运行中");
        return Err("代理已经在运行中".to_string());
    }

    // 创建一个新的one-shot通道，用于发送停止信号
    let (stop_sender, stop_receiver) = oneshot::channel::<()>();
    control.stop_signal_sender = Some(stop_sender);

    // 标记代理状态为运行中
    control.state = true;

    let listener = TcpListener::bind("127.0.0.1:1080")
        .await
        .expect("无法绑定到代理端口");

    // 使用tokio::select! 宏来同时等待新连接和停止信号
    tokio::select! {
        _ = accept_connections(listener) => {},
        _ = stop_receiver => {
            // 收到停止信号，退出监听循环
            info!("收到停止信号，代理即将停止...");
        },
    }

    info!("代理已经停止。");

    // 重置代理控制状态，准备下一次启动
    let mut control = PROXY_CONTROL.lock().await;
    control.reset().await;

    Ok(())
}

// 一个独立的异步函数，用于接收连接
async fn accept_connections(listener: TcpListener) {
    info!("代理已经启动，等待连接...");
    while let Ok((stream, addr)) = listener.accept().await {
        tokio::spawn(handle_client(stream, addr));
    }
}

#[tauri::command]
async fn stop_proxy() -> Result<String, String> {
    info!("尝试停止代理...");
    let mut control = PROXY_CONTROL.lock().await;
    if let Some(sender) = control.stop_signal_sender.take() {
        // 尝试发送停止信号。如果接收端已经被丢弃，就返回错误。
        sender
            .send(())
            .map_err(|_| "无法发送停止信号，代理可能已经停止。".to_string())?;

        Ok("代理已经停止。".to_string())
    } else {
        Err("代理没有运行，无法停止。".to_string())
    }
}

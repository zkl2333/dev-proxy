// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use once_cell::sync::Lazy;
use std::sync::Arc;
use time::macros::format_description;
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

// 异步转发数据
async fn forward_data(src: Arc<Mutex<TcpStream>>, dst: Arc<Mutex<TcpStream>>) -> io::Result<()> {
    let mut src_lock = src.lock().await;
    let mut dst_lock = dst.lock().await;

    // 尝试转发数据，并处理可能出现的错误
    match io::copy(&mut *src_lock, &mut *dst_lock).await {
        Ok(bytes_copied) => {
            info!(
                "成功转发了 {} 字节, 从 {} 到 {}",
                bytes_copied,
                src_lock.peer_addr()?,
                dst_lock.peer_addr()?
            );
            Ok(())
        }
        Err(e) => {
            error!("在转发数据时发生错误: {}", e);
            // 这里可以添加更多的错误处理逻辑，比如关闭连接等
            Err(e)
        }
    }
}

// 异步处理客户端连接
async fn handle_client(mut stream: TcpStream) -> io::Result<()> {
    info!("新连接: {}", stream.peer_addr()?);
    let mut buffer = [0u8; 512];

    // 1. 接收客户端的握手请求
    let n = stream.read(&mut buffer).await?;
    info!(
        "握手请求 VER: {}, NMETHODS: {}, METHODS: {:?}",
        buffer[0],
        buffer[1],
        &buffer[2..n]
    );
    if n < 3 || buffer[0] != 0x05 {
        return Err(io::Error::new(io::ErrorKind::Other, "错误的握手请求"));
    }

    // 2. 响应握手请求（无需认证）
    stream.write_all(&[0x05, 0x00]).await?;
    info!("响应握手请求（无需认证）");

    // 3. 接收客户端的连接请求
    let ver = buffer[0];
    let cmd = buffer[1];
    let n = stream.read(&mut buffer).await?;
    if n < 7 || ver != 0x05 || cmd != 0x01 {
        return Err(io::Error::new(io::ErrorKind::Other, "错误的连接请求"));
    }

    let atyp = buffer[3];
    let dst_addr: String = match atyp {
        // IPV4
        0x01 => {
            // format!("{}.{}.{}.{}", buffer[4], buffer[5], buffer[6], buffer[7]);
            let addr = format!("{}.{}.{}.{}", buffer[4], buffer[5], buffer[6], buffer[7]);
            addr
        }
        // 域名
        0x03 => {
            let len = buffer[4] as usize;
            String::from_utf8(buffer[5..5 + len].to_vec()).unwrap()
        }
        // IPV6
        0x04 => {
            let addr = format!(
                "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
                buffer[4],
                buffer[5],
                buffer[6],
                buffer[7],
                buffer[8],
                buffer[9],
                buffer[10],
                buffer[11]
            );
            addr
        }
        _ => {
            return Err(io::Error::new(io::ErrorKind::Other, "不支持的地址类型"));
        }
    };
    let dst_port = ((buffer[n - 2] as u16) << 8) | (buffer[n - 1] as u16);
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
        .write_all(&[ver, 0x00, 0x00, atyp, 0, 0, 0, 0, 0, 0])
        .await?;
    info!("响应连接成功消息给客户端");

    // 将连接添加到全局活动连接列表
    let src_arc = Arc::new(Mutex::new(stream));
    let dst_arc = Arc::new(Mutex::new(target_stream));

    // 添加到活动连接列表
    {
        let mut control = PROXY_CONTROL.lock().await;
        control.connections.push(src_arc.clone());
        control.connections.push(dst_arc.clone());
    }

    // 使用forward_data函数转发数据
    // 创建从客户端到服务器的数据转发任务
    let client_to_server = forward_data(src_arc.clone(), dst_arc.clone());

    // 创建从服务器到客户端的数据转发任务
    let server_to_client = forward_data(dst_arc.clone(), src_arc.clone());

    // 同时运行两个数据转发任务，并等待任意一个完成
    tokio::select! {
        result = client_to_server => {
            if let Err(e) = result {
                error!("客户端到服务器转发失败: {}", e);
            }
        },
        result = server_to_client => {
            if let Err(e) = result {
                error!("服务器到客户端转发失败: {}", e);
            }
        },
    };

    Ok(())
}

#[tokio::main]
async fn main() {
    // 设置时区
    let timer = tracing_subscriber::fmt::time::LocalTime::new(format_description!(
        "[year]-[month]-[day] [hour]:[minute]:[second]"
    ));
    // 初始化日志记录器
    let subscriber = tracing_subscriber::fmt()
        .compact()
        // 设置时区
        .with_timer(timer)
        .with_thread_ids(true)
        .finish();

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
        return Err("代理已经在运行中".to_string());
    }

    // 创建一个新的one-shot通道，用于发送停止信号
    let (stop_sender, stop_receiver) = oneshot::channel::<()>();
    control.stop_signal_sender = Some(stop_sender);

    // 标记代理状态为运行中
    control.state = true;

    tokio::spawn(async move {
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

        // 重置代理控制状态，准备下一次启动
        let mut control = PROXY_CONTROL.lock().await;
        control.reset().await;
    });

    Ok(())
}

// 一个独立的异步函数，用于接收连接
async fn accept_connections(listener: TcpListener) {
    info!("代理已经启动，等待连接...");
    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_client(stream));
    }
}

#[tauri::command]
async fn stop_proxy() -> Result<String, String> {
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

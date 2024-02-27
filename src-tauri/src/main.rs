// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Mutex};

// 定义全局变量来存储代理状态
static PROXY_STATE: Lazy<Arc<Mutex<bool>>> = Lazy::new(|| Arc::new(Mutex::new(false)));
// 定义用于存储活动连接的全局变量
static ACTIVE_CONNECTIONS: Lazy<Mutex<Vec<Arc<Mutex<TcpStream>>>>> =
    Lazy::new(|| Mutex::new(Vec::new()));
// 定义一个全局变量来存储停止信号发送端
static STOP_SIGNAL: Lazy<Arc<Mutex<Option<oneshot::Sender<()>>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

// 异步转发数据
async fn forward_data(src: Arc<Mutex<TcpStream>>, dst: Arc<Mutex<TcpStream>>) -> io::Result<u64> {
    println!(
        "开始转发数据 {} -> {}",
        src.lock().await.peer_addr()?,
        dst.lock().await.peer_addr()?
    );
    // 锁定源和目标TcpStream
    let mut src_lock = src.lock().await;
    let mut dst_lock = dst.lock().await;

    // 使用tokio::io::copy异步复制数据
    // 注意: 这里的复制是单向的，从src到dst
    let bytes_copied = io::copy(&mut *src_lock, &mut *dst_lock).await?;

    // 可以选择在这里关闭TcpStream，如果你认为合适
    // 例如：src_lock.shutdown().await?;
    // dst_lock.shutdown().await?;

    Ok(bytes_copied)
}

// 异步处理客户端连接
async fn handle_client(mut stream: TcpStream) -> io::Result<()> {
    println!("新连接: {}", stream.peer_addr()?);
    let mut buffer = [0u8; 512];

    // 1. 接收客户端的握手请求
    let n = stream.read(&mut buffer).await?;
    println!(
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
    println!("响应握手请求（无需认证）");

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
    println!(
        "连接请求 CMD: {}, ATYP: {}, DST_ADDR: {}, DST_PORT: {}",
        cmd, atyp, dst_addr, dst_port
    );

    let target_addr = format!("{}:{}", dst_addr, dst_port);
    println!("连接到: {}", target_addr);

    // 4. 尝试连接到目标服务器
    let target_stream = TcpStream::connect(target_addr).await?;
    println!("连接成功: {}", target_stream.peer_addr()?);

    // 5. 收到客户端的连接请求后，需要返回一个响应
    stream
        .write_all(&[ver, 0x00, 0x00, atyp, 0, 0, 0, 0, 0, 0])
        .await?;
    println!("响应连接成功消息给客户端");

    // 将连接添加到全局活动连接列表
    let src_arc = Arc::new(Mutex::new(stream));
    let dst_arc = Arc::new(Mutex::new(target_stream));

    // 添加到活动连接列表
    {
        let mut active_connections = ACTIVE_CONNECTIONS.lock().await;
        active_connections.push(src_arc.clone());
        active_connections.push(dst_arc.clone());
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
                eprintln!("客户端到服务器转发失败: {}", e);
            }
        },
        result = server_to_client => {
            if let Err(e) = result {
                eprintln!("服务器到客户端转发失败: {}", e);
            }
        },
    };

    Ok(())
}

#[tokio::main]
async fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![start_proxy, stop_proxy])
        .run(tauri::generate_context!())
        .expect("运行Tauri应用时出错");
}

#[tauri::command]
async fn start_proxy() -> Result<(), String> {
    let state = PROXY_STATE.lock().await;
    if *state {
        return Err("代理已经在运行中".to_string());
    }
    drop(state); // 提前释放锁

    // 创建一个one-shot通道，用于发送停止信号
    let (stop_sender, stop_receiver) = oneshot::channel::<()>();

    // 存储停止信号的发送端
    *STOP_SIGNAL.lock().await = Some(stop_sender);

    tokio::spawn(async move {
        let listener = TcpListener::bind("127.0.0.1:1080")
            .await
            .expect("无法绑定到代理端口");
        *PROXY_STATE.lock().await = true;

        // 使用tokio::select! 宏来同时等待新连接和停止信号
        tokio::select! {
            _ = accept_connections(listener) => {},
            _ = stop_receiver => {
                // 收到停止信号，退出监听循环
            },
        }

        *PROXY_STATE.lock().await = false;
    });

    Ok(())
}

// 一个独立的异步函数，用于接收连接
async fn accept_connections(listener: TcpListener) {
    println!("代理已经启动，等待连接...");
    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_client(stream));
    }
}

#[tauri::command]
async fn stop_proxy() -> Result<(), String> {
    println!("停止代理...");
    let mut state = PROXY_STATE.lock().await;
    if !*state {
        return Err("代理已经关闭".to_string());
    }

    *state = false;

    // 发送停止信号
    let maybe_sender = STOP_SIGNAL.lock().await.take(); // 移除并取得发送端
    if let Some(sender) = maybe_sender {
        // 发送停止信号
        let _ = sender.send(());
    } else {
        eprintln!("代理未在运行或已停止");
    }

    // 获取并关闭所有活动连接
    {
        let active_connections = ACTIVE_CONNECTIONS.lock().await;
        let connections: Vec<Arc<Mutex<TcpStream>>> = active_connections.iter().cloned().collect();
        drop(active_connections);

        println!("当前活动连接数: {}", connections.len());

        // 并行关闭所有连接，使用tokio的方法
        let futures: Vec<_> = connections
            .iter()
            .map(|stream_arc| async move {
                let mut stream = stream_arc.lock().await;
                let _ = stream.shutdown().await;
                println!("关闭连接: {}", stream.peer_addr().unwrap());
            })
            .collect();

        // 直接使用for循环等待每个future完成
        for future in futures {
            let _ = future.await; // 这里忽略了结果
        }

        // 清空活动连接列表
        let mut active_connections = ACTIVE_CONNECTIONS.lock().await;
        active_connections.clear();
    }

    Ok(())
}

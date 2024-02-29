use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::{oneshot, Mutex, Notify};
use tracing::{error, info};

// 使用Mutex包装代理控制实例，以便在异步环境中安全访问
pub static PROXY_CONTROL: Lazy<Arc<Mutex<ProxyControl>>> =
    Lazy::new(|| Arc::new(Mutex::new(ProxyControl::new())));

#[derive(Clone)]
pub struct Connection {
    id: usize,
    addr: SocketAddr,
    stream: Arc<Mutex<TcpStream>>,
    cancel_signal: Arc<Notify>,
}

pub struct ProxyControl {
    connections: HashMap<usize, Connection>,
    next_id: usize,
    pub state: bool,
    pub stop_signal_sender: Option<oneshot::Sender<()>>,
}

impl ProxyControl {
    // 初始化新的代理控制实例
    pub fn new() -> Self {
        ProxyControl {
            connections: HashMap::new(),
            next_id: 0,
            state: false,
            stop_signal_sender: None,
        }
    }

    // 重置代理状态，准备下一次启动
    pub async fn reset(&mut self) {
        self.cancel_all_connections().await;
        self.state = false;
        self.stop_signal_sender = None;
    }

    pub fn add_connection(
        &mut self,
        addr: SocketAddr,
        stream: Arc<Mutex<TcpStream>>,
    ) -> (usize, Arc<Notify>) {
        let id = self.next_id;
        self.next_id += 1;
        let cancel_signal = Arc::new(Notify::new());
        let connection = Connection {
            id,
            stream,
            addr,
            cancel_signal: cancel_signal.clone(),
        };
        self.connections.insert(id, connection);
        info!("ID: {} 连接 {} 已经添加", id, addr);
        (id, cancel_signal)
    }

    // 提供方法来触发特定连接的取消信号
    pub async fn cancel_connection(&self, id: usize) {
        info!("ID: {} 取消连接", id);
        if let Some(connection) = self.connections.get(&id) {
            info!("ID: {} 发送取消信号", id);
            connection.cancel_signal.notify_one();
            info!("ID: {} 取消信号已发送", id);
        } else {
            info!("ID: {} 连接不存在", id);
        }
    }

    pub async fn remove_connection(&mut self, id: usize) {
        if let Some(connection) = self.connections.remove(&id) {
            let mut stream = connection.stream.lock().await;
            let _ = match stream.shutdown().await {
                Ok(_) => Ok(()),
                Err(e) => {
                    error!("关闭连接时发生错误: {}", e);
                    Err(e)
                }
            };
            info!("ID: {} 连接 {} 移除成功", connection.id, connection.addr);
        } else {
            info!("ID: {} 连接不存在", id);
        }
    }

    pub async fn cancel_all_connections(&mut self) {
        let ids = self.connections.keys().cloned().collect::<Vec<_>>();
        info!("移除所有连接: {:?}", ids);
        for id in ids {
            self.cancel_connection(id).await;
        }
    }
}

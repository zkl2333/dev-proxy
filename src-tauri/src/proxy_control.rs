use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::{oneshot, Mutex};

struct Connection {
    id: usize,
    stream: Arc<Mutex<TcpStream>>,
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
        self.remove_all_connections().await;
        self.state = false;
        self.connections.clear();
        self.stop_signal_sender = None;
    }

    pub fn add_connection(&mut self, stream: Arc<Mutex<TcpStream>>) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let connection = Connection { id, stream };
        self.connections.insert(id, connection);
        id
    }

    pub async fn remove_connection(&mut self, id: usize) {
        if let Some(connection) = self.connections.remove(&id) {
            let mut stream = connection.stream.lock().await;
            let _ = stream.shutdown().await;
        }
    }

    pub async fn remove_all_connections(&mut self) {
        for id in self.connections.keys().cloned().collect::<Vec<_>>() {
            self.remove_connection(id).await;
        }
    }
}

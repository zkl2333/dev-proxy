use std::{
    net::SocketAddr,
    sync::{atomic::AtomicU64, Arc},
};

use serde::Serialize;
use tokio::{
    net::TcpStream,
    sync::{oneshot, Mutex},
};
use tracing::{info, instrument};

use crate::protocol::{self, ProxyProtocol, Socks5Handler};

// 自增长的会话ID
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(0);

// 自定义类型，用于延迟解析地址
#[derive(Debug, Serialize, Clone)]
pub enum Address {
    SocketAddr(SocketAddr),
    DomainName(String, u16),
}

// 会话状态枚举
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    // 解析请求
    RequestParsing,
    // 连接到目标服务器
    ConnectingToTarget,
    // 转发数据
    ForwardingData,
    // 取消
    Canceled,
    // 结束
    Finished,
}

// 处理单个客户端会话的结构体
pub struct SessionHandler {
    // 唯一标识
    pub id: u64,
    // 客户端连接
    client_connection: Arc<Mutex<TcpStream>>,
    // 目标服务器连接，可能在 SOCKS5 或 HTTP 请求处理过程中建立
    target_connection: Arc<Mutex<Option<TcpStream>>>,
    // 会话状态
    pub state: Arc<Mutex<SessionState>>,
    // 协议类型（SOCKS5或HTTP）
    protocol: ProxyProtocol,
    // 目标服务器地址
    target_addr: Arc<Mutex<Option<Address>>>,
    // 取消信号通道
    cancel_sender: Option<oneshot::Sender<()>>,
    cancel_receiver: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
}

#[derive(Serialize)]
pub struct SessionHandlerData {
    pub id: u64,
    pub state: SessionState,
    pub protocol: ProxyProtocol,
    pub target_addr: Option<String>,
}

impl Drop for SessionHandler {
    fn drop(&mut self) {
        info!("会话{}已销毁", self.id);
    }
}

impl SessionHandler {
    #[instrument(skip(client_connection))]
    pub async fn new(
        client_connection: TcpStream,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let protocol = protocol::identify_protocol(&client_connection).await?;

        let (cancel_sender, cancel_receiver) = oneshot::channel();
        let session = Self {
            id: NEXT_SESSION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            client_connection: Arc::new(Mutex::new(client_connection)),
            target_connection: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(SessionState::RequestParsing)),
            protocol,
            target_addr: Arc::new(Mutex::new(None)),
            cancel_sender: Some(cancel_sender),
            cancel_receiver: Arc::new(Mutex::new(Some(cancel_receiver))),
        };

        tracing::info!("创建会话: {}", session.id);
        Ok(session)
    }

    #[instrument(skip(self))]
    pub async fn run(&self) -> &Self {
        loop {
            let state = self.state.lock().await;
            info!("会话{}状态: {:?}", self.id, *state);
            match *state {
                SessionState::RequestParsing => {
                    drop(state);
                    // 处理认证逻辑
                    match self.protocol {
                        ProxyProtocol::Socks5 => {
                            let mut handler = Socks5Handler::new(self.client_connection.clone())
                                .await
                                .unwrap();
                            let addr = handler.parse().await;
                            match addr {
                                Ok(addr) => {
                                    *self.target_addr.lock().await = Some(addr);
                                    *self.state.lock().await = SessionState::ConnectingToTarget;
                                }
                                Err(e) => {
                                    tracing::error!("解析SOCKS5请求时出错: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                }
                SessionState::ConnectingToTarget => {
                    drop(state);
                    let target_addr = self.target_addr.lock().await;
                    match &*target_addr {
                        Some(Address::SocketAddr(addr)) => {
                            let target_connection = TcpStream::connect(addr).await;
                            match target_connection {
                                Ok(target_connection) => {
                                    *self.target_connection.lock().await = Some(target_connection);
                                    *self.state.lock().await = SessionState::ForwardingData;
                                }
                                Err(e) => {
                                    tracing::error!("连接到目标服务器时出错: {}", e);
                                    *self.state.lock().await = SessionState::Finished;
                                }
                            }
                        }
                        Some(Address::DomainName(addr, port)) => {
                            let target_connection =
                                TcpStream::connect((addr.as_str(), *port)).await;
                            match target_connection {
                                Ok(target_connection) => {
                                    *self.target_connection.lock().await = Some(target_connection);
                                    *self.state.lock().await = SessionState::ForwardingData;
                                }
                                Err(e) => {
                                    tracing::error!("连接到目标服务器时出错: {}", e);
                                    *self.state.lock().await = SessionState::Finished;
                                }
                            }
                        }
                        None => {
                            tracing::error!("目标地址为空");
                            *self.state.lock().await = SessionState::Finished;
                        }
                    }
                }
                SessionState::ForwardingData => {
                    drop(state);
                    let mut client_connection = self.client_connection.lock().await;
                    let mut target_connection = self.target_connection.lock().await;

                    let target_connection = match &mut *target_connection {
                        Some(connection) => connection,
                        None => {
                            tracing::error!("目标连接为空");
                            *self.state.lock().await = SessionState::Finished;
                            continue;
                        }
                    };

                    if let Some(receiver) = self.cancel_receiver.lock().await.take() {
                        tokio::select! {
                            _ = receiver => {
                                tracing::info!("会话{}已取消", self.id);
                                *self.state.lock().await = SessionState::Canceled;
                            }
                            result = tokio::io::copy_bidirectional(
                                &mut *client_connection,
                                &mut *target_connection,
                            ) => {
                                match result {
                                    Ok(_) => {
                                        *self.state.lock().await = SessionState::Finished;

                                    }
                                    Err(e) => {
                                        tracing::error!("数据转发时出错: {}", e);
                                        *self.state.lock().await = SessionState::Finished;
                                    }
                                }
                            }
                        }
                    }
                }
                SessionState::Canceled => {
                    tracing::info!("会话{}已取消", self.id);
                    break;
                }
                SessionState::Finished => {
                    tracing::info!("会话{}已结束", self.id);
                    break;
                }
            }
        }
        self
    }

    // 取消转发数据
    pub fn cancel(&mut self) {
        if let Some(sender) = self.cancel_sender.take() {
            let _ = sender.send(());
        }
    }

    #[instrument(skip(self))]
    pub async fn get_data(&self) -> SessionHandlerData {
        let state = self.state.lock().await;
        SessionHandlerData {
            id: self.id,
            state: *state,
            protocol: self.protocol,
            target_addr: match &*self.target_addr.lock().await {
                Some(addr) => match addr {
                    Address::SocketAddr(addr) => Some(addr.to_string()),
                    Address::DomainName(domain, port) => Some(format!("{}:{}", domain, port)),
                },
                None => None,
            },
        }
    }
}

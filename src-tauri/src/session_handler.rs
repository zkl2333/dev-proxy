use std::{
    net::SocketAddr,
    sync::{atomic::AtomicU64, Arc},
};

use serde::Serialize;
use tokio::{
    net::TcpStream,
    sync::{oneshot, Mutex},
};
use tracing::info;

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
    target_connection: Option<Arc<Mutex<TcpStream>>>,
    // 会话状态
    state: SessionState,
    // 协议类型（SOCKS5或HTTP）
    protocol: ProxyProtocol,
    // 目标服务器地址
    target_addr: Option<Address>,
    // 取消信号通道
    cancel_sender: Option<oneshot::Sender<()>>,
    cancel_receiver: Option<oneshot::Receiver<()>>,
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
    pub async fn new(
        client_connection: TcpStream,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let protocol = protocol::identify_protocol(&client_connection).await?;

        let (cancel_sender, cancel_receiver) = oneshot::channel();
        let session = Self {
            id: NEXT_SESSION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            client_connection: Arc::new(Mutex::new(client_connection)),
            target_connection: None,
            state: SessionState::RequestParsing,
            protocol,
            target_addr: None,
            cancel_sender: Some(cancel_sender),
            cancel_receiver: Some(cancel_receiver),
        };

        tracing::info!("创建会话: {}", session.id);
        Ok(session)
    }

    pub async fn run(&mut self) -> &Self {
        loop {
            info!("会话{}的状态: {:?}", self.id, self.state);
            match self.state {
                SessionState::RequestParsing => {
                    // 处理认证逻辑
                    match self.protocol {
                        ProxyProtocol::Socks5 => {
                            let mut handler = Socks5Handler::new(self.client_connection.clone())
                                .await
                                .unwrap();
                            let addr = handler.parse().await;
                            match addr {
                                Ok(addr) => {
                                    self.target_addr = Some(addr);
                                    self.state = SessionState::ConnectingToTarget;
                                }
                                Err(e) => {
                                    tracing::error!("解析SOCKS5请求时出错: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                }
                SessionState::ConnectingToTarget => match &self.target_addr {
                    Some(Address::SocketAddr(addr)) => {
                        let target_connection = TcpStream::connect(addr).await;
                        match target_connection {
                            Ok(target_connection) => {
                                self.target_connection =
                                    Some(Arc::new(Mutex::new(target_connection)));
                                self.state = SessionState::ForwardingData;
                            }
                            Err(e) => {
                                tracing::error!("连接到目标服务器时出错: {}", e);
                                break;
                            }
                        }
                    }
                    Some(Address::DomainName(addr, port)) => {
                        let target_connection = TcpStream::connect((addr.as_str(), *port)).await;
                        match target_connection {
                            Ok(target_connection) => {
                                self.target_connection =
                                    Some(Arc::new(Mutex::new(target_connection)));
                                self.state = SessionState::ForwardingData;
                            }
                            Err(e) => {
                                tracing::error!("连接到目标服务器时出错: {}", e);
                                break;
                            }
                        }
                    }
                    None => {
                        tracing::error!("目标地址为空");
                        break;
                    }
                },
                SessionState::ForwardingData => {
                    let mut client_connection = self.client_connection.lock().await;
                    let mut target_connection =
                        self.target_connection.as_ref().unwrap().lock().await;
                    if let Some(receiver) = self.cancel_receiver.take() {
                        tokio::select! {
                            _ = receiver => {
                                tracing::info!("会话{}已取消", self.id);
                                self.state = SessionState::Canceled;
                                break;
                            }
                            result = tokio::io::copy_bidirectional(
                                &mut *client_connection,
                                &mut *target_connection,
                            ) => {
                                match result {
                                    Ok(_) => {
                                        self.state = SessionState::Finished;

                                    }
                                    Err(e) => {
                                        tracing::error!("数据转发时出错: {}", e);
                                        self.state = SessionState::Finished;
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

    pub fn get_data(&self) -> SessionHandlerData {
        SessionHandlerData {
            id: self.id,
            state: self.state,
            protocol: self.protocol,
            target_addr: match &self.target_addr {
                Some(addr) => match addr {
                    Address::SocketAddr(addr) => Some(addr.to_string()),
                    Address::DomainName(domain, port) => Some(format!("{}:{}", domain, port)),
                },
                None => None,
            },
        }
    }
}

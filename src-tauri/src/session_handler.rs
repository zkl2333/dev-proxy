use std::{
    net::SocketAddr,
    sync::{atomic::AtomicU64, Arc},
};

use serde::Serialize;
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot, Mutex},
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
    pub state: SessionState,
    // 协议类型（SOCKS5或HTTP）
    protocol: ProxyProtocol,
    // 目标服务器地址
    target_addr: Option<Address>,
    // 取消信号通道
    cancel_sender: Option<oneshot::Sender<()>>,
    cancel_receiver: Option<oneshot::Receiver<()>>,
    state_control_sender: mpsc::Sender<SessionState>,
    state_control_receiver: mpsc::Receiver<SessionState>,
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
        let (state_control_sender, state_control_receiver) = mpsc::channel(1);
        let session = Self {
            id: NEXT_SESSION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            client_connection: Arc::new(Mutex::new(client_connection)),
            target_connection: None,
            state: SessionState::RequestParsing,
            protocol,
            target_addr: None,
            cancel_sender: Some(cancel_sender),
            cancel_receiver: Some(cancel_receiver),
            state_control_sender,
            state_control_receiver,
        };

        // 发送初始状态消息（例如，RequestParsing，表示开始解析请求）
        let _ = session.state_control_sender.send(SessionState::RequestParsing).await;

        tracing::info!("创建会话: {}", session.id);
        Ok(session)
    }

    pub async fn run(&mut self) -> &Self {
        info!("会话{}开始运行", self.id);
        while let Some(state) = self.state_control_receiver.recv().await {
            self.state = state;
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
                                    // self.state = SessionState::ConnectingToTarget;
                                    let _ = self
                                        .state_control_sender
                                        .send(SessionState::ConnectingToTarget)
                                        .await;
                                }
                                Err(e) => {
                                    tracing::error!("解析SOCKS5请求时出错: {}", e);
                                    let _ = self.state_control_sender.send(SessionState::Finished).await;
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
                                // self.state = SessionState::ForwardingData;
                                let _ = self
                                    .state_control_sender
                                    .send(SessionState::ForwardingData)
                                    .await;
                            }
                            Err(e) => {
                                tracing::error!("连接到目标服务器时出错: {}", e);
                                let _ = self.state_control_sender.send(SessionState::Finished).await;
                            }
                        }
                    }
                    Some(Address::DomainName(addr, port)) => {
                        let target_connection = TcpStream::connect((addr.as_str(), *port)).await;
                        match target_connection {
                            Ok(target_connection) => {
                                self.target_connection =
                                    Some(Arc::new(Mutex::new(target_connection)));
                                let _ = self
                                    .state_control_sender
                                    .send(SessionState::ForwardingData)
                                    .await;
                            }
                            Err(e) => {
                                tracing::error!("连接到目标服务器时出错: {}", e);
                                let _ = self.state_control_sender.send(SessionState::Finished).await;
                            }
                        }
                    }
                    None => {
                        // tracing::error!("目标地址为空");
                        let _ = self.state_control_sender.send(SessionState::Finished).await;
                    }
                },
                SessionState::ForwardingData => {
                    if let Some(cancel_receiver) = self.cancel_receiver.take() {
                        self.forward_data(cancel_receiver).await;
                    }else {
                        tracing::error!("取消信号通道丢失");
                        let _ = self.state_control_sender.send(SessionState::Finished).await;
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

    async fn forward_data(&self, cancel_receiver: oneshot::Receiver<()>) {
        let client_connection = self.client_connection.clone();
        let target_connection = self.target_connection.clone().unwrap(); // 假设已经设置
        let state_sender = self.state_control_sender.clone();

        tokio::spawn(async move {
            let mut client_connection = client_connection.lock().await;
            let mut target_connection = target_connection.lock().await;

            tokio::select! {
                result = tokio::io::copy_bidirectional(&mut *client_connection, &mut *target_connection) => {
                    match result {
                        Ok(_) => {
                            // 数据转发成功，发送Finished状态
                            let _ = state_sender.send(SessionState::Finished).await;
                        }
                        Err(_) => {
                            // 数据转发失败，同样视为结束
                            let _ = state_sender.send(SessionState::Finished).await;
                        }
                    }
                },
                _ = cancel_receiver => {
                    // 收到取消信号，提前结束
                    println!("数据转发被取消");
                    let _ = state_sender.send(SessionState::Canceled).await;
                }
            }
        });

        info!("会话{}开始转发数据", self.id);
    }

    // 取消转发数据
    pub fn cancel(&mut self) {
        info!("会话{}取消", self.id);
        if let Some(cancel_sender) = self.cancel_sender.take() {
            let _ = cancel_sender.send(());
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

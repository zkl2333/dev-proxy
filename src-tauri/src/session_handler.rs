use std::{
    net::SocketAddr,
    sync::{atomic::AtomicU64, Arc},
};

use tokio::{io::AsyncWriteExt, net::TcpStream, sync::Mutex};
use tracing::info;

use crate::protocol::{self, ProxyProtocol, Socks5Handler};

// 自增长的会话ID
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(0);

// 自定义类型，用于延迟解析地址
pub enum Address {
    SocketAddr(SocketAddr),
    DomainName(String, u16),
}

// 会话状态枚举
#[derive(Debug)]
enum SessionState {
    // 解析请求
    RequestParsing,
    // 连接到目标服务器
    ConnectingToTarget,
    // 转发数据
    ForwardingData,
    // 销毁
    Destroyed,
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
}

impl SessionHandler {
    pub async fn new(
        client_connection: TcpStream,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let protocol = protocol::identify_protocol(&client_connection).await?;

        let session = Self {
            id: NEXT_SESSION_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            client_connection: Arc::new(Mutex::new(client_connection)),
            target_connection: None,
            state: SessionState::RequestParsing,
            protocol,
            target_addr: None,
        };

        tracing::info!("创建会话: {}", session.id);
        Ok(session)
    }
    pub async fn run(&mut self) {
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
                    let (mut client_reader, mut client_writer) = client_connection.split();
                    let (mut target_reader, mut target_writer) = target_connection.split();

                    let client_to_target = tokio::io::copy(&mut client_reader, &mut target_writer);
                    let target_to_client = tokio::io::copy(&mut target_reader, &mut client_writer);

                    tokio::select! {
                        result = client_to_target => {
                            if let Err(e) = result {
                                tracing::error!("从客户端到目标服务器的数据转发时出错: {}", e);
                            }
                        }
                        result = target_to_client => {
                            if let Err(e) = result {
                                tracing::error!("从目标服务器到客户端的数据转发时出错: {}", e);
                            }
                        }
                    }
                    break;
                }
                SessionState::Destroyed => {
                    break;
                }
            }
        }
    }

    // 销毁会话
    pub async fn destroy(mut self) {
        // tracing::info!("销毁会话: {}", self.id);

        // // 关闭客户端连接
        // let _ = self.client_connection.shutdown().await;
        // // 如果存在目标连接，则关闭
        // if let Some(mut target_conn) = self.target_connection.take() {
        //     let _ = target_conn.shutdown().await;
        // }

        // // 此处可以添加其他清理逻辑，比如更新状态或发送特定的断开消息
        // self.state = SessionState::Destroyed;
    }
}

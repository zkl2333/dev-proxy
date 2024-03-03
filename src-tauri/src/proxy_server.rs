use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::{io, sync::Mutex};

use crate::session_handler::SessionHandlerData;
use crate::session_manager::SessionManagerCommand;
use crate::{SessionHandler, SessionManager};

// ProxyServer定义
pub struct ProxyServer {
    stop_signal: Option<oneshot::Sender<()>>,
    session_manager_command_sender: mpsc::Sender<SessionManagerCommand>,
}

impl ProxyServer {
    pub fn new() -> Self {
        let (_session_manager, sender) = SessionManager::new();
        Self {
            stop_signal: None,
            session_manager_command_sender: sender,
        }
    }

    // 启动代理服务器的逻辑
    pub async fn start(&mut self) -> io::Result<()> {
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let listener = TcpListener::bind("127.0.0.1:1080").await?;
        self.stop_signal = Some(stop_tx);

        // 获取`session_manager_command_sender`的克隆以在异步任务中使用
        let session_manager_sender = self.session_manager_command_sender.clone();

        tokio::spawn(async move {
            tokio::select! {
                _ = async {
                    while let Ok((stream, _)) = listener.accept().await {
                        // 在这里处理新连接
                        let session = SessionHandler::new(stream).await;
                        match session {
                            Ok(session) => {
                                let session_arc = Arc::new(Mutex::new(session));
                                // 使用克隆的sender发送添加会话的命令
                                if let Err(e) = session_manager_sender.send(SessionManagerCommand::Add(session_arc.clone())).await {
                                    tracing::error!("发送会话管理命令时出错: {}", e);
                                } else {
                                    tracing::info!("新会话已添加");
                                    let session_manager_sender_clone_inside = session_manager_sender.clone();
                                    // 启动会话
                                    tokio::spawn(async move {
                                        let mut session_locked = session_arc.lock().await;
                                        let session = session_locked.run().await;
                                        tracing::info!("会话{}已结束", session.id);
                                        session_manager_sender_clone_inside.send(SessionManagerCommand::Remove(session_arc.clone())).await.unwrap();
                                    });
                                }
                            }
                            Err(e) => {
                                tracing::error!("处理新连接时出错: {}", e);
                            }
                        }
                           }
                } => {},
                _ = stop_rx => {
                    // 停止监听新的连接
                    tracing::info!("停止监听新的连接");
                },
            }
        });

        Ok(())
    }

    // 停止代理服务器的逻辑
    pub fn stop(&mut self) -> Result<(), &'static str> {
        if let Some(stop_signal) = self.stop_signal.take() {
            stop_signal.send(()).map_err(|_| "发送停止信号失败")
        } else {
            Err("代理服务器未运行或已停止")
        }
    }

    // 获取代理服务器的状态
    pub fn get_state(&self) -> bool {
        self.stop_signal.is_some()
    }

    pub async fn get_sessions(&self) -> Vec<Arc<Mutex<SessionHandler>>> {
        let (tx, rx) = oneshot::channel();
        if self
            .session_manager_command_sender
            .send(SessionManagerCommand::GetSessions(tx))
            .await
            .is_ok()
        {
            if let Ok(sessions) = rx.await {
                return sessions;
            }
        }
        vec![] // 如果出现错误，则返回空列表
    }

    pub async fn get_sessions_serializable(&self) -> Vec<SessionHandlerData> {
        let sessions = self.get_sessions().await;
        let mut session_data_list = Vec::new();
        for session in sessions {
            let session = session.lock().await;
            session_data_list.push(session.get_data());
        }
        session_data_list
    }
}

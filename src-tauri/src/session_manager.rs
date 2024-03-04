use crate::session_handler::SessionHandler;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};

type Session = Arc<SessionHandler>;

pub enum SessionManagerCommand {
    Add(Session),
    GetSessions(oneshot::Sender<Vec<Session>>),
}

pub struct SessionManager {
    pub sessions: Vec<Session>,
}

impl SessionManager {
    pub fn new() -> (Arc<Mutex<Self>>, mpsc::Sender<SessionManagerCommand>) {
        let (sender, receiver) = mpsc::channel(100);

        let manager = Arc::new(Mutex::new(Self {
            sessions: Vec::new(),
        }));

        let manager_clone = manager.clone();
        tokio::spawn(async move {
            let mut manager = manager_clone.lock().await;
            manager.run(receiver).await;
        });

        (manager, sender)
    }

    // 开始运行会话管理器
    pub async fn run(&mut self, mut receiver: mpsc::Receiver<SessionManagerCommand>) {
        // 接收命令并处理
        while let Some(command) = receiver.recv().await {
            match command {
                SessionManagerCommand::Add(session) => {
                    self.add_session(session.clone());
                    tokio::spawn(async move {
                        let session_locked = session;
                        let session = session_locked.run().await;
                        tracing::info!("会话 run 结束 {}状态: {:?}", session.id, session.state);
                    });
                }
                SessionManagerCommand::GetSessions(sender) => {
                    // 克隆会话列表
                    let sessions_clone = self.sessions.clone();
                    // 尝试发送会话列表 如果失败则说明接收方已经被丢弃
                    if sender.send(sessions_clone).is_err() {
                        tracing::error!("发送会话列表时出错");
                    }
                }
            }
        }
    }

    pub fn add_session(&mut self, session: Session) {
        self.sessions.push(session);
    }

    pub fn cancel_session() {
        todo!("取消会话")
    }
}

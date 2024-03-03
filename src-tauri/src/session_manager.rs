use crate::session_handler::SessionHandler;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};

pub enum SessionManagerCommand {
    Add(Arc<Mutex<SessionHandler>>),
    Remove(Arc<Mutex<SessionHandler>>),
    GetSessions(oneshot::Sender<Vec<Arc<Mutex<SessionHandler>>>>),
}

pub struct SessionManager {
    pub sessions: Vec<Arc<Mutex<SessionHandler>>>,
}

impl SessionManager {
    pub fn new() -> (Arc<Mutex<Self>>, mpsc::Sender<SessionManagerCommand>) {
        let (sender, receiver) = mpsc::channel(100);

        let manager = Arc::new(Mutex::new(Self {
            sessions: Vec::new(),
        }));

        let manager_clone = manager.clone();
        tokio::spawn(async move {
            // 由于 manager_clone 是 Arc<Mutex<Self>>，我们需要先获取锁
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
                    self.add_session(session);
                }
                SessionManagerCommand::Remove(session) => {
                    self.remove_session(session).await;
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

    pub fn add_session(&mut self, session: Arc<Mutex<SessionHandler>>) {
        self.sessions.push(session);
    }

    pub async fn remove_session(&mut self, session: Arc<Mutex<SessionHandler>>) {
        // 判断是否存在
        for (i, s) in self.sessions.iter().enumerate() {
            if Arc::ptr_eq(s, &session) {
                session.lock().await.cancel();
                self.sessions.remove(i);
                break;
            }
        }
    }
}

use crate::proxy_control::PROXY_CONTROL;
use tracing::error;

#[tauri::command]
pub(crate) async fn stop_proxy() -> Result<String, String> {
    let control = PROXY_CONTROL.clone();
    let mut guard = control.lock().await;

    if let Some(sender) = guard.stop_signal_sender.take() {
        drop(guard); // 在发送信号前释放锁，避免死锁
        sender
            .send(())
            .map_err(|_| "无法发送停止信号，代理可能已经停止。".to_string())?;
        Ok("代理停止信号发送成功。".to_string())
    } else {
        error!("代理没有运行，无法停止。");
        Err("代理没有运行，无法停止。".to_string())
    }
}

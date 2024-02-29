use crate::proxy_control::PROXY_CONTROL;

#[tauri::command]
pub(crate) async fn get_proxy_state() -> Result<bool, String> {
    let control = PROXY_CONTROL.clone();
    let guard = control.lock().await;
    Ok(guard.state)
}

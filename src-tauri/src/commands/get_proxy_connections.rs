use crate::proxy_control::{SerializableConnection, PROXY_CONTROL};
use std::collections::HashMap;

#[tauri::command]
pub(crate) async fn get_proxy_connections() -> Result<HashMap<usize, SerializableConnection>, String>
{
    let control = PROXY_CONTROL.clone();
    let guard = control.lock().await;
    let connections = guard.get_serializable_connections().await;
    Ok(connections)
}

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct AppConfig {
    pub setting1: String,
    pub setting2: i32,
    // 其他配置项...
}

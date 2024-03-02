use std::{error::Error, future::Future, pin::Pin};
use tokio::net::TcpStream;

mod socks5;
pub use socks5::Socks5Handler;

// 代理协议枚举
pub enum ProxyProtocol {
    Socks5,
    // 如果添加了更多协议，可以在这里继续添加
}

pub trait ProtocolHandler {
    fn new(client_connection: TcpStream) -> Self
    where
        Self: Sized;

    // 返回一个动态分派的 Future，而不是直接使用 async 方法
    fn handle_request(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn Error>>> + Send>>;
}

// 识别协议的函数
pub async fn identify_protocol(stream: &TcpStream) -> std::io::Result<ProxyProtocol> {
    let mut buffer = [0; 1];
    stream.peek(&mut buffer).await?;

    // 检查是否为SOCKS5协议
    if buffer[0] == 0x05 {
        return Ok(ProxyProtocol::Socks5);
    }

    // 如果添加了更多协议，可以在这里继续添加识别逻辑
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "无法识别的代理协议",
    ))
}

use crate::session_handler::Address;
use std::sync::Arc;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

pub struct Socks5Handler {
    stream: Arc<Mutex<TcpStream>>,
}

impl Socks5Handler {
    pub async fn new(stream: Arc<Mutex<TcpStream>>) -> io::Result<Self> {
        Ok(Socks5Handler { stream })
    }

    async fn handshake(&mut self) -> io::Result<()> {
        let mut stream = self.stream.lock().await;

        let ver = stream.read_u8().await?;
        if ver != 0x05 {
            // 不是SOCKS5协议
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("非SOCKS5协议: {}", ver),
            ));
        }
        let nmethods = stream.read_u8().await?;
        let mut methods = vec![0u8; nmethods as usize];
        stream.read_exact(&mut methods).await?;
        // methods 中是否包含 0x00，即无需认证方式
        if !methods.contains(&0x00) {
            // 不支持无需认证的方式 拒绝
            stream.write_all(&[0x05, 0xFF]).await?;
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("不支持的认证方式:METHODS:{:?}", methods),
            ));
        }
        // 发送无需认证的方式
        stream.write_all(&[0x05, 0x00]).await?;
        Ok(())
    }

    async fn parse_request(&mut self) -> io::Result<Address> {
        let mut stream = self.stream.lock().await;
        let ver = stream.read_u8().await?;
        if ver != 0x05 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("非SOCKS5协议: {}", ver),
            ));
        }
        let cmd = stream.read_u8().await?;
        if cmd != 0x01 {
            return Err(io::Error::new(io::ErrorKind::Other, "只支持 CONNECT 请求"));
        }
        let _rsv = stream.read_u8().await?;
        let atyp = stream.read_u8().await?;
        match atyp {
            0x01 => {
                // IPv4
                let mut buf = [0u8; 4];
                stream.read_exact(&mut buf).await?;
                let addr = std::net::Ipv4Addr::from(buf);
                let port = stream.read_u16().await?;
                Ok(Address::SocketAddr(std::net::SocketAddr::new(
                    addr.into(),
                    port,
                )))
            }
            0x03 => {
                // 域名
                let len = stream.read_u8().await? as usize;
                let mut buf = vec![0u8; len];
                stream.read_exact(&mut buf).await?;
                let addr = String::from_utf8(buf)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "无效的域名格式"))?;
                let port = stream.read_u16().await?;
                Ok(Address::DomainName(addr, port))
            }
            0x04 => {
                // IPv6
                let mut buf = [0u8; 16];
                stream.read_exact(&mut buf).await?;
                let addr = std::net::Ipv6Addr::from(buf);
                let port = stream.read_u16().await?;
                Ok(Address::SocketAddr(std::net::SocketAddr::new(
                    addr.into(),
                    port,
                )))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "不支持的地址类型",
            )),
        }
    }

    // 解析请求 返回目标地址
    pub async fn parse(&mut self) -> io::Result<Address> {
        self.handshake().await?;
        let addr = self.parse_request().await?;
        self.stream
            .lock()
            .await
            .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await?;
        Ok(addr)
    }
}

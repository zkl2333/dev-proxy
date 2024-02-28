use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::info;

pub struct Socks5Connection {
    stream: TcpStream,
}

impl Socks5Connection {
    pub async fn new(stream: TcpStream) -> io::Result<Self> {
        Ok(Socks5Connection { stream })
    }

    async fn handshake(&mut self) -> io::Result<()> {
        let ver = self.stream.read_u8().await?;
        if ver != 0x05 {
            // 不是SOCKS5协议
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("非SOCKS5协议: {}", ver),
            ));
        }
        let nmethods = self.stream.read_u8().await?;
        let mut methods = vec![0u8; nmethods as usize];
        self.stream.read_exact(&mut methods).await?;
        // methods 中是否包含 0x00，即无需认证方式
        if !methods.contains(&0x00) {
            // 不支持无需认证的方式 拒绝
            self.stream.write_all(&[0x05, 0xFF]).await?;
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("不支持的认证方式:METHODS:{:?}", methods),
            ));
        }
        // 发送无需认证的方式
        self.stream.write_all(&[0x05, 0x00]).await?;
        Ok(())
    }

    async fn parse_request(&mut self) -> io::Result<std::net::SocketAddr> {
        let ver = self.stream.read_u8().await?;
        if ver != 0x05 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("非SOCKS5协议: {}", ver),
            ));
        }
        let cmd = self.stream.read_u8().await?;
        if cmd != 0x01 {
            return Err(io::Error::new(io::ErrorKind::Other, "只支持 CONNECT 请求"));
        }
        let _rsv = self.stream.read_u8().await?;
        let atyp = self.stream.read_u8().await?;
        match atyp {
            0x01 => {
                // IPv4
                let mut buf = [0u8; 4];
                self.stream.read_exact(&mut buf).await?;
                let addr = std::net::Ipv4Addr::from(buf);
                let port = self.stream.read_u16().await?;
                Ok(std::net::SocketAddr::new(addr.into(), port))
            }
            0x03 => {
                // 域名
                let len = self.stream.read_u8().await? as usize;
                let mut buf = vec![0u8; len];
                self.stream.read_exact(&mut buf).await?;
                let addr = String::from_utf8(buf)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "无效的域名格式"))?;
                let port = self.stream.read_u16().await?;
                let mut lookup_result = tokio::net::lookup_host((addr.as_str(), port)).await?;
                let socket_addr = lookup_result
                    .next()
                    .ok_or(io::Error::new(io::ErrorKind::NotFound, "未找到地址"))?;
                Ok(socket_addr)
            }
            0x04 => {
                // IPv6
                let mut buf = [0u8; 16];
                self.stream.read_exact(&mut buf).await?;
                let addr = std::net::Ipv6Addr::from(buf);
                let port = self.stream.read_u16().await?;
                Ok(std::net::SocketAddr::new(addr.into(), port))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "不支持的地址类型",
            )),
        }
    }

    async fn connect_target(&self, target_addr: std::net::SocketAddr) -> io::Result<TcpStream> {
        TcpStream::connect(target_addr).await
    }

    async fn forward_data(&mut self, mut target_stream: TcpStream) -> io::Result<()> {
        let copy_result = tokio::io::copy_bidirectional(&mut self.stream, &mut target_stream).await;
        match copy_result {
            Ok((n1, n2)) => {
                info!("数据传输完成, 客户端->目标: {}, 目标->客户端: {}", n1, n2);
                Ok(())
            }
            Err(e) => {
                info!("数据传输失败: {}", e);
                Err(e)
            }
        }
    }

    pub async fn serve(&mut self) -> io::Result<()> {
        self.handshake().await?;
        let addr = self.parse_request().await?;
        let target_stream = self.connect_target(addr).await?;
        // 发送连接成功响应
        self.stream
            .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await?;
        self.forward_data(target_stream).await
    }
}

struct HttpHandler {
    client_connection: TcpStream,
    target_connection: Option<TcpStream>,
}

impl ProtocolHandler for HttpHandler {
    // 实现方法
}

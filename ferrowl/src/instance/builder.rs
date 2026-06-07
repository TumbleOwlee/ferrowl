use ferrowl_net::{KeyParams, tcp, rtu};

pub enum Builder<T: KeyParams> {
    TcpClient(tcp::ClientBuilder<T>),
    TcpServer(tcp::ServerBuilder<T>),
    RtuClient(rtu::ClientBuilder<T>),
    RtuServer(rtu::ServerBuilder<T>),
}

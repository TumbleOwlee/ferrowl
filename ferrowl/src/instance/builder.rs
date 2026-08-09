//! Transport/role-specific builder held by an [`Instance`](crate::instance::Instance).

use ferrowl_modbus::{KeyParams, ascii, ascii_over_tcp, rtu, rtu_over_tcp, tcp, udp};

/// The underlying ferrowl-modbus builder for each transport/role combination.
pub enum Builder<T: KeyParams> {
    TcpClient(tcp::ClientBuilder<T>),
    TcpServer(tcp::ServerBuilder<T>),
    RtuClient(rtu::ClientBuilder<T>),
    RtuServer(rtu::ServerBuilder<T>),
    RtuOverTcpClient(rtu_over_tcp::ClientBuilder<T>),
    RtuOverTcpServer(rtu_over_tcp::ServerBuilder<T>),
    UdpClient(udp::ClientBuilder<T>),
    UdpServer(udp::ServerBuilder<T>),
    AsciiClient(ascii::ClientBuilder<T>),
    AsciiServer(ascii::ServerBuilder<T>),
    AsciiOverTcpClient(ascii_over_tcp::ClientBuilder<T>),
    AsciiOverTcpServer(ascii_over_tcp::ServerBuilder<T>),
}

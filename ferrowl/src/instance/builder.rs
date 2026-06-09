//! Transport/role-specific builder held by an [`Instance`](crate::instance::Instance).

use ferrowl_net::{KeyParams, rtu, tcp};

/// The underlying ferrowl-net builder for each transport/role combination.
pub enum Builder<T: KeyParams> {
    TcpClient(tcp::ClientBuilder<T>),
    TcpServer(tcp::ServerBuilder<T>),
    RtuClient(rtu::ClientBuilder<T>),
    RtuServer(rtu::ServerBuilder<T>),
}

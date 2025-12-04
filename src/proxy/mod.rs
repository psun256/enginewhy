mod tcp_proxy;

use std::net::SocketAddr;
use std::time::Instant;

// owned and accessed by only one thread.
pub struct ConnectionContext {
    pub connection_id: u64,
    pub client_addr: SocketAddr,
    pub start_time: Instant,
    pub backend_addr: Option<SocketAddr>,
    pub bytes_transferred: usize,
    // pub protocol: String,
    // pub sticky_id: Option<String>,
}

impl ConnectionContext {
    pub fn new(connection_id: u64, client_addr: SocketAddr) -> Self {
        Self {
            connection_id: connection_id,
            client_addr: client_addr,
            start_time: Instant::now(),
            backend_addr: Default::default(),
            bytes_transferred: 0,
        }
    }
}
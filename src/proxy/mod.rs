use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use crate::backend::Backend;

pub mod tcp;

pub struct ConnectionContext {
    pub id: u64,
    pub client_addr: SocketAddr,
    pub start_time: Instant,
    pub backend: Arc<Backend>,
    pub bytes_transferred: u64,
}

impl ConnectionContext {
    pub fn new(id: u64, client_addr: SocketAddr, backend: Arc<Backend>) -> Self {
        backend.inc_connections();

        Self {
            id,
            client_addr,
            start_time: Instant::now(),
            backend,
            bytes_transferred: 0,
        }
    }
}

impl Drop for ConnectionContext {
    fn drop(&mut self) {
        self.backend.dec_connections();
        let duration = self.start_time.elapsed();

        println!("info: conn_id={} closed. client={} backend={} bytes={} duration={:.2?}",
            self.id,
            self.client_addr,
            self.backend.address,
            self.bytes_transferred,
            duration.as_secs_f64()
        );
    }
}
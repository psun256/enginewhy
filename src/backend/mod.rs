use core::fmt;
use std::net::SocketAddr;
use std::sync::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
pub struct Backend {
    pub id: String,
    pub address: SocketAddr,
    pub active_connections: AtomicUsize,
}

impl Backend {
    pub fn new(id: String, address: SocketAddr) -> Self {
        Self {
            id: id.to_string(),
            address,
            active_connections: AtomicUsize::new(0),
        }
    }

    // Ordering::Relaxed means the ops could be in any order, but since this
    // is just a metric, and we assume the underlying system is sane
    // enough not to behave poorly, so SeqCst is probably overkill.
    pub fn inc_connections(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
        println!("{} has {} connections open", self.id, self.active_connections.load(Ordering::Relaxed));
    }

    pub fn dec_connections(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
        println!("{} has {} connections open", self.id, self.active_connections.load(Ordering::Relaxed));
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.address, self.id)
    }
}

#[derive(Clone, Debug)]
pub struct BackendPool {
    pub backends: Arc<RwLock<Vec<Arc<Backend>>>>,
}

impl BackendPool {
    pub fn new() -> Self {
        BackendPool {
            backends: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn add(&self, backend: Backend) {
        self.backends.write().unwrap().push(Arc::new(backend));
    }
}
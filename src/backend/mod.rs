pub mod health;

use crate::backend::health::ServerMetrics;
use core::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};

// A possible endpoint for a proxied connection.
// Note that multiple may live on the same server, hence the Arc<RwLock<ServerMetric>>
#[derive(Debug)]
pub struct Backend {
    pub id: String,
    pub address: SocketAddr,
    pub active_connections: AtomicUsize,
    pub metrics: Arc<RwLock<ServerMetrics>>,
}

impl Backend {
    pub fn new(
        id: String,
        address: SocketAddr,
        server_metrics: Arc<RwLock<ServerMetrics>>,
    ) -> Self {
        Self {
            id: id.to_string(),
            address,
            active_connections: AtomicUsize::new(0),
            metrics: server_metrics,
        }
    }

    // Ordering::Relaxed means the ops could be in any order, but since this
    // is just a metric, and we assume the underlying system is sane
    // enough not to behave poorly, so SeqCst is probably overkill.
    pub fn inc_connections(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
        println!(
            "{} has {} connections open",
            self.id,
            self.active_connections.load(Ordering::Relaxed)
        );
    }

    pub fn dec_connections(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
        println!(
            "{} has {} connections open",
            self.id,
            self.active_connections.load(Ordering::Relaxed)
        );
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.address, self.id)
    }
}

// A set of endpoints that can be load balanced around.
// Each Balancer owns one of these. Backend instances may be shared
// with other Balancer instances, hence Arc<Backend>.
#[derive(Clone, Debug)]
pub struct BackendPool {
    pub backends: Arc<Vec<Arc<Backend>>>,
}

impl BackendPool {
    pub fn new(backends: Vec<Arc<Backend>>) -> Self {
        BackendPool {
            backends: Arc::new(backends),
        }
    }
}

use crate::backend::{Backend, BackendPool};
use crate::balancer::{Balancer, ConnectionInfo};
use std::fmt::Debug;
use std::sync::Arc;

// only the main thread for receiving connections should be
// doing the load balancing. alternatively, each thread
// that handles load balancing should get their own instance.
#[derive(Debug)]
pub struct RoundRobinBalancer {
    pool: BackendPool,
    index: usize,
}

impl RoundRobinBalancer {
    pub fn new(pool: BackendPool) -> RoundRobinBalancer {
        Self { pool, index: 0 }
    }
}

impl Balancer for RoundRobinBalancer {
    fn choose_backend(&mut self, _ctx: ConnectionInfo) -> Option<Arc<Backend>> {
        if self.pool.backends.is_empty() {
            return None;
        }

        let backend = self.pool.backends[self.index % self.pool.backends.len()].clone();
        self.index = self.index.wrapping_add(1);
        Some(backend)
    }
}

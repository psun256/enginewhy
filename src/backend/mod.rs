use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct BackendPool {
    pub backends: Arc<RwLock<HashMap<String, Arc<Backend>>>>,
}

#[derive(Debug)]
pub struct Backend {
    pub id: String,
    pub address: SocketAddr,
    pub is_healthy: AtomicBool,    // no clue how this should work, for now
    pub current_load: AtomicUsize, // no clue how this should work, for now
}

impl BackendPool {
    pub fn new(initial_backends: Vec<Arc<Backend>>) -> Self {
        let mut map = HashMap::new();
        for backend in initial_backends {
            map.insert(backend.id.clone(), backend);
        }

        Self {
            backends: Arc::new(RwLock::new(map)),
        }
    }

    pub fn add_backend(&self, backend: Arc<Backend>) {
        let mut backends_guard = self.backends
            .write()
            .expect("BackendPool lock poisoned");
        // let backends_guard = self.backends.read().unwrap_or_else(|poisoned| poisoned.into_inner());
        backends_guard.insert(backend.id.clone(), backend);
    }

    pub fn get_backend(&self, id: &str) -> Option<Arc<Backend>> {
        let backends_guard = self.backends
            .read()
            .expect("BackendPool lock poisoned");
        // let backends_guard = self.backends.read().unwrap_or_else(|poisoned| poisoned.into_inner());
        backends_guard.get(id).cloned()
    }

    pub fn bruh_amogus_sus(&self) {
        for k in self.backends.read().unwrap().keys() {
            self.backends.write().unwrap().get(k).unwrap().increment_current_load();
        }
    }
}

impl Backend {
    pub fn new(id: String, address: SocketAddr) -> Self {
        Self {
            id: id,
            address: address,
            is_healthy: AtomicBool::new(false),
            current_load: AtomicUsize::new(0),
        }
    }

    pub fn increment_current_load(&self) {
        self.current_load.fetch_add(1, Ordering::SeqCst);
    }

    pub fn decrement_current_load(&self) {
        self.current_load.fetch_sub(1, Ordering::SeqCst);
    }
}
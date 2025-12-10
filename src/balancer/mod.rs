pub mod round_robin;
pub mod adaptive_weight;
pub mod least_connections;
pub mod ip_hashing;

use std::fmt::Debug;
use std::sync::Arc;
use crate::backend::Backend;
use std::cell::RefCell;
use std::net::{SocketAddr};

thread_local! {
    pub static CURRENT_CONNECTION_INFO: RefCell<Option<ConnectionInfo>> = RefCell::new(None);
}

#[derive(Clone, Debug)] 
pub struct ConnectionInfo {
    pub client_ip : SocketAddr,
}

pub trait Balancer: Debug + Send + Sync + 'static {
    fn choose_backend(&mut self) -> Option<Arc<Backend>>;
}


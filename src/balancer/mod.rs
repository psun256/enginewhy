pub mod round_robin;
pub mod adaptive_weight;
pub mod least_connections;
pub mod ip_hashing;

use std::fmt::Debug;
use std::sync::Arc;
use crate::backend::Backend;

pub trait Balancer: Debug + Send + Sync + 'static {
    fn choose_backend(&mut self) -> Option<Arc<Backend>>;
}

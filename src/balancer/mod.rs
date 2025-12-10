pub mod adaptive_weight;
pub mod ip_hashing;
pub mod least_connections;
pub mod round_robin;

use crate::backend::Backend;
use std::fmt::Debug;
use std::net::IpAddr;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ConnectionInfo {
    pub client_ip: IpAddr,
}

pub trait Balancer: Debug + Send + Sync + 'static {
    fn choose_backend(&mut self, ctx: ConnectionInfo) -> Option<Arc<Backend>>;
}

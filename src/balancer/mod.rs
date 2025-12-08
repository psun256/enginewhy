pub mod round_robin;

use std::fmt::Debug;
use std::sync::Arc;
use crate::backend::Backend;

pub trait Balancer: Debug + Send + Sync + 'static {
    fn choose_backend(&mut self) -> Option<Arc<Backend>>;
}
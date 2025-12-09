extern crate core;

mod balancer;
mod config;
mod backend;
mod proxy;

use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicU64, Ordering};
use crate::backend::{Backend, BackendPool, ServerHealth};
use crate::balancer::Balancer;
use crate::balancer::round_robin::RoundRobinBalancer;
use crate::proxy::tcp::proxy_tcp_connection;

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut pool: Vec<Arc<Backend>> = Vec::new();
    let server_metric = Arc::new(RwLock::new(ServerHealth::default()));
    
    pool.push(Arc::new(Backend::new(
        "backend 1".into(),
        "127.0.0.1:8081".parse().unwrap(),
        server_metric.clone()
    )));

    pool.push(Arc::new(Backend::new(
        "backend 2".into(),
        "127.0.0.1:8082".parse().unwrap(),
        server_metric.clone()
    )));

    let mut balancer = RoundRobinBalancer::new(BackendPool::new(pool));

    let listener = TcpListener::bind("127.0.0.1:8080").await?;

    loop {
        let (socket, _) = listener.accept().await?;

        let conn_id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);

        if let Some(backend) = balancer.choose_backend() {
            tokio::spawn(async move {
                if let Err(e) = proxy_tcp_connection(conn_id, socket, backend).await {
                    eprintln!("error: conn_id={} proxy failed: {}", conn_id, e);
                }
            });
        } else {
            eprintln!("error: no backendsd for conn_id={}", conn_id);
        }
    }
}

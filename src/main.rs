extern crate core;

mod balancer;
mod config;
mod backend;
mod proxy;

use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use crate::backend::{Backend, BackendPool};
use crate::balancer::Balancer;
use crate::balancer::round_robin::RoundRobinBalancer;
use crate::proxy::tcp::proxy_tcp_connection;

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = BackendPool::new();

    pool.add(Backend::new(
        "backend 1".into(),
        "127.0.0.1:8081".parse().unwrap(),
    ));

    pool.add(Backend::new(
        "backend 2".into(),
        "127.0.0.1:8082".parse().unwrap(),
    ));

    let mut balancer = RoundRobinBalancer::new(pool.clone());

    let listener = TcpListener::bind("127.0.0.1:8080").await?;

    loop {
        let (socket, _) = listener.accept().await?;

        let conn_id = NEXT_CONN_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

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

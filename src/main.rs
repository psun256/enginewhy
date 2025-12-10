mod balancer;
mod config;
mod backend;
mod proxy;

use std::fs::File;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::net::TcpListener;
use crate::backend::{Backend, BackendPool, ServerHealth};
use crate::balancer::{Balancer, CURRENT_CONNECTION_INFO, ConnectionInfo};
use crate::balancer::round_robin::RoundRobinBalancer;
use crate::balancer::ip_hashing::SourceIPHash;
use crate::proxy::tcp::proxy_tcp_connection;

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let f = File::open("config.yaml").expect("couldn't open config.yaml");
    let app_config: config::AppConfig = serde_saphyr::from_reader(f)?;

    println!("Loaded {} backends, {} rules.",
             app_config.backends.len(),
             app_config.rules.len()
    );

    let listeners = config::loader::build_lb(app_config);

    if listeners.is_empty() {
        eprintln!("its a lawless land");
        return Ok(());
    }

    let mut handles = Vec::new();

    for (port, mut routing_table) in listeners {
        handles.push(tokio::spawn(async move {
            let addr = format!("0.0.0.0:{}", port);
            println!("Starting tcp listener on {}", addr);

            let listener = TcpListener::bind(&addr).await.expect("Failed to bind port");

            loop {
                let (socket, remote_addr) = match listener.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("error: listener port {}: {}", port, e);
                        continue;
                    }
                };

                let remote_ip = remote_addr.ip();
                let conn_id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
                let client_ip = socket.local_addr()?;
        
                CURRENT_CONNECTION_INFO.with(|info| {
                    *info.borrow_mut() = Some(ConnectionInfo { client_ip : client_ip });
                });

                let mut chosen_backend = None;

                for (cidr, balancer_idx) in &mut routing_table.entries {
                    if cidr.contains(&remote_ip) {
                        let balancer = &mut routing_table.balancers[*balancer_idx];
                        chosen_backend = balancer.choose_backend();
                        break;
                    }
                }

                if let Some(backend) = chosen_backend {
                    tokio::spawn(async move {
                        if let Err(e) = proxy_tcp_connection(conn_id, socket, backend).await {
                            eprintln!("error: conn_id={} proxy failed: {}", conn_id, e);
                        }
                    });
                } else {
                    println!("error: no matching rule for {} on port {}", remote_ip, port);
                }

                // clear the slot after use to avoid stale data
                CURRENT_CONNECTION_INFO.with(|info| {
                    *info.borrow_mut() = None;
                });
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    Ok(())
}

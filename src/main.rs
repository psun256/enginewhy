mod backend;
mod balancer;
mod config;
mod proxy;

use crate::balancer::{Balancer, ConnectionInfo};
use crate::proxy::tcp::proxy_tcp_connection;
use std::fs::File;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncBufReadExt, AsyncReadExt};
use serde_json::Value;
use std::collections::HashMap;
use std::net::{IpAddr};
use std::sync::{Arc, RwLock};
use crate::backend::health::ServerMetrics;
use rperf3::{Server, Config};
use std::io::Read;
use std::io::{BufRead, BufReader};

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

async fn start_iperf_server() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::server(5001);
    let server = Server::new(config);
    server.run().await?;
    Ok(())
}

async fn handle_metrics_stream(stream: TcpStream, healths: &HashMap<IpAddr, Arc<RwLock<ServerMetrics>>>) -> std::io::Result<()> {
    let server_ip = stream.peer_addr()?.ip();
    let mut reader = tokio::io::BufReader::new(stream);
    let mut line = String::new();

    loop {
        line.clear();
        
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                if let Err(e) = process_metrics(server_ip, &line, healths) {
                    eprintln!("skipping invalid packet: {}", e);
                }
            }
            Err(e) => {
                eprintln!("connection error: {}", e);
                break;
            }
        }
    }
    Ok(())
}

fn process_metrics(server_ip: IpAddr, json_str: &str, healths: &HashMap<IpAddr, Arc<RwLock<ServerMetrics>>>) -> Result<(), String> {
    let parsed: Value = serde_json::from_str(json_str)
        .map_err(|e| format!("parse error: {}", e))?;

    let metrics_lock = healths.get(&server_ip)
        .ok_or_else(|| format!("unknown server: {}", server_ip))?;

    let get_f64 = |key: &str| -> Result<f64, String> {
        parsed.get(key)
            .and_then(|v| v.as_f64())
            .ok_or_else(|| format!("invalid '{}'", key))
    };

    if let Ok(mut guard) = metrics_lock.write() {
        guard.update(
            get_f64("cpu")?,
            get_f64("mem")?,
            get_f64("net")?,
            get_f64("io")?,
        );
    }

    Ok(())
}

async fn start_healthcheck_listener(addr: &str, healths: HashMap<IpAddr, Arc<RwLock<ServerMetrics>>>) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    println!("TCP server listening on {}", addr);
    loop {
        let (stream, remote_addr) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                continue;
            }
        };

        if let Err(e) = handle_metrics_stream(stream, &healths).await {
            eprintln!("connection handler error: {}", e);
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let f = File::open("config.yaml").expect("couldn't open config.yaml");
    let app_config: config::AppConfig = serde_saphyr::from_reader(f)?;

    println!(
        "Loaded {} backends, {} rules.",
        app_config.backends.len(),
        app_config.rules.len()
    );

    let (listeners, healths) = config::loader::build_lb(app_config);

    if listeners.is_empty() {
        eprintln!("its a lawless land");
        return Ok(());
    }

    let mut handles = Vec::new();

    handles.push(
        tokio::spawn(async {
            start_healthcheck_listener("0.0.0.0:8080", healths).await.unwrap();
        })
    );

    handles.push(
        tokio::spawn(async {
            start_iperf_server().await;
        })
    );

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

                let mut chosen_backend = None;

                for (cidr, balancer_idx) in &mut routing_table.entries {
                    if cidr.contains(&remote_ip) {
                        let balancer = &mut routing_table.balancers[*balancer_idx];
                        chosen_backend = balancer.choose_backend(ConnectionInfo {
                            client_ip: remote_ip,
                        });
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
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    Ok(())
}

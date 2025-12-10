mod backend;
mod balancer;
mod config;
mod proxy;

use crate::balancer::{Balancer, ConnectionInfo};
use crate::proxy::tcp::proxy_tcp_connection;
use std::fs::File;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::AsyncReadExt;
use serde_json::Value;
use std::collections::HashMap;
use std::net::{IpAddr};
use std::sync::{Arc, RwLock};
use crate::backend::health::ServerMetrics;
use rperf3::{Server, Config};
use std::io::Read;


static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

async fn start_iperf_server() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::server(5001);
    let server = Server::new(config);
    server.run().await?;
    Ok(())
}

async fn handle_connection(mut stream: TcpStream, healths: &HashMap<IpAddr, Arc<RwLock<ServerMetrics>>>) -> std::io::Result<()> {
    loop {
        let mut buffer = [0u8; 512];
        let bytes_read = stream.read(&mut buffer).await?; 
        let receiving_data = String::from_utf8((buffer[..bytes_read]).to_vec())
                                    .unwrap();
        
        let parsed_recdata : Value = serde_json::from_str(&receiving_data).unwrap();
        let server_ip: IpAddr = parsed_recdata["server_ip"]
                                .as_str()
                                .unwrap()
                                .parse()
                                .unwrap();

        healths.get(&server_ip)
               .unwrap()
               .write()
               .unwrap()
               .update(    
                    parsed_recdata["cpu"].as_f64().unwrap(),
                    parsed_recdata["mem"].as_f64().unwrap(),
                    parsed_recdata["net"].as_f64().unwrap(),
                    parsed_recdata["io"].as_f64().unwrap(),
                );
    }
    #[warn(unreachable_code)]
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

        if let Err(e) = handle_connection(stream, &healths).await {
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
            start_healthcheck_listener("127.0.0.1:8080", healths).await.unwrap();
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

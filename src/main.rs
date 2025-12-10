mod backend;
mod balancer;
mod config;
mod proxy;

use std::collections::HashMap;
use crate::balancer::{ConnectionInfo};
use crate::proxy::tcp::proxy_tcp_connection;
use std::fs::File;
use std::path::PathBuf;
use std::net::IpAddr;
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
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use anywho::Error;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use crate::backend::ServerMetrics;
use crate::config::loader::{build_lb, RoutingTable};

use notify::{Watcher, RecursiveMode, Event};
use clap::Parser;

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

struct ProgramState {
    tx_rt_map: HashMap<u16, mpsc::UnboundedSender<RoutingTable>>,
    healths: HashMap<IpAddr, Arc<RwLock<ServerMetrics>>>,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if !args.config.is_file() {
        eprintln!("config file not found or not accessible");
        std::process::exit(1);
    }

    println!("reading config from {:?}", args.config);

    let state = Arc::new(Mutex::new(ProgramState {
        tx_rt_map: HashMap::new(),
        healths: HashMap::new(),
    }));

    if let Err(e) = load_config(&args.config, state.clone()).await {
        eprintln!("config file loading failed: {}", e);
    }

    let config_path = args.config.clone();
    let state_clone = state.clone();
  
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

    tokio::spawn(async move {
        let (tx, mut rx) = mpsc::channel(1);
      
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if event.kind.is_modify() {
                    let _ = tx.blocking_send(());
                }
            }
        }).unwrap();

        watcher.watch(&config_path, RecursiveMode::NonRecursive).unwrap();
        println!("watching for changes to {:?}", config_path);

        while rx.recv().await.is_some() {
            if let Err(e) = load_config(&config_path, state_clone.clone()).await {
                eprintln!("loading config failed: {}", e);
            }
        }
    });

    loop { tokio::time::sleep(Duration::from_hours(1)).await; }
}

async fn load_config(path: &PathBuf, state: Arc<Mutex<ProgramState>>) -> Result<(), Error> {
    let f = File::open(path)?;
    let app_config: config::AppConfig = match serde_saphyr::from_reader(f) {
        Ok(app_config) => app_config,
        Err(e) => { eprintln!("error parsing config {}", e); return Ok(()); }
    };

    println!(
        "Loaded config, with {} backends, {} rules.",
        app_config.backends.len(),
        app_config.rules.len()
    );

    let (mut listeners, health_monitors) = match build_lb(app_config) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("config has logical errors: {}", e);
            return Ok(());
        }
    };
    let mut prog_state = state.lock().unwrap();

    let ports_to_remove: Vec<u16> = prog_state.tx_rt_map
        .keys()
        .cloned()
        .filter(|port| !listeners.contains_key(port))
        .collect();

    for port in ports_to_remove {
        prog_state.tx_rt_map.remove(&port);
    }

    prog_state.healths = health_monitors;
    for (port, routing_table) in listeners.drain() {
        if let Some(x) = prog_state.tx_rt_map.get_mut(&port) {
            x.send(routing_table)?;
            println!("updated rules on port {}", port);
        } else {
            let (tx_rt, rx_rt) = mpsc::unbounded_channel();
            prog_state.tx_rt_map.insert(port, tx_rt);

            tokio::spawn(run_listener(port, rx_rt, routing_table));
        }
    }

    println!("reload complete");
    Ok(())
}

async fn run_listener(
    port: u16,
    mut rx_rt: mpsc::UnboundedReceiver<RoutingTable>,
    mut current_table: RoutingTable
) {
    let addr = format!("0.0.0.0:{}", port);
    println!("Starting tcp listener on {}", addr);

    let listener = TcpListener::bind(&addr).await.expect("Failed to bind port");

    loop {
        tokio::select! {
            msg = rx_rt.recv() => {
                match msg {
                    Some(new_table) => {
                        current_table = new_table;
                    }
                    None => {
                        println!("Unbinding listener on port {}", port);
                        break;
                    }
                }
            }
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((socket, remote_addr)) => {
                        let remote_ip = remote_addr.ip();
                        let conn_id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);

                        let mut chosen_backend = None;

                        for (cidr, balancer_idx) in &mut current_table.entries {
                            if cidr.contains(&remote_ip) {
                                let balancer = &mut current_table.balancers[*balancer_idx];
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
                    Err(e) => {
                        eprintln!("error: listener port {}: {}", port, e);
                    continue;
                    }
                }
            }
        }
    }
}

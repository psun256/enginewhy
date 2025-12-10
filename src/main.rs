mod backend;
mod balancer;
mod config;
mod proxy;

use crate::backend::health::{start_healthcheck_listener, start_iperf_server, ServerMetrics};
use crate::balancer::ConnectionInfo;
use crate::config::loader::{build_lb, RoutingTable};
use crate::proxy::tcp::proxy_tcp_connection;
use anywho::Error;
use std::collections::HashMap;
use std::fs::File;
use std::hash::Hash;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use clap::Parser;
use notify::{Event, RecursiveMode, Watcher};
use std::cmp;

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

struct ProgramState {
    tx_rt_map: HashMap<u16, mpsc::UnboundedSender<RoutingTable>>,
    healths: HashMap<IpAddr, Arc<RwLock<ServerMetrics>>>,
    health_listener: Option<tokio::task::JoinHandle<()>>,
    iperf_server: Option<tokio::task::JoinHandle<()>>,
    health_listener_addr: Option<String>,
    iperf_server_addr: Option<String>,
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
        health_listener: None,
        iperf_server: None,
        health_listener_addr: None,
        iperf_server_addr: None,
    }));

    if let Err(e) = load_config(&args.config, state.clone()).await {
        eprintln!("config file loading failed: {}", e);
    }

    let config_path = args.config.clone();
    let state_clone = state.clone();

    tokio::spawn(async move {
        let (tx, mut rx) = mpsc::channel(1);

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if event.kind.is_modify() {
                    let _ = tx.blocking_send(());
                }
            }
        })
        .unwrap();

        watcher
            .watch(&config_path, RecursiveMode::NonRecursive)
            .unwrap();
        println!("watching for changes to {:?}", config_path);

        while rx.recv().await.is_some() {
            // for some reason, saving on certain text editors fires several events,
            // and this causes us to reload a lot. try to flush some events, add a tiny delay
            // to mitigate this

            while rx.try_recv().is_ok() {}
            tokio::time::sleep(Duration::from_millis(50)).await;
            while rx.try_recv().is_ok() {}

            if let Err(e) = load_config(&config_path, state_clone.clone()).await {
                eprintln!("loading config failed: {}", e);
            }
        }
    });

    loop {
        tokio::time::sleep(Duration::from_hours(1)).await;
    }
}

async fn load_config(path: &PathBuf, state: Arc<Mutex<ProgramState>>) -> Result<(), Error> {
    let f = File::open(path)?;
    let app_config: config::AppConfig = match serde_saphyr::from_reader(f) {
        Ok(app_config) => app_config,
        Err(e) => {
            eprintln!("error parsing config {}", e);
            return Ok(());
        }
    };

    println!(
        "Loaded config, with {} backends, {} rules.",
        app_config.backends.len(),
        app_config.rules.len()
    );

    let (mut listeners, health_monitors) = match build_lb(&app_config) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("config has logical errors: {}", e);
            return Ok(());
        }
    };
    let mut prog_state = state.lock().unwrap();

    let ports_to_remove: Vec<u16> = prog_state
        .tx_rt_map
        .keys()
        .cloned()
        .filter(|port| !listeners.contains_key(port))
        .collect();

    for port in ports_to_remove {
        prog_state.tx_rt_map.remove(&port);
    }

    if let Some(handle) = prog_state.health_listener.take() {
        handle.abort();
    }
    let health_map: HashMap<IpAddr, Arc<RwLock<ServerMetrics>>> = health_monitors.clone();
    let health_addr = app_config.healthcheck_addr.clone();
    let health_addr_c = health_addr.clone();
    let health_handle = tokio::spawn(async move {
        if let Err(e) = start_healthcheck_listener(&health_addr, health_map).await {
            eprintln!("health check listener failed: {}", e);
        }
    });
    prog_state.health_listener = Some(health_handle);
    prog_state.health_listener_addr = Some(health_addr_c);

    // maybe restart iperf server
    let iperf_addr = app_config.iperf_addr.clone();
    if prog_state.iperf_server_addr.as_ref() != Some(&iperf_addr) {
        if let Some(handle) = prog_state.iperf_server.take() {
            handle.abort();
        }

        let iperf_addr_c = iperf_addr.clone();
        let iperf_handle = tokio::spawn(async move {
            if let Err(e) = start_iperf_server(iperf_addr.as_str()).await {
                eprintln!("iperf server failed: {}", e);
            }
        });

        prog_state.iperf_server = Some(iperf_handle);
        prog_state.iperf_server_addr = Some(iperf_addr_c);
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
    mut current_table: RoutingTable,
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

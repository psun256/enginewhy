use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock};
use serde_json::Value;
use rperf3::{Config, Server};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::io::AsyncBufReadExt;

// Physical server health statistics, used for certain load balancing algorithms
#[derive(Debug, Default)]
pub struct ServerMetrics {
    pub cpu: f64,
    pub mem: f64,
    pub net: f64,
    pub io: f64,
}

impl ServerMetrics {
    pub fn update(&mut self, cpu: f64, mem: f64, net: f64, io: f64) {
        self.cpu = cpu;
        self.mem = mem;
        self.net = net;
        self.io = io;
    }
}

pub async fn start_healthcheck_listener(
    addr: &str,
    healths: HashMap<IpAddr, Arc<RwLock<ServerMetrics>>>,
) -> std::io::Result<()> {
    let addrs = tokio::net::lookup_host(addr).await?;
    let mut listener = None;

    for a in addrs {
        let socket = match a {
            SocketAddr::V4(_) => TcpSocket::new_v4()?,
            SocketAddr::V6(_) => TcpSocket::new_v6()?,
        };

        socket.set_reuseaddr(true)?;

        if socket.bind(a).is_ok() {
            listener = Some(socket.listen(1024)?);
            break;
        }
    }

    let listener = listener.ok_or_else(|| {
        eprintln!("health listener could not bind to port");
        std::io::Error::new(std::io::ErrorKind::Other, "health listener failed")
    })?;

    println!("healthcheck server listening on {}", addr);
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
}

pub async fn start_iperf_server(addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let sock = addr.parse::<SocketAddr>()?;
    let mut config = Config::server(sock.port());
    config.bind_addr = Some(sock.ip());
    let server = Server::new(config);
    println!("iperf server listening on {}", addr);
    server.run().await?;
    Ok(())
}

async fn handle_metrics_stream(
    stream: TcpStream,
    healths: &HashMap<IpAddr, Arc<RwLock<ServerMetrics>>>,
) -> std::io::Result<()> {
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

fn process_metrics(
    server_ip: IpAddr,
    json_str: &str,
    healths: &HashMap<IpAddr, Arc<RwLock<ServerMetrics>>>,
) -> Result<(), String> {
    let parsed: Value =
        serde_json::from_str(json_str).map_err(|e| format!("parse error: {}", e))?;

    let metrics_lock = healths
        .get(&server_ip)
        .ok_or_else(|| format!("unknown server: {}", server_ip))?;

    let get_f64 = |key: &str| -> Result<f64, String> {
        parsed
            .get(key)
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
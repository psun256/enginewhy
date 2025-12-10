use rperf3::{Server, Config};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::io::{Read, Write};
use std::env;
use tokio::task;

async fn start_iperf_server() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::server(5001);
    let server = Server::new(config);
    server.run().await?;
    Ok(())
}

fn handle_connection(mut stream: TcpStream) -> std::io::Result<()> {
    loop {
        let mut buffer = [0u8; 512];
        let bytes_read = stream.read(&mut buffer)?; 
        let received = String::from_utf8_lossy(&buffer[..bytes_read]);
        println!("Received: {}", received);
    }
    Ok(())
}

fn start_tcp_server(addr: &str) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    println!("TCP server listening on {}", addr);
    let mut handles = Vec::new();
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let handle = thread::spawn(move || {
                    if let Err(e) = handle_connection(stream) {
                        eprintln!("connection handler error: {}", e);
                    }
                });
                handles.push(handle);
            }
            Err(e) => eprintln!("incoming connection failed: {}", e),
        }
    }

    // When the incoming stream iterator ends (listener closed), join all handlers.
    for h in handles {
        let _ = h.join();
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    // Choose IP based on `--localhost` flag for debugging
    let use_localhost = env::args().any(|a| a == "--localhost");
    let ip = if use_localhost { "127.0.0.1" } else { "192.67.67.67" };
    let tcp_addr = format!("{}:8080", ip);

    let iperf_server = task::spawn(async {
        start_iperf_server().await;
    });

    let tcp_ip = tcp_addr.clone();
    let tcp_server = thread::spawn(move || {
        start_tcp_server(&tcp_ip).unwrap();
    });

    iperf_server.await.unwrap();
    tcp_server.join().unwrap();
}

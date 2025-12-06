use std::fmt;
use tokio::io;
use tokio::net::{TcpListener, TcpStream};

use std::error::Error;

#[derive(Clone, Debug)]
pub struct Backend {
    address: String,
}

impl Backend {
    pub fn new(address: String) -> Self {
        Backend { address }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.address)
    }
}

pub async fn proxy(client_addr: String, server_addr: String) -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(client_addr.clone()).await?;
    println!("Opened {client_addr} -> {server_addr}");
    loop {
        let server_addr = server_addr.clone();
        let (client, _) = listener.accept().await?;
        tokio::spawn(async move {
            let server = TcpStream::connect(server_addr).await.unwrap();

            let (mut read_client, mut write_client) = client.into_split();
            let (mut read_server, mut write_server) = server.into_split();

            let forward_to_server =
                tokio::spawn(async move { io::copy(&mut read_client, &mut write_server).await });

            let forward_to_client =
                tokio::spawn(async move { io::copy(&mut read_server, &mut write_client).await });

            let _ = tokio::join!(forward_to_server, forward_to_client);
        });
    }
}

pub async fn proxy_connection(
    client_stream: TcpStream,
    backend: &Backend,
) -> Result<(), io::Error> {
    let log_error = |e| {
        eprintln!("error: something went wrong {}", e);
        e
    };

    println!("info: connecting to backend {}", backend);
    let backend_stream = TcpStream::connect(&backend.address)
        .await
        .map_err(log_error)?;
    println!(
        "info: the bluetooth device is connected successfully {}",
        backend
    );

    let (mut client_read, mut client_write) = client_stream.into_split();
    let (mut backend_read, mut backend_write) = backend_stream.into_split();

    let client_to_backend = io::copy(&mut client_read, &mut backend_write);
    let backend_to_client = io::copy(&mut backend_read, &mut client_write);

    let _ = tokio::select! {
        res_a = client_to_backend => res_a.map_err(log_error)?,
        res_b = backend_to_client => res_b.map_err(log_error)?,
    };

    Ok(())
}

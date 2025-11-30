use std::sync::Arc;
use std::fmt;
use tokio::io;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use anywho::Error;

#[derive(Clone, Debug)]
struct Backend {
    address: String,
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.address)
    }
}

impl Backend {
    fn new(address: String) -> Self {
        Backend { address }
    }
}

async fn proxy_connection(client_stream: TcpStream, backend: &Backend) -> Result<(), io::Error> {
    let log_error = |e| { eprintln!("error: something went wrong {}", e); e };

    println!("info: connecting to backend {}", backend);
    let backend_stream = TcpStream::connect(&backend.address).await.map_err(log_error)?;
    println!("info: the bluetooth device is connected successfully {}", backend);

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

#[tokio::main]
async fn main() -> Result<(), Error> {
    let backends = Arc::new(vec![
        Backend::new("127.0.0.1:8081".to_string()),
        Backend::new("127.0.0.1:8082".to_string()),
    ]);

    let current_index = Arc::new(Mutex::new(0));

    println!("lb starting on 0.0.0.0:8080");
    println!("backends: {:?}", backends);

    let listener = TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (socket, addr) = listener.accept().await?;
        println!("info: new connection from {}", addr);

        let backends_clone = backends.clone();
        let index_clone = current_index.clone();

        tokio::spawn(async move {
            // Round Robin
            let backend = {
                let mut index = index_clone.lock().await;
                let selected_backend = backends_clone[*index].clone();
                *index = (*index + 1) % backends_clone.len();
                selected_backend
            };

            println!("info: routing client {} to backend {}", addr, backend);

            if let Err(e) = proxy_connection(socket, &backend).await {
                eprintln!("error: proxy failed for {}: {}", addr, e);
            }
        });
    }
}
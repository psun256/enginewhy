mod netutils;

use anywho::Error;
use netutils::{Backend, proxy, proxy_connection};
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::Mutex;

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

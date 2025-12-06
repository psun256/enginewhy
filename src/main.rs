macro_rules! info {
    ($($arg:tt)*) => {{
        print!("info: ");
        println!($($arg)*);
    }};
}

macro_rules! error {
    ($($arg:tt)*) => {
        eprint!("error: ");
        eprintln!($($arg)*);
    };
}

mod netutils;

use anywho::Error;
use netutils::{Backend, tunnel};
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

    info!("enginewhy starting on 0.0.0.0:8080");
    info!("backends: {:?}", backends);

    let listener = TcpListener::bind("0.0.0.0:8080").await?;

    loop {
        let (client, addr) = listener.accept().await?;
        info!("new connection from {}", addr);

        let backend = {
            let mut index = current_index.lock().await;
            let selected_backend = backends[*index].clone();
            *index = (*index + 1) % backends.len();
            selected_backend
        };

        info!("routing client {} to backend {}", addr, backend);

        if let Err(e) = tunnel(client, backend).await {
            error!("proxy failed for {}: {}", addr, e);
        }
    }
}

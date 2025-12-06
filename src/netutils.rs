use std::fmt;
use tokio::io;
use tokio::net::TcpStream;

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

pub async fn tunnel(client_stream: TcpStream, backend: Backend) -> Result<(), Box<dyn Error>> {
    let backend_address: String = backend.address.clone();
    tokio::spawn(async move {
        let backend_stream: TcpStream = match TcpStream::connect(&backend_address).await {
            Ok(s) => {
                println!("Connected to backend {backend_address}");
                s
            }
            Err(e) => {
                eprintln!("Failed connecting to backend {backend_address}: {e}");
                return;
            }
        };

        let (mut read_client, mut write_client) = client_stream.into_split();
        let (mut read_backend, mut write_backend) = backend_stream.into_split();

        let client_to_backend = tokio::spawn(async move {
            match io::copy(&mut read_client, &mut write_backend)
                .await
                .unwrap()
            {
                n => println!("{n}B ==> backend"),
            }
        });

        let backend_to_client = tokio::spawn(async move {
            match io::copy(&mut read_backend, &mut write_client)
                .await
                .unwrap()
            {
                n => println!("{n}B ==> client"),
            }
        });

        let _ = tokio::join!(client_to_backend, backend_to_client);
    });

    Ok(())
}

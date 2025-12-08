use std::fmt;
use tokio::io;
use tokio::net::TcpStream;

use std::error::Error;

#[derive(Clone, Debug)]
pub struct Backend {
    address: String,
    pub current_load : u32
}

impl Backend {
    pub fn new(address: String) -> Self {
        Backend { 
            address,  
            current_load : 0 
        }
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
                info!("connected to backend {backend_address}");
                s
            }
            Err(e) => {
                error!("failed connecting to backend {backend_address}: {e}");
                return;
            }
        };

        let (mut read_client, mut write_client) = client_stream.into_split();
        let (mut read_backend, mut write_backend) = backend_stream.into_split();

        let client_to_backend =
            tokio::spawn(async move { io::copy(&mut read_client, &mut write_backend).await });

        let backend_to_client =
            tokio::spawn(async move { io::copy(&mut read_backend, &mut write_client).await });

        let _ = tokio::join!(client_to_backend, backend_to_client);
    });

    Ok(())
}

use std::sync::Arc;
use tokio::io;
use tokio::net::TcpStream;
use anywho::Error;
use crate::backend::Backend;
use crate::proxy::ConnectionContext;

pub async fn proxy_tcp_connection(connection_id: u64, mut client_stream: TcpStream, backend: Arc<Backend>) -> Result<(), Error> {
    let client_addr = client_stream.peer_addr()?;

    let mut ctx = ConnectionContext::new(connection_id, client_addr, backend.clone());

    #[cfg(debug_assertions)]
    println!("info: conn_id={} connecting to {}", connection_id, ctx.backend.id);

    let mut backend_stream = TcpStream::connect(&backend.address).await?;

    let (tx, rx) = io::copy_bidirectional(
        &mut client_stream,
        &mut backend_stream,
    ).await?;

    ctx.bytes_transferred = tx + rx;

    Ok(())
}

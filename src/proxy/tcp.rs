use crate::backend::Backend;
use crate::proxy::ConnectionContext;
use anywho::Error;
use std::sync::Arc;
use tokio::io;
use tokio::net::TcpStream;

pub async fn proxy_tcp_connection(
    connection_id: u64,
    mut client_stream: TcpStream,
    backend: Arc<Backend>,
) -> Result<(), Error> {
    let _ = client_stream.set_nodelay(true);
    let client_addr = client_stream.peer_addr()?;

    #[cfg(debug_assertions)]
    println!(
        "info: conn_id={} connecting to {}",
        connection_id, backend.id
    );

    let mut backend_stream = TcpStream::connect(&backend.address).await?;
    let _ = backend_stream.set_nodelay(true);

    let mut ctx = ConnectionContext::new(connection_id, client_addr, backend);

    let (tx, rx) = io::copy_bidirectional(&mut client_stream, &mut backend_stream).await?;

    ctx.bytes_transferred = tx + rx;

    Ok(())
}

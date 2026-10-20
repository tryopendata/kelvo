//! The SSH side of a remote session: stdin and stdout bridged to a local metrics socket.
use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Connects to the metrics socket at `path` and copies stdin to it and its output to
/// stdout, flushing every read, until either side closes. A missing socket is the
/// connect error, so a caller can tell "not running" from a broken session.
pub async fn stdio_bridge(path: &Path) -> std::io::Result<()> {
    let stream = tokio::net::UnixStream::connect(path).await?;
    let (mut read, mut write) = stream.into_split();
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    tokio::select! {
        result=async {tokio::io::copy(&mut input,&mut write).await?;write.shutdown().await}=>{result?;},
        result=async {let mut buffer=[0u8;16*1024];loop{let count=read.read(&mut buffer).await?;if count==0 {break;}output.write_all(&buffer[..count]).await?;output.flush().await?;}Ok::<(),std::io::Error>(())}=>{result?;}
    }
    Ok(())
}

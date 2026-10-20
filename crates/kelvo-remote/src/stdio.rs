//! The SSH side of a remote session: stdin and stdout bridged to a local metrics socket.
use std::path::Path;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Connects to the metrics socket at `path` and copies stdin to it and its output to
/// stdout, flushing every read. Stdin closing half-closes the socket and the bridge
/// keeps relaying until the daemon closes its side, so replies already in flight
/// still arrive; the daemon closing ends the bridge at once. A missing socket is the
/// connect error, so a caller can tell "not running" from a broken session.
pub async fn stdio_bridge(path: &Path) -> std::io::Result<()> {
    let stream = tokio::net::UnixStream::connect(path).await?;
    let (read, write) = stream.into_split();
    bridge(tokio::io::stdin(), tokio::io::stdout(), read, write).await
}

async fn bridge(
    input: impl AsyncRead + Unpin,
    output: impl AsyncWrite + Unpin,
    from_daemon: impl AsyncRead + Unpin,
    to_daemon: impl AsyncWrite + Unpin,
) -> std::io::Result<()> {
    let downstream = relay_output(from_daemon, output);
    tokio::pin!(downstream);
    tokio::select! {
        result = forward_input(input, to_daemon) => {
            result?;
            downstream.await
        }
        result = &mut downstream => result,
    }
}

/// Copies stdin to the daemon, then half-closes the socket.
async fn forward_input(
    mut input: impl AsyncRead + Unpin,
    mut to_daemon: impl AsyncWrite + Unpin,
) -> std::io::Result<()> {
    tokio::io::copy(&mut input, &mut to_daemon).await?;
    to_daemon.shutdown().await
}

/// Copies the daemon's output to stdout until it closes, flushing every read so a
/// frame never waits in a buffer.
async fn relay_output(
    mut from_daemon: impl AsyncRead + Unpin,
    mut output: impl AsyncWrite + Unpin,
) -> std::io::Result<()> {
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let count = from_daemon.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        output.write_all(&buffer[..count]).await?;
        output.flush().await?;
    }
}

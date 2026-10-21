use crate::{RemoteError, Result};
use kelvo_proto::Message;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// A tighter transport envelope than the compatibility codec's historical ceiling.
pub const MAX_WIRE_BYTES: usize = 1024 * 1024;
pub async fn read<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Message> {
    let len = reader.read_u32().await? as usize;
    if len == 0 || len > MAX_WIRE_BYTES {
        return Err(RemoteError::Protocol(format!(
            "frame length {len} exceeds bounds"
        )));
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body).await?;
    Ok(kelvo_proto::decode_message(&body)?)
}
pub async fn write<W: AsyncWrite + Unpin>(writer: &mut W, message: &Message) -> Result<()> {
    write_frame(writer, &kelvo_proto::encode_frame(message)?).await
}
/// Writes a frame already encoded with `kelvo_proto::encode_frame`.
pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_WIRE_BYTES + 4 {
        return Err(RemoteError::Protocol("outbound frame too large".into()));
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        writer.write_all(bytes).await?;
        writer.flush().await
    })
    .await
    .map_err(|_| RemoteError::Transport("peer stopped reading".into()))??;
    Ok(())
}
/// Keep reads in their own task: cancelling read_exact midway would corrupt framing.
pub fn reader_task<R: AsyncRead + Unpin + Send + 'static>(
    mut reader: R,
) -> (
    tokio::sync::mpsc::Receiver<Result<Message>>,
    tokio::task::JoinHandle<()>,
) {
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    let task = tokio::spawn(async move {
        loop {
            let result = read(&mut reader).await;
            let failed = result.is_err();
            if tx.send(result).await.is_err() || failed {
                break;
            }
        }
    });
    (rx, task)
}
pub struct AbortOnDrop(pub tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rejects_oversized_prefix_before_body() {
        let mut bytes = ((MAX_WIRE_BYTES + 1) as u32)
            .to_be_bytes()
            .as_slice()
            .to_owned();
        assert!(matches!(
            read(&mut bytes.as_slice()).await,
            Err(RemoteError::Protocol(_))
        ));
        bytes.clear();
    }
}

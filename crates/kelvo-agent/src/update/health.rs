//! Exercise the same protocol a controller uses, but only on the private trial socket.
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, bail, ensure};
use kelvo_proto::{Hello, Message, Offer, RemoteInterest, SyncRequest};
use kelvo_schema::Tier;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

async fn read(stream: &mut UnixStream) -> anyhow::Result<Message> {
    let length = stream
        .read_u32()
        .await
        .context("reading trial frame length")?;
    ensure!(
        length <= kelvo_proto::MAX_FRAME_LEN,
        "trial frame too large"
    );
    let mut body = vec![0; length as usize];
    stream
        .read_exact(&mut body)
        .await
        .context("reading trial frame")?;
    Ok(kelvo_proto::decode_message(&body)?)
}

async fn write(stream: &mut UnixStream, message: &Message) -> anyhow::Result<()> {
    stream
        .write_all(&kelvo_proto::encode_frame(message)?)
        .await
        .context("writing trial request")
}

pub async fn hello(socket: &Path) -> anyhow::Result<Hello> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = UnixStream::connect(socket)
            .await
            .context("connecting to collector")?;
        match read(&mut stream).await? {
            Message::Hello(hello) => Ok(*hello),
            _ => bail!("collector did not send its identity"),
        }
    })
    .await
    .context("collector identity timed out")?
}

fn verify_protocol(hello: &Hello, signed: kelvo_update::ProtocolRange) -> anyhow::Result<()> {
    ensure!(
        hello.min_compatible == signed.min && hello.proto_version == signed.max,
        "trial protocol range differs from signed release"
    );
    Ok(())
}

pub async fn trial(
    socket: &Path,
    expected: &Hello,
    version: &str,
    protocol: kelvo_update::ProtocolRange,
) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(45), async {
        let mut stream = loop {
            match UnixStream::connect(socket).await {
                Ok(stream) => break stream,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    tokio::time::sleep(Duration::from_millis(100)).await
                }
                Err(e) => return Err(e).context("connecting to trial collector"),
            }
        };
        let Message::Hello(hello) = read(&mut stream).await? else {
            bail!("trial did not identify itself")
        };
        ensure!(
            hello.app_version == version,
            "trial daemon version differs from signed release"
        );
        verify_protocol(&hello, protocol)?;
        ensure!(
            hello.host.id == expected.host.id,
            "trial changed host identity"
        );
        ensure!(
            !hello.db_instance_uuid.is_nil() && hello.db_instance_uuid == expected.db_instance_uuid,
            "trial changed history identity or history is unavailable"
        );
        let negotiated = kelvo_proto::negotiate(
            &Offer::local(kelvo_proto::local_features()),
            &Offer::from(hello.as_ref()),
        )?;
        write(&mut stream, &Message::HelloAck(negotiated.ack())).await?;
        write(
            &mut stream,
            &Message::Interest(RemoteInterest {
                detail: true,
                ..RemoteInterest::default()
            }),
        )
        .await?;
        write(
            &mut stream,
            &Message::SyncRequest(SyncRequest {
                tier: Tier::S10,
                after: None,
                max_rows: 1,
                from_ms: None,
                max_bytes: Some(16384),
            }),
        )
        .await?;
        let mut first = None;
        let mut advancing = false;
        let mut history = false;
        while !advancing || !history {
            match read(&mut stream).await? {
                Message::Live(frame) if !frame.values.is_empty() => {
                    if let Some(ts) = first {
                        advancing |= frame.ts_ms > ts;
                    } else {
                        first = Some(frame.ts_ms);
                    }
                }
                Message::SyncPage(page) => {
                    ensure!(
                        page.epoch == expected.db_instance_uuid,
                        "trial history query returned another epoch"
                    );
                    history = true;
                }
                Message::Error { message, .. } => bail!("trial health query failed: {message}"),
                _ => {}
            }
        }
        Ok(())
    })
    .await
    .context("trial collector did not become healthy within 45 seconds")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_protocol_must_equal_the_executable_range() {
        let Message::Hello(hello) = kelvo_proto::decode_message(include_bytes!(
            "../../../kelvo-proto/tests/fixtures/v1/hello.cbor"
        ))
        .unwrap() else {
            panic!("fixture must be Hello")
        };
        let signed = kelvo_update::ProtocolRange {
            min: hello.min_compatible,
            max: hello.proto_version,
        };
        verify_protocol(&hello, signed).unwrap();
        assert!(
            verify_protocol(
                &hello,
                kelvo_update::ProtocolRange {
                    max: signed.max + 1,
                    ..signed
                }
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn trial_rejects_a_same_version_hello_with_a_narrower_protocol_range() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let socket = dir.path().join("trial.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let Message::Hello(hello) = kelvo_proto::decode_message(include_bytes!(
            "../../../kelvo-proto/tests/fixtures/v1/hello.cbor"
        ))
        .unwrap() else {
            panic!("fixture must be Hello")
        };
        let reply = hello.clone();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            write(&mut stream, &Message::Hello(reply)).await.unwrap();
        });
        let error = trial(
            &socket,
            &hello,
            &hello.app_version,
            kelvo_update::ProtocolRange {
                min: hello.min_compatible,
                max: hello.proto_version + 1,
            },
        )
        .await
        .unwrap_err();
        assert!(
            error.to_string().contains("protocol range differs"),
            "{error:#}"
        );
        server.await.unwrap();
    }
}

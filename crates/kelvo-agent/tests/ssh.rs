#![allow(clippy::unwrap_used)]
//! The controller's real SSH path against an agent on another host. Run by hand:
//!
//! ```sh
//! KELVO_AGENT_SSH_ALIAS=linux-tailscale \
//! KELVO_AGENT_SSH_HELPER=/tmp/kelvo-agent-qa-serve \
//! KELVO_AGENT_SSH_BIN=/tmp/kelvo-agent-qa \
//!   cargo test -p kelvo-agent --test ssh -- --ignored --nocapture
//! ```
//!
//! `KELVO_AGENT_SSH_HELPER` is a remote script that ends in `exec <agent> serve --stdio
//! --socket <the daemon's socket>` (the controller appends `serve --stdio`, which the
//! script ignores), with the daemon already running there. `KELVO_AGENT_SSH_BIN` is the
//! bare agent binary: its default socket has no daemon, so the controller must report
//! the agent as not running.
use std::time::Duration;

use kelvo_proto::{Message, RemoteInterest};
use kelvo_remote::{EndpointConfig, RemoteError, SshSession};
use kelvo_schema::HostId;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn config(helper: String) -> EndpointConfig {
    EndpointConfig {
        lan: Some(std::env::var("KELVO_AGENT_SSH_ALIAS").unwrap()),
        tailscale: None,
        helper_path: helper,
    }
}

async fn read(session: &mut SshSession) -> Message {
    tokio::time::timeout(Duration::from_secs(15), async {
        let length = session.reader.read_u32().await.unwrap();
        let mut body = vec![0; length as usize];
        session.reader.read_exact(&mut body).await.unwrap();
        kelvo_proto::decode_message(&body).unwrap()
    })
    .await
    .expect("the remote agent must make protocol progress")
}

#[tokio::test]
#[ignore = "needs an SSH host running kelvo-agent; see the module docs"]
async fn controller_handshakes_and_streams_over_real_ssh() {
    let local = HostId(uuid::Uuid::new_v4());
    let helper = std::env::var("KELVO_AGENT_SSH_HELPER").unwrap();
    let mut session = kelvo_remote::connect(&config(helper), local, None)
        .await
        .unwrap();
    println!(
        "Hello: host {} ({:?} {}), agent {}, proto {}, epoch {}, features {:?}",
        session.hello.host.id,
        session.hello.host.info.os,
        session.hello.host.info.os_version,
        session.hello.app_version,
        session.negotiated.proto_version,
        session.hello.db_instance_uuid,
        session.negotiated.features,
    );
    assert!(matches!(read(&mut session).await, Message::SourceStatus(_)));
    let interest = Message::Interest(RemoteInterest {
        detail: true,
        ..RemoteInterest::default()
    });
    session
        .writer
        .write_all(&kelvo_proto::encode_frame(&interest).unwrap())
        .await
        .unwrap();
    session.writer.flush().await.unwrap();
    let mut live = 0;
    while live < 3 {
        match read(&mut session).await {
            Message::Live(frame) => {
                live += 1;
                println!(
                    "Live ts_ms={} values={} layout={}",
                    frame.ts_ms,
                    frame.values.len(),
                    frame.layout_no
                );
            }
            Message::LayoutDef(layout) => println!(
                "LayoutDef {} with {} series",
                layout.layout_no,
                layout.series.len()
            ),
            other => println!("{}", message_name(&other)),
        }
    }
    session.close().await;
}

#[tokio::test]
#[ignore = "needs an SSH host with the kelvo-agent binary; see the module docs"]
async fn controller_reports_a_stopped_agent_as_not_running() {
    let local = HostId(uuid::Uuid::new_v4());
    let bin = std::env::var("KELVO_AGENT_SSH_BIN").unwrap();
    let error = kelvo_remote::connect(&config(bin), local, None)
        .await
        .err()
        .expect("no daemon is listening on the default socket");
    println!("error: {error}");
    assert!(
        matches!(error, RemoteError::AgentNotRunning(_)),
        "{error:?}"
    );
}

fn message_name(message: &Message) -> &'static str {
    match message {
        Message::SourceStatus(_) => "SourceStatus",
        Message::CapabilitiesChanged(_) => "CapabilitiesChanged",
        Message::HostSummary(_) => "HostSummary",
        _ => "other message",
    }
}

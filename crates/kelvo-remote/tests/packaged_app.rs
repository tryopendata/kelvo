#![allow(clippy::too_many_lines)] // Integration scenarios keep setup and assertions together.
//! Opt-in acceptance against the actual packaged app and collectors, on one Mac.
//! Launch the isolated com.tryopendata.kelvo.remoteqa build before running this test.
#![allow(clippy::unwrap_used)]

use kelvo_proto::{Message, Offer, RemoteInterest};
use kelvo_remote::{ControlRequest, ControlResponse, SocketPaths, control_request};
use std::{io::Write, path::PathBuf, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

async fn read(stream: &mut UnixStream) -> Message {
    tokio::time::timeout(Duration::from_secs(15), async {
        let len = stream.read_u32().await.unwrap() as usize;
        assert!((1..=1024 * 1024).contains(&len));
        let mut bytes = vec![0; len];
        stream.read_exact(&mut bytes).await.unwrap();
        kelvo_proto::decode_message(&bytes).unwrap()
    })
    .await
    .expect("packaged app must keep responding")
}

async fn send(stream: &mut UnixStream, message: Message) {
    stream
        .write_all(&kelvo_proto::encode_frame(&message).unwrap())
        .await
        .unwrap();
}

// Restore even if an assertion fails; this uses only the isolated QA identity.
struct Restore {
    socket: PathBuf,
    request: Option<ControlRequest>,
}
impl Drop for Restore {
    fn drop(&mut self) {
        let Some(request) = &self.request else {
            return;
        };
        let result = (|| -> std::io::Result<()> {
            let mut socket = std::os::unix::net::UnixStream::connect(&self.socket)?;
            socket.set_write_timeout(Some(Duration::from_secs(3)))?;
            socket.set_read_timeout(Some(Duration::from_secs(3)))?;
            let bytes = serde_json::to_vec(request)?;
            socket.write_all(&(bytes.len() as u32).to_be_bytes())?;
            socket.write_all(&bytes)?;
            let mut size = [0; 4];
            std::io::Read::read_exact(&mut socket, &mut size)?;
            let len = u32::from_be_bytes(size) as usize;
            if len > 65536 {
                return Err(std::io::Error::other("oversized restoration reply"));
            }
            let mut bytes = vec![0; len];
            std::io::Read::read_exact(&mut socket, &mut bytes)?;
            let response: ControlResponse = serde_json::from_slice(&bytes)?;
            if !response.ok {
                return Err(std::io::Error::other(response.message));
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("QA app settings restoration failed: {error}");
        }
    }
}

#[tokio::test]
#[ignore = "requires isolated packaged QA app; set KELVO_QA_DATA_DIR and run explicitly"]
async fn packaged_app_streams_real_metrics_and_revokes_access_when_disabled() {
    let dir = PathBuf::from(std::env::var_os("KELVO_QA_DATA_DIR").expect("KELVO_QA_DATA_DIR"));
    assert_eq!(
        dir.file_name().unwrap(),
        "com.tryopendata.kelvo.remoteqa",
        "never change the everyday app's settings in acceptance tests"
    );
    let paths = SocketPaths::new(dir);
    let before = control_request(&paths.control, &ControlRequest::Status)
        .await
        .unwrap();
    assert!(before.ok);
    let host = before.host.unwrap();
    let mut restore = Restore {
        socket: paths.control.clone(),
        request: Some(ControlRequest::Configure {
            allow_remote_monitoring: before.allow_remote_monitoring.unwrap(),
            show_menu_bar_icon: None,
            launch_at_login: None,
        }),
    };
    let enabled = control_request(
        &paths.control,
        &ControlRequest::Configure {
            allow_remote_monitoring: true,
            show_menu_bar_icon: None,
            launch_at_login: None,
        },
    )
    .await
    .unwrap();
    assert!(enabled.ok, "{}", enabled.message);
    let mut stream = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match UnixStream::connect(&paths.metrics).await {
                Ok(stream) => break stream,
                Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .expect("enabling monitoring starts the socket");
    let Message::Hello(hello) = read(&mut stream).await else {
        panic!("expected Hello")
    };
    assert_eq!(hello.host.id, host.id);
    assert!(hello.host.info.mem_total_bytes > 0);
    let negotiated = kelvo_proto::negotiate(
        &Offer::local(kelvo_proto::local_features()),
        &Offer::from(&*hello),
    )
    .unwrap();
    send(&mut stream, Message::HelloAck(negotiated.ack())).await;
    send(
        &mut stream,
        Message::Interest(RemoteInterest {
            detail: true,
            process_period_ms: Some(1000),
            ..Default::default()
        }),
    )
    .await;
    send(
        &mut stream,
        Message::Ping {
            nonce: 47,
            t0_ms: 0,
        },
    )
    .await;
    let mut layouts = std::collections::HashMap::new();
    let mut timestamps = Vec::new();
    let mut saw_processes = false;
    let mut saw_pong = false;
    tokio::time::timeout(Duration::from_secs(20), async {
        while timestamps.len() < 2 || !saw_processes || !saw_pong {
            match read(&mut stream).await {
                Message::LayoutDef(layout) => {
                    layouts.insert(layout.layout_no, layout.series);
                }
                Message::Live(frame) => {
                    let series = layouts.get(&frame.layout_no).expect("layout before frame");
                    assert_eq!(frame.values.len(), series.len());
                    assert_eq!(frame.held.len(), series.len());
                    assert_eq!(frame.holds.len(), series.len());
                    let memory = series
                        .iter()
                        .position(|s| s.metric.as_str() == "mem.used")
                        .expect("real memory collector must be available");
                    assert!(frame.held[memory] > 0.0);
                    assert!(frame.held[memory] <= hello.host.info.mem_total_bytes as f32);
                    assert!(frame.interval_ms > 0);
                    if timestamps.last() != Some(&frame.ts_ms) {
                        timestamps.push(frame.ts_ms);
                    }
                }
                Message::LiveProcesses(batch) => {
                    saw_processes = batch
                        .rows
                        .iter()
                        .any(|p| p.pid == std::process::id() as i32);
                }
                Message::Pong {
                    nonce: 47,
                    t_remote_ms,
                    ..
                } => {
                    assert!(t_remote_ms > 0);
                    saw_pong = true;
                }
                Message::Error { message, .. } => panic!("app reported {message}"),
                _ => {}
            }
        }
    })
    .await
    .expect("live metrics, this test process and heartbeat must arrive");
    assert!(timestamps[1] > timestamps[0]);
    let disabled = control_request(
        &paths.control,
        &ControlRequest::Configure {
            allow_remote_monitoring: false,
            show_menu_bar_icon: None,
            launch_at_login: None,
        },
    )
    .await
    .unwrap();
    assert!(disabled.ok, "{}", disabled.message);
    tokio::time::timeout(Duration::from_secs(5), async {
        while paths.metrics.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("disabling monitoring removes the listener");
    let mut trailing = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut trailing))
        .await
        .expect("existing viewer must be disconnected")
        .unwrap();
    assert!(UnixStream::connect(&paths.metrics).await.is_err());
    let restored = control_request(&paths.control, restore.request.as_ref().unwrap())
        .await
        .unwrap();
    assert!(restored.ok);
    assert_eq!(
        restored.allow_remote_monitoring,
        before.allow_remote_monitoring
    );
    restore.request = None;
}

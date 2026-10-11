//! Public remote server exercised through framed byte streams and real SQLite.
//! The fixture adapter only supplies existing LiveHub/Store instances. Assertions
//! are on client-visible protocol messages and queried mirrored history.
#![allow(clippy::unwrap_used)]

use kelvo_engine::{BusMsg, FrameLayout, LiveFrame, LiveHub, ProcessBatch, ProcessSample};
use kelvo_proto::{Hello, Message, Offer, RemoteInterest, SyncRequest};
use kelvo_remote::{ServerProvider, page_from_wire, page_to_wire, serve_connection, serve_metrics};
use kelvo_schema::{
    Capabilities, HostId, HostInfo, HostRecord, MetricId, OsKind, SeriesKey, SyncKinds, Tier,
};
use kelvo_store::{BucketRow, Store, StoreConfig};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use uuid::Uuid;

struct Fixture {
    host: HostRecord,
    hub: LiveHub,
    store: Store,
    _directory: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Arc<Self> {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(StoreConfig::new(directory.path().join("source.sqlite"))).unwrap();
        let host = HostRecord {
            id: HostId(Uuid::new_v4()),
            is_local: true,
            display_name: "Fixture Mac".into(),
            info: HostInfo {
                os: OsKind::MacOs,
                os_version: "26.0".into(),
                model: None,
                chip: None,
                chip_known: false,
                cpu_topology: vec![],
                mem_total_bytes: 16 << 30,
                boot_time_ms: 0,
                gpu_dvfs_mhz: vec![],
                boot_mounts: vec![],
            },
        };
        store.writer().upsert_host(host.clone()).unwrap();
        Arc::new(Self {
            host,
            hub: LiveHub::default(),
            store,
            _directory: directory,
        })
    }
    fn frame(&self, ts_ms: i64, value: f32) {
        self.hub.publish(BusMsg::Frame(Arc::new(LiveFrame {
            ts_ms,
            interval_ms: 1000,
            timeline: 0,
            layout: Arc::new(FrameLayout {
                layout_no: 0,
                series: vec![cpu()].into(),
            }),
            values: vec![value].into(),
            held: vec![value].into(),
            holds: vec![2500].into(),
        })));
    }
    fn processes(&self, ts_ms: i64, pid: i32) {
        self.hub.publish(BusMsg::Processes(Arc::new(ProcessBatch {
            ts_ms,
            gpu_span_ms: None,
            rows: vec![ProcessSample {
                pid,
                start_time_us: 1,
                name: "fixture-worker".into(),
                cpu_pct: 12.0,
                mem_bytes: 4096,
                compressed_bytes: None,
                threads: 1,
                idle_wakeups_per_s: 0.0,
                energy: 0.0,
                energy_j: 0.0,
                interval_s: 1.0,
                app: None,
                app_main: false,
                disk_read_bps: 0.0,
                disk_write_bps: 0.0,
                net_rx_bps: None,
                net_tx_bps: None,
                gpu_pct: None,
                ports: None,
                user: "fixture".into(),
            }],
        })));
    }
}
impl ServerProvider for Fixture {
    fn hello(&self) -> Hello {
        Hello {
            proto_version: kelvo_proto::PROTO_VERSION,
            min_compatible: kelvo_proto::MIN_COMPATIBLE,
            app_version: "fixture".into(),
            host: self.host.identity(),
            db_instance_uuid: self.store.epoch(),
            capabilities: Capabilities::default(),
            features: kelvo_proto::local_features(),
        }
    }
    fn live(&self) -> LiveHub {
        self.hub.clone()
    }
    // Collection-interest aggregation belongs to the shell and is covered there.
    // The fixture injects deterministic collector samples directly into the real hub.
    fn set_interest(&self, _session: u64, _interest: RemoteInterest) {}
    fn sync(&self, request: &SyncRequest, kinds: SyncKinds) -> Result<Message, String> {
        self.store
            .reader()
            .and_then(|mut reader| {
                reader.read_after_since(
                    self.host.id,
                    request.tier,
                    request.after,
                    request.max_rows,
                    kinds,
                    request.from_ms,
                )
            })
            .map(page_to_wire)
            .map_err(|error| error.to_string())
    }
}
fn cpu() -> SeriesKey {
    SeriesKey::bare(MetricId::from_static("cpu.total"))
}
async fn write<S: AsyncWrite + Unpin>(stream: &mut S, message: &Message) {
    stream
        .write_all(&kelvo_proto::encode_frame(message).unwrap())
        .await
        .unwrap();
    stream.flush().await.unwrap();
}
async fn read<S: AsyncRead + Unpin>(stream: &mut S) -> Message {
    tokio::time::timeout(Duration::from_secs(3), async {
        let length = stream.read_u32().await.unwrap() as usize;
        assert!(length <= 1024 * 1024);
        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes).await.unwrap();
        kelvo_proto::decode_message(&bytes).unwrap()
    })
    .await
    .expect("server must make protocol progress")
}
async fn handshake<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> Hello {
    let Message::Hello(hello) = read(stream).await else {
        panic!("expected Hello")
    };
    let negotiated = kelvo_proto::negotiate(
        &Offer::local(kelvo_proto::local_features()),
        &Offer::from(&hello),
    )
    .unwrap();
    write(stream, &Message::HelloAck(negotiated.ack())).await;
    assert!(matches!(read(stream).await, Message::SourceStatus(_)));
    hello
}
async fn subscribe<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, processes: bool) {
    write(
        stream,
        &Message::Interest(RemoteInterest {
            detail: true,
            process_period_ms: processes.then_some(0),
            ..Default::default()
        }),
    )
    .await;
    assert!(matches!(read(stream).await, Message::SourceStatus(_)));
}
fn session(
    fixture: &Arc<Fixture>,
    capacity: usize,
) -> (
    DuplexStream,
    tokio::task::JoinHandle<kelvo_remote::Result<()>>,
) {
    let (client, server) = tokio::io::duplex(capacity);
    (
        client,
        tokio::spawn(serve_connection(server, fixture.clone())),
    )
}
async fn receive_live<S: AsyncRead + Unpin>(stream: &mut S, ts: i64, value: f32) {
    loop {
        match read(stream).await {
            Message::LayoutDef(layout) => assert_eq!(layout.series, vec![cpu()]),
            Message::Live(frame) => {
                assert_eq!(frame.ts_ms, ts);
                assert_eq!(frame.values, vec![value]);
                return;
            }
            unexpected => panic!("unexpected frame before metric: {unexpected:?}"),
        }
    }
}

#[tokio::test]
async fn controller_subscriptions_are_independent_and_one_disconnect_preserves_the_other() {
    let fixture = Fixture::new();
    let (mut metrics, metrics_task) = session(&fixture, 65536);
    let (mut processes, processes_task) = session(&fixture, 65536);
    assert_eq!(
        handshake(&mut metrics).await.host.id,
        handshake(&mut processes).await.host.id
    );
    subscribe(&mut metrics, false).await;
    subscribe(&mut processes, true).await;
    fixture.processes(1000, 123);
    fixture.frame(1000, 42.0);
    // Processes reach only the subscriber asking for them; the other sees metrics first.
    assert!(
        matches!(read(&mut processes).await, Message::LiveProcesses(batch) if batch.rows.len()==1 && batch.rows[0].pid==123)
    );
    receive_live(&mut metrics, 1000, 42.0).await;
    receive_live(&mut processes, 1000, 42.0).await;
    drop(processes);
    assert!(processes_task.await.unwrap().is_err());
    fixture.frame(2000, 17.0);
    receive_live(&mut metrics, 2000, 17.0).await;
    write(
        &mut metrics,
        &Message::Ping {
            nonce: 88,
            t0_ms: 2,
        },
    )
    .await;
    assert!(matches!(
        read(&mut metrics).await,
        Message::Pong { nonce: 88, .. }
    ));
    drop(metrics);
    assert!(metrics_task.await.unwrap().is_err());
}

#[tokio::test]
async fn partial_controller_frame_survives_live_output_between_its_fragments() {
    let fixture = Fixture::new();
    let (mut client, task) = session(&fixture, 1);
    handshake(&mut client).await;
    subscribe(&mut client, false).await;
    let ping = kelvo_proto::encode_frame(&Message::Ping {
        nonce: 456,
        t0_ms: 123,
    })
    .unwrap();
    client.write_all(&ping[..2]).await.unwrap();
    fixture.frame(1000, 28.0);
    receive_live(&mut client, 1000, 28.0).await;
    client.write_all(&ping[2..]).await.unwrap();
    assert!(matches!(
        read(&mut client).await,
        Message::Pong {
            nonce: 456,
            t0_ms: 123,
            ..
        }
    ));
    drop(client);
    assert!(task.await.unwrap().is_err());
}

#[tokio::test(start_paused = true)]
async fn a_stalled_viewer_times_out_without_blocking_a_healthy_viewer() {
    let fixture = Fixture::new();
    let (mut slow, slow_task) = session(&fixture, 128);
    let (mut healthy, healthy_task) = session(&fixture, 65536);
    handshake(&mut slow).await;
    subscribe(&mut slow, false).await;
    handshake(&mut healthy).await;
    subscribe(&mut healthy, false).await;
    fixture.frame(1000, 63.0);
    receive_live(&mut healthy, 1000, 63.0).await;
    // Stop consuming the first connection. Virtual time exercises the production
    // write deadline; the healthy client must keep receiving frames meanwhile.
    fixture.frame(2000, 24.0);
    receive_live(&mut healthy, 2000, 24.0).await;
    let stopped = tokio::time::timeout(Duration::from_secs(13), slow_task)
        .await
        .expect("a stalled viewer must be disconnected")
        .unwrap()
        .unwrap_err();
    assert!(stopped.to_string().contains("peer stopped reading"));
    fixture.frame(3000, 81.0);
    receive_live(&mut healthy, 3000, 81.0).await;
    drop(slow);
    drop(healthy);
    assert!(healthy_task.await.unwrap().is_err());
}

#[tokio::test]
async fn reconnect_resumes_durable_sqlite_cursor_without_duplicate_history() {
    let fixture = Fixture::new();
    for (ts, value) in [(0, 12.0), (10000, 28.0), (20000, 91.0)] {
        fixture
            .store
            .writer()
            .write_bucket(BucketRow {
                host: fixture.host.id,
                tier: Tier::S10,
                bucket_ts: ts,
                series: vec![cpu()].into(),
                stats: vec![value, value, value],
            })
            .unwrap();
    }
    fixture.store.writer().flush().unwrap();
    let mirror_dir = tempfile::tempdir().unwrap();
    let mirror = Store::open(StoreConfig::new(mirror_dir.path().join("mirror.sqlite"))).unwrap();
    let mut remote_host = fixture.host.clone();
    remote_host.is_local = false;
    mirror.writer().upsert_host(remote_host).unwrap();
    let (mut first, first_task) = session(&fixture, 65536);
    let hello = handshake(&mut first).await;
    write(
        &mut first,
        &Message::SyncRequest(SyncRequest {
            tier: Tier::S10,
            after: None,
            max_rows: 1,
            from_ms: None,
            max_bytes: Some(16384),
        }),
    )
    .await;
    let Message::SyncPage(page) = read(&mut first).await else {
        panic!("expected persisted page")
    };
    assert!(page.more);
    assert_eq!(page.rows.len(), 1);
    mirror
        .writer()
        .ingest_page_durable(
            fixture.host.id,
            page_from_wire(page.clone(), SyncKinds::ALL).unwrap(),
        )
        .unwrap();
    drop(first);
    assert!(first_task.await.unwrap().is_err());
    // Replaying the page before reconnect also exercises stable durable cursor storage.
    mirror
        .writer()
        .ingest_page_durable(
            fixture.host.id,
            page_from_wire(page, SyncKinds::ALL).unwrap(),
        )
        .unwrap();
    let (mut second, second_task) = session(&fixture, 65536);
    assert_eq!(
        handshake(&mut second).await.db_instance_uuid,
        hello.db_instance_uuid
    );
    loop {
        let after = mirror
            .reader()
            .unwrap()
            .resume_cursor(
                fixture.host.id,
                Tier::S10,
                hello.db_instance_uuid,
                SyncKinds::ALL,
            )
            .unwrap();
        write(
            &mut second,
            &Message::SyncRequest(SyncRequest {
                tier: Tier::S10,
                after,
                max_rows: 1,
                from_ms: None,
                max_bytes: Some(16384),
            }),
        )
        .await;
        let Message::SyncPage(page) = read(&mut second).await else {
            panic!("expected next persisted page")
        };
        let more = page.more;
        mirror
            .writer()
            .ingest_page_durable(
                fixture.host.id,
                page_from_wire(page, SyncKinds::ALL).unwrap(),
            )
            .unwrap();
        if !more {
            break;
        }
    }
    let history = mirror
        .reader()
        .unwrap()
        .history(&kelvo_store::HistoryQuery {
            host: fixture.host.id,
            selectors: vec![kelvo_schema::SeriesSelector {
                metric: MetricId::from_static("cpu.total"),
                labels: Default::default(),
            }],
            from_ms: 0,
            to_ms: 30000,
            tier: kelvo_store::TierChoice::Fixed(Tier::S10),
            max_points: 100,
        })
        .unwrap();
    assert_eq!(history.series.len(), 1);
    let points: Vec<_> = history.series[0]
        .points
        .iter()
        .map(|point| (point.t, point.avg))
        .collect();
    assert_eq!(points, vec![(0, 12.0), (10000, 28.0), (20000, 91.0)]);
    drop(second);
    assert!(second_task.await.unwrap().is_err());
}

async fn connect_when_ready(path: &Path) -> tokio::net::UnixStream {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match tokio::net::UnixStream::connect(path).await {
                Ok(stream) => return stream,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tokio::task::yield_now().await
                }
                Err(error) => panic!("socket failed: {error}"),
            }
        }
    })
    .await
    .expect("metrics listener must start")
}
#[tokio::test]
async fn stopping_metrics_closes_all_clients_and_allows_a_clean_restart() {
    let fixture = Fixture::new();
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let socket = directory.path().join("remote/metrics.sock");
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let server = tokio::spawn(serve_metrics(socket.clone(), fixture.clone(), receiver));
    let mut first = connect_when_ready(&socket).await;
    let mut second = connect_when_ready(&socket).await;
    handshake(&mut first).await;
    handshake(&mut second).await;
    shutdown.send(true).unwrap();
    server.await.unwrap().unwrap();
    let mut byte = [0];
    assert_eq!(first.read(&mut byte).await.unwrap(), 0);
    assert_eq!(second.read(&mut byte).await.unwrap(), 0);
    assert!(!socket.exists());
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let restarted = tokio::spawn(serve_metrics(socket.clone(), fixture.clone(), receiver));
    let mut client = connect_when_ready(&socket).await;
    assert_eq!(handshake(&mut client).await.host.id, fixture.host.id);
    shutdown.send(true).unwrap();
    restarted.await.unwrap().unwrap();
}

#[tokio::test]
async fn invalid_frame_isolated_to_its_client_and_listener_accepts_another_controller() {
    let fixture = Fixture::new();
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let socket = directory.path().join("remote/metrics.sock");
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let server = tokio::spawn(serve_metrics(socket.clone(), fixture.clone(), receiver));
    let mut invalid = connect_when_ready(&socket).await;
    handshake(&mut invalid).await;
    // The length must be rejected without waiting for a body or allocating it.
    invalid.write_u32(2 * 1024 * 1024).await.unwrap();
    let mut byte = [0];
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), invalid.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    let mut healthy = connect_when_ready(&socket).await;
    handshake(&mut healthy).await;
    write(&mut healthy, &Message::Ping { nonce: 7, t0_ms: 0 }).await;
    assert!(matches!(
        read(&mut healthy).await,
        Message::Pong { nonce: 7, .. }
    ));
    shutdown.send(true).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn socket_startup_preserves_an_existing_regular_file() {
    let fixture = Fixture::new();
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let socket = directory.path().join("metrics.sock");
    std::fs::write(&socket, b"user-owned contents").unwrap();
    let (_shutdown, receiver) = tokio::sync::watch::channel(false);
    let error = serve_metrics(socket.clone(), fixture, receiver)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("unsafe existing socket path"));
    assert_eq!(std::fs::read(socket).unwrap(), b"user-owned contents");
}

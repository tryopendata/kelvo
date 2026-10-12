use crate::{RemoteError, Result, codec, socket};
use kelvo_engine::{BusMsg, EngineStatus, LiveHub, ProcessSample};
use kelvo_proto::{Hello, Message, RemoteInterest, SourceStatus, SyncRequest, WireProcess};
use kelvo_schema::SyncKinds;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::watch,
};

pub trait ServerProvider: Send + Sync + 'static {
    fn hello(&self) -> Hello;
    fn live(&self) -> LiveHub;
    /// Replace this session's interest. Default interest releases all collection demand.
    fn set_interest(&self, session: u64, interest: RemoteInterest);
    fn sync(&self, request: &SyncRequest, kinds: SyncKinds)
    -> std::result::Result<Message, String>;
    fn recent(&self, _from: i64, _to: i64) -> std::result::Result<Message, String> {
        Err("recent history unavailable".into())
    }
    fn status(&self) -> SourceStatus {
        status_to_wire(&self.live().status(), false)
    }
}
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
struct Release {
    provider: Arc<dyn ServerProvider>,
    session: u64,
}
impl Drop for Release {
    fn drop(&mut self) {
        self.provider
            .set_interest(self.session, RemoteInterest::default());
    }
}

pub async fn serve_metrics(
    path: PathBuf,
    provider: Arc<dyn ServerProvider>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let listener = socket::bind_private(&path)?;
    let _cleanup = socket::SocketCleanup(path);
    let mut sessions = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            _=shutdown.changed()=>break,
            accepted=listener.accept()=>{
                accept_metrics(accepted, &provider, &mut sessions).await?;
            },
            _=sessions.join_next(),if !sessions.is_empty()=>{}
        }
    }
    sessions.abort_all();
    while sessions.join_next().await.is_some() {}
    Ok(())
}
async fn accept_metrics(
    accepted: std::io::Result<(tokio::net::UnixStream, tokio::net::unix::SocketAddr)>,
    provider: &Arc<dyn ServerProvider>,
    sessions: &mut tokio::task::JoinSet<()>,
) -> Result<()> {
    let (stream, _) = match accepted {
        Ok(connection) => connection,
        Err(error) => {
            tracing::warn!(%error, "remote metrics socket accept failed; retrying");
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            return Ok(());
        }
    };
    if socket::same_uid(&stream).is_err() || sessions.len() >= 16 {
        return Ok(());
    }
    let provider = provider.clone();
    sessions.spawn(async move {
        if let Err(error) = serve_connection(stream, provider).await {
            tracing::debug!(%error, "remote session closed");
        }
    });
    Ok(())
}
pub async fn serve_connection<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    provider: Arc<dyn ServerProvider>,
) -> Result<()> {
    let (mut reader, mut writer) = tokio::io::split(stream);
    let hello = provider.hello();
    codec::write(&mut writer, &Message::Hello(hello.clone())).await?;
    let ack = tokio::time::timeout(Duration::from_secs(5), codec::read(&mut reader))
        .await
        .map_err(|_| RemoteError::Transport("handshake deadline".into()))??;
    let Message::HelloAck(ack) = ack else {
        return Err(RemoteError::Protocol("expected HelloAck".into()));
    };
    if ack.proto_version < hello.min_compatible
        || ack.proto_version > hello.proto_version
        || !ack.features.is_subset(&hello.features)
        || !ack.features.contains("rows.stable-ids")
    {
        return Err(RemoteError::Incompatible(
            "unsupported acknowledgement/features".into(),
        ));
    }
    let kinds = SyncKinds::from_features(ack.features.iter().map(String::as_str));
    let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    let _release = Release {
        provider: provider.clone(),
        session,
    };
    let hub = provider.live();
    let mut bus = hub.subscribe();
    let (mut input, task) = codec::reader_task(reader);
    let _reader = codec::AbortOnDrop(task);
    let interest = RemoteInterest::default();
    let mut summary = tokio::time::interval(Duration::from_secs(5));
    summary.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let last_layout = None;
    let last_process = tokio::time::Instant::now() - Duration::from_secs(3600);
    codec::write(&mut writer, &Message::SourceStatus(provider.status())).await?;
    let mut connection = Connection {
        provider,
        hub,
        writer,
        session,
        kinds,
        interest,
        last_layout,
        last_process,
    };
    loop {
        tokio::select! {
            incoming = input.recv() => connection.incoming(incoming).await?,
            event = bus.recv_event() => connection.event(event).await?,
            _ = summary.tick(), if !connection.interest.detail => connection.summary().await?,
        }
    }
}

struct Connection<W> {
    provider: Arc<dyn ServerProvider>,
    hub: LiveHub,
    writer: W,
    session: u64,
    kinds: SyncKinds,
    interest: RemoteInterest,
    last_layout: Option<u32>,
    last_process: tokio::time::Instant,
}
impl<W: AsyncWrite + Unpin> Connection<W> {
    async fn incoming(&mut self, incoming: Option<Result<Message>>) -> Result<()> {
        let message = incoming.ok_or_else(|| RemoteError::Transport("peer closed".into()))??;
        match message {
            Message::Interest(next) => {
                self.interest = next;
                self.provider
                    .set_interest(self.session, self.interest.clone());
                codec::write(
                    &mut self.writer,
                    &Message::SourceStatus(self.provider.status()),
                )
                .await?;
                if self.interest.detail
                    && let Some(frame) = self.hub.latest_frame()
                {
                    send_frame(&mut self.writer, &frame, &mut self.last_layout).await?;
                }
            }
            Message::Subscribe { tiers } => {
                self.interest.detail = tiers.contains(&kelvo_proto::LiveTier::Live1s);
                self.provider
                    .set_interest(self.session, self.interest.clone());
            }
            Message::Unsubscribe => {
                self.interest = RemoteInterest::default();
                self.provider
                    .set_interest(self.session, self.interest.clone());
            }
            Message::Ping { nonce, t0_ms } => {
                codec::write(
                    &mut self.writer,
                    &Message::Pong {
                        nonce,
                        t0_ms,
                        t_remote_ms: kelvo_engine::wall_ms(),
                    },
                )
                .await?
            }
            Message::SyncRequest(mut request) => {
                request.max_rows = request.max_rows.clamp(1, 2000);
                request.max_bytes = Some(
                    request
                        .max_bytes
                        .unwrap_or(16 * 1024)
                        .clamp(1024, 256 * 1024),
                );
                let p = self.provider.clone();
                let kinds = self.kinds;
                let result = tokio::task::spawn_blocking(move || p.sync(&request, kinds))
                    .await
                    .map_err(|e| RemoteError::Transport(e.to_string()))?;
                let reply = result.unwrap_or_else(|message| Message::Error {
                    code: kelvo_proto::ErrorCode::Internal,
                    message,
                });
                if kelvo_proto::encode_message(&reply)?.len() > 256 * 1024 {
                    return Err(RemoteError::Protocol(
                        "history provider exceeded page limit".into(),
                    ));
                }
                codec::write(&mut self.writer, &reply).await?;
            }
            Message::RecentRequest { from_ms, to_ms } => {
                let p = self.provider.clone();
                let reply = tokio::task::spawn_blocking(move || {
                    p.recent(from_ms.max(to_ms.saturating_sub(300_000)), to_ms)
                })
                .await
                .map_err(|e| RemoteError::Transport(e.to_string()))?
                .unwrap_or_else(|message| Message::Error {
                    code: kelvo_proto::ErrorCode::Internal,
                    message,
                });
                let reply = if kelvo_proto::encode_message(&reply)?.len() > codec::MAX_WIRE_BYTES {
                    Message::Error {
                        code: kelvo_proto::ErrorCode::Internal,
                        message: "recent history snapshot exceeds transport limit".into(),
                    }
                } else {
                    reply
                };
                codec::write(&mut self.writer, &reply).await?;
            }
            Message::Unknown => {}
            _ => {
                return Err(RemoteError::Protocol(
                    "unexpected controller message".into(),
                ));
            }
        }
        Ok(())
    }
    async fn event(&mut self, event: kelvo_engine::Recv) -> Result<()> {
        match event {
            kelvo_engine::Recv::Msg(BusMsg::Frame(frame)) if self.interest.detail => {
                send_frame(&mut self.writer, &frame, &mut self.last_layout).await?
            }
            kelvo_engine::Recv::Msg(BusMsg::Caps(caps)) => {
                codec::write(
                    &mut self.writer,
                    &Message::CapabilitiesChanged((*caps).clone()),
                )
                .await?
            }
            kelvo_engine::Recv::Msg(BusMsg::Status(_)) => {
                codec::write(
                    &mut self.writer,
                    &Message::SourceStatus(self.provider.status()),
                )
                .await?
            }
            kelvo_engine::Recv::Msg(BusMsg::Processes(batch))
                if self.interest.process_period_ms.is_some() =>
            {
                let period = self.interest.process_period_ms.unwrap_or(1000);
                if self.last_process.elapsed() >= Duration::from_millis(u64::from(period)) {
                    if batch.rows.len() > 32768 {
                        return Err(RemoteError::Protocol("too many process rows".into()));
                    }
                    codec::write(
                        &mut self.writer,
                        &Message::LiveProcesses(kelvo_proto::LiveProcesses {
                            ts_ms: batch.ts_ms,
                            rows: batch.rows.iter().map(process_to_wire).collect(),
                            gpu_span_ms: batch.gpu_span_ms,
                        }),
                    )
                    .await?;
                    self.last_process = tokio::time::Instant::now();
                }
            }
            kelvo_engine::Recv::Lagged(_) => {
                return Err(RemoteError::Transport(
                    "viewer fell behind live stream".into(),
                ));
            }
            kelvo_engine::Recv::Closed => {
                return Err(RemoteError::Transport("engine stopped".into()));
            }
            _ => {}
        }
        Ok(())
    }
    async fn summary(&mut self) -> Result<()> {
        if let Some(frame) = self.hub.latest_frame() {
            codec::write(
                &mut self.writer,
                &Message::HostSummary(kelvo_proto::HostSummary {
                    ts_ms: frame.ts_ms,
                    online: true,
                    headline: frame
                        .layout
                        .series
                        .iter()
                        .cloned()
                        .zip(frame.held.iter().copied())
                        .filter(|(key, _)| {
                            [
                                "cpu.total",
                                "mem.used",
                                "mem.pressure",
                                "gpu.util",
                                "thermal.hottest",
                                "net.rx",
                                "net.tx",
                                "disk.read",
                                "disk.write",
                            ]
                            .contains(&key.metric.as_str())
                        })
                        .collect(),
                }),
            )
            .await?;
        }
        Ok(())
    }
}

async fn send_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    frame: &kelvo_engine::LiveFrame,
    last: &mut Option<u32>,
) -> Result<()> {
    if *last != Some(frame.layout.layout_no) {
        codec::write(
            writer,
            &Message::LayoutDef(kelvo_proto::WireLayout {
                layout_no: frame.layout.layout_no,
                series: frame.layout.series.to_vec(),
            }),
        )
        .await?;
        *last = Some(frame.layout.layout_no);
    }
    codec::write(
        writer,
        &Message::Live(kelvo_proto::LiveFrame {
            ts_ms: frame.ts_ms,
            layout_no: frame.layout.layout_no,
            values: frame.values.to_vec(),
            held: frame.held.to_vec(),
            holds: frame.holds.to_vec(),
            interval_ms: frame.interval_ms,
            timeline: frame.timeline,
        }),
    )
    .await
}
pub fn status_to_wire(s: &EngineStatus, history_available: bool) -> SourceStatus {
    SourceStatus {
        interval_ms: s.interval_ms,
        paused: s.paused,
        asleep: s.asleep,
        on_battery: s.on_battery,
        low_power_mode: s.low_power_mode,
        backed_off: s.backed_off,
        backgrounded: s.backgrounded,
        display_idle: s.display_idle,
        history_held_until: s.history_held_until,
        performance: s.performance,
        power_source: s.power_source,
        primary_iface: s.primary_iface.as_ref().map(ToString::to_string),
        history_available,
    }
}
pub(crate) fn status_from_wire(s: SourceStatus) -> EngineStatus {
    EngineStatus {
        interval_ms: s.interval_ms,
        paused: s.paused,
        asleep: s.asleep,
        on_battery: s.on_battery,
        low_power_mode: s.low_power_mode,
        backed_off: s.backed_off,
        backgrounded: s.backgrounded,
        display_idle: s.display_idle,
        history_held_until: s.history_held_until,
        performance: s.performance,
        power_source: s.power_source,
        primary_iface: s.primary_iface.map(Into::into),
    }
}
fn process_to_wire(p: &ProcessSample) -> WireProcess {
    WireProcess {
        pid: p.pid,
        start_time_us: p.start_time_us,
        name: p.name.to_string(),
        cpu_pct: p.cpu_pct,
        mem_bytes: p.mem_bytes,
        compressed_bytes: p.compressed_bytes,
        threads: p.threads,
        idle_wakeups_per_s: p.idle_wakeups_per_s,
        energy: p.energy,
        disk_read_bps: p.disk_read_bps,
        disk_write_bps: p.disk_write_bps,
        user: p.user.to_string(),
        net_rx_bps: p.net_rx_bps,
        net_tx_bps: p.net_tx_bps,
        gpu_pct: p.gpu_pct,
        ports: p.ports.as_ref().map(|p| p.to_vec()),
        interval_s: p.interval_s,
        energy_j: p.energy_j,
        app: p.app.as_ref().map(ToString::to_string),
        app_main: p.app_main,
    }
}
pub(crate) fn process_from_wire(p: WireProcess) -> ProcessSample {
    ProcessSample {
        pid: p.pid,
        start_time_us: p.start_time_us,
        name: p.name.into(),
        cpu_pct: p.cpu_pct,
        mem_bytes: p.mem_bytes,
        compressed_bytes: p.compressed_bytes,
        threads: p.threads,
        idle_wakeups_per_s: p.idle_wakeups_per_s,
        energy: p.energy,
        disk_read_bps: p.disk_read_bps,
        disk_write_bps: p.disk_write_bps,
        user: p.user.into(),
        net_rx_bps: p.net_rx_bps,
        net_tx_bps: p.net_tx_bps,
        gpu_pct: p.gpu_pct,
        ports: p.ports.map(Into::into),
        interval_s: p.interval_s,
        energy_j: p.energy_j,
        app: p.app.map(Into::into),
        app_main: p.app_main,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use kelvo_schema::{
        Capabilities, HostId, HostIdentity, HostInfo, MetricId, OsKind, SeriesKey, lock::LockExt,
    };
    use std::sync::Mutex;
    pub(crate) struct Provider {
        hub: LiveHub,
        interests: Mutex<Vec<RemoteInterest>>,
    }
    impl ServerProvider for Provider {
        fn hello(&self) -> Hello {
            Hello {
                proto_version: kelvo_proto::PROTO_VERSION,
                min_compatible: kelvo_proto::MIN_COMPATIBLE,
                app_version: "test".into(),
                host: HostIdentity {
                    id: HostId(uuid::Uuid::from_u128(1)),
                    display_name: "Remote Mac".into(),
                    info: HostInfo {
                        os: OsKind::MacOs,
                        os_version: "15".into(),
                        model: None,
                        chip: None,
                        chip_known: false,
                        cpu_topology: vec![],
                        mem_total_bytes: 1,
                        boot_time_ms: 0,
                        gpu_dvfs_mhz: vec![],
                        boot_mounts: vec![],
                    },
                },
                db_instance_uuid: uuid::Uuid::from_u128(2),
                capabilities: Capabilities::default(),
                features: kelvo_proto::local_features(),
            }
        }
        fn live(&self) -> LiveHub {
            self.hub.clone()
        }
        fn set_interest(&self, _session: u64, interest: RemoteInterest) {
            self.interests.lock_ok().push(interest);
        }
        fn sync(
            &self,
            request: &SyncRequest,
            _kinds: SyncKinds,
        ) -> std::result::Result<Message, String> {
            Ok(Message::SyncPage(kelvo_proto::SyncPage {
                tier: request.tier,
                epoch: self.hello().db_instance_uuid,
                layouts: vec![],
                rows: vec![],
                gaps: vec![],
                events: vec![],
                last_seq: request.after.map_or(0, |c| c.seq),
                more: false,
            }))
        }
    }
    pub(crate) fn provider() -> Arc<Provider> {
        Arc::new(Provider {
            hub: LiveHub::default(),
            interests: Mutex::new(vec![]),
        })
    }
    async fn ack(client: &mut tokio::io::DuplexStream) {
        let Message::Hello(hello) = codec::read(client).await.unwrap() else {
            panic!("expected hello")
        };
        let negotiated = kelvo_proto::negotiate(
            &kelvo_proto::Offer::local(kelvo_proto::local_features()),
            &kelvo_proto::Offer::from(&hello),
        )
        .unwrap();
        codec::write(client, &Message::HelloAck(negotiated.ack()))
            .await
            .unwrap();
        assert!(matches!(
            codec::read(client).await.unwrap(),
            Message::SourceStatus(_)
        ));
    }
    #[tokio::test(start_paused = true)]
    async fn accept_error_backs_off_and_keeps_serving() {
        let provider: Arc<dyn ServerProvider> = provider();
        let mut sessions = tokio::task::JoinSet::new();
        let start = tokio::time::Instant::now();
        for _ in 0..2 {
            accept_metrics(
                Err(std::io::Error::from_raw_os_error(libc::EMFILE)),
                &provider,
                &mut sessions,
            )
            .await
            .unwrap();
        }
        assert_eq!(start.elapsed(), std::time::Duration::from_millis(200));
        assert!(sessions.is_empty());
    }
    #[tokio::test]
    async fn live_stream_preserves_timing_layout_and_releases_interest_on_disconnect() {
        let p = provider();
        let (mut client, server) = tokio::io::duplex(1024 * 1024);
        let pp = p.clone();
        let task = tokio::spawn(serve_connection(server, pp));
        ack(&mut client).await;
        let interest = RemoteInterest {
            detail: true,
            process_period_ms: Some(2000),
            gpu_processes: true,
            ..Default::default()
        };
        codec::write(&mut client, &Message::Interest(interest.clone()))
            .await
            .unwrap();
        assert!(matches!(
            codec::read(&mut client).await.unwrap(),
            Message::SourceStatus(_)
        ));
        let layout = Arc::new(kelvo_engine::FrameLayout {
            layout_no: 7,
            series: vec![SeriesKey::bare(MetricId::from_static("cpu.total"))].into(),
        });
        p.hub
            .publish(BusMsg::Frame(Arc::new(kelvo_engine::LiveFrame {
                ts_ms: 123000,
                interval_ms: 5000,
                timeline: 3,
                layout,
                values: vec![41.0].into(),
                held: vec![41.0].into(),
                holds: vec![12500].into(),
            })));
        let layout = codec::read(&mut client).await.unwrap();
        assert!(matches!(layout,Message::LayoutDef(l) if l.layout_no==7));
        let frame = codec::read(&mut client).await.unwrap();
        assert!(
            matches!(frame,Message::Live(f) if f.timeline==3 && f.interval_ms==5000 && f.holds==vec![12500] && f.values==vec![41.0])
        );
        assert_eq!(p.interests.lock_ok().last(), Some(&interest));
        drop(client);
        assert!(task.await.unwrap().is_err());
        assert_eq!(
            p.interests.lock_ok().last(),
            Some(&RemoteInterest::default())
        );
    }
    #[tokio::test]
    async fn server_answers_ping_and_history_on_same_connection() {
        let p = provider();
        let (mut client, server) = tokio::io::duplex(1024 * 1024);
        let task = tokio::spawn(serve_connection(server, p));
        ack(&mut client).await;
        codec::write(
            &mut client,
            &Message::Ping {
                nonce: 42,
                t0_ms: 7,
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            codec::read(&mut client).await.unwrap(),
            Message::Pong {
                nonce: 42,
                t0_ms: 7,
                ..
            }
        ));
        codec::write(
            &mut client,
            &Message::SyncRequest(SyncRequest {
                tier: kelvo_schema::Tier::S10,
                after: None,
                max_rows: u32::MAX,
                from_ms: Some(0),
                max_bytes: Some(u32::MAX),
            }),
        )
        .await
        .unwrap();
        assert!(
            matches!(codec::read(&mut client).await.unwrap(),Message::SyncPage(page) if page.tier==kelvo_schema::Tier::S10&&!page.more)
        );
        drop(client);
        let _ = task.await;
    }
    #[tokio::test]
    async fn rejects_incompatible_features_before_any_interest() {
        let p = provider();
        let (mut client, server) = tokio::io::duplex(1024 * 1024);
        let pp = p.clone();
        let task = tokio::spawn(serve_connection(server, pp));
        let _ = codec::read(&mut client).await.unwrap();
        codec::write(
            &mut client,
            &Message::HelloAck(kelvo_proto::HelloAck {
                proto_version: 1,
                features: Default::default(),
            }),
        )
        .await
        .unwrap();
        assert!(matches!(
            task.await.unwrap(),
            Err(RemoteError::Incompatible(_))
        ));
        assert!(p.interests.lock_ok().is_empty());
    }
}

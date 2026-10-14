use crate::{
    EndpointConfig, HostClock, MirrorError, RemoteError, Result, Route, SshSession, codec, server,
};
use kelvo_engine::{
    BusMsg, EngineStatus, FrameLayout, LiveFrame, ProcessBatch, Source, SourceControl, SourceError,
    SourceHandle, SourceSink,
};
use kelvo_proto::{Message, RemoteInterest, SyncPage, SyncRequest, WireBucket, WireLayout};
use kelvo_schema::{
    Capabilities, Cursor, HostId, HostRecord, Settings, SyncKinds, Tier, lock::LockExt,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};
use tokio::{sync::watch, time::Instant};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub connected: bool,
    pub route: Option<Route>,
    pub retrying: bool,
    pub error: Option<String>,
    pub history_error: Option<String>,
    pub last_seen_ms: Option<i64>,
    pub catching_up: bool,
    pub clock_offset_ms: i64,
}
pub type StatusCallback = Arc<dyn Fn(ConnectionStatus) + Send + Sync>;
/// Each sink owns its isolated mirror and reset generation. Methods must reject
/// writes after that mirror was detached/reset. Failed sync never stops live metrics.
pub trait MirrorSink: Send + Sync + 'static {
    fn cursor(
        &self,
        tier: Tier,
        epoch: uuid::Uuid,
        kinds: SyncKinds,
    ) -> std::result::Result<Option<Cursor>, MirrorError>;
    fn ingest(&self, page: SyncPage, kinds: SyncKinds) -> std::result::Result<(), MirrorError>;
    fn recent(
        &self,
        _epoch: uuid::Uuid,
        _tier: Tier,
        _complete_to_ms: i64,
        _layouts: Vec<WireLayout>,
        _rows: Vec<WireBucket>,
    ) -> std::result::Result<(), MirrorError> {
        Ok(())
    }
    fn truncated(
        &self,
        _tier: Tier,
        _epoch: uuid::Uuid,
        _earliest_ms: i64,
    ) -> std::result::Result<(), MirrorError> {
        Ok(())
    }
    fn enforce_budget(&self, _now_ms: i64) -> std::result::Result<(), MirrorError> {
        Ok(())
    }
    fn prune(&self, _now_ms: i64) -> std::result::Result<(), MirrorError> {
        Ok(())
    }
    fn history_floor(&self, tier: Tier, now_ms: i64) -> i64 {
        now_ms.saturating_sub(match tier {
            Tier::S10 => 86_400_000,
            Tier::M1 => 604_800_000,
            _ => 2_592_000_000,
        })
    }
}
#[derive(Clone, Default)]
struct ControlState {
    interest: RemoteInterest,
    stop: bool,
    asleep: bool,
    retry: u64,
    network: u64,
}
pub struct RemoteSource {
    record: HostRecord,
    local: HostId,
    endpoints: EndpointConfig,
    on_status: StatusCallback,
    mirror: Option<Arc<dyn MirrorSink>>,
    clock: HostClock,
    caps: Mutex<Capabilities>,
    status: Mutex<EngineStatus>,
    control: watch::Sender<ControlState>,
    started: AtomicBool,
    controller_battery: AtomicBool,
    next_layout: AtomicU32,
    next_timeline: AtomicU32,
}
impl RemoteSource {
    pub fn new(
        record: HostRecord,
        local: HostId,
        endpoints: EndpointConfig,
        on_status: StatusCallback,
        mirror: Option<Arc<dyn MirrorSink>>,
    ) -> Arc<Self> {
        let (control, _) = watch::channel(ControlState::default());
        Arc::new(Self {
            record,
            local,
            endpoints,
            on_status,
            mirror,
            clock: HostClock::default(),
            caps: Mutex::new(Capabilities::default()),
            status: Mutex::new(EngineStatus::default()),
            control,
            started: AtomicBool::new(false),
            controller_battery: AtomicBool::new(false),
            next_layout: AtomicU32::new(1),
            next_timeline: AtomicU32::new(1),
        })
    }
    pub fn set_controller_on_battery(&self, on_battery: bool) {
        self.controller_battery.store(on_battery, Ordering::Relaxed);
    }
    pub fn sleep(&self) {
        self.control.send_modify(|s| s.asleep = true);
    }
    pub fn wake(&self) {
        self.control.send_modify(|s| {
            s.asleep = false;
            s.retry = s.retry.wrapping_add(1);
        });
    }
    pub fn clock(&self) -> HostClock {
        self.clock.clone()
    }
    pub fn retry(&self) {
        self.control
            .send_modify(|s| s.retry = s.retry.wrapping_add(1));
    }
    pub fn network_changed(&self) {
        self.control
            .send_modify(|s| s.network = s.network.wrapping_add(1));
    }
}
impl Source for RemoteSource {
    fn host(&self) -> HostRecord {
        self.record.clone()
    }
    fn capabilities(&self) -> Capabilities {
        self.caps.lock_ok().clone()
    }
    fn start(self: Arc<Self>, sink: SourceSink) -> std::result::Result<SourceHandle, SourceError> {
        if self.started.swap(true, Ordering::SeqCst) {
            return Err(SourceError::AlreadyStarted);
        }
        let source = self.clone();
        let thread = std::thread::Builder::new()
            .name(format!("remote-{}", self.record.id))
            .spawn(move || {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt.block_on(run(source, sink)),
                    Err(e) => (source.on_status)(ConnectionStatus {
                        error: Some(e.to_string()),
                        ..Default::default()
                    }),
                }
            })?;
        Ok(SourceHandle::new(Box::new(Control {
            source: self,
            thread: Some(thread),
        })))
    }
}
struct Control {
    source: Arc<RemoteSource>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl SourceControl for Control {
    fn set_paused(&self, _paused: bool) {}
    fn set_process_interest(&self, v: Option<u32>) {
        self.source
            .control
            .send_modify(|s| s.interest.process_period_ms = v);
    }
    fn set_network_process_interest(&self, v: bool) {
        self.source
            .control
            .send_modify(|s| s.interest.network_processes = v);
    }
    fn set_gpu_process_interest(&self, v: bool) {
        self.source
            .control
            .send_modify(|s| s.interest.gpu_processes = v);
    }
    fn set_port_process_interest(&self, v: bool) {
        self.source
            .control
            .send_modify(|s| s.interest.port_processes = v);
    }
    fn set_network_history(&self, _on: bool) {}
    fn set_detail_interest(&self, v: bool) {
        self.source.control.send_modify(|s| s.interest.detail = v);
    }
    fn apply_settings(&self, _settings: &Settings) {}
    fn set_store(&self, _store: Option<kelvo_store::Writer>) {}
    fn status(&self) -> EngineStatus {
        self.source.status.lock_ok().clone()
    }
    fn stop(&mut self) {
        self.source.control.send_modify(|s| s.stop = true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
async fn run(source: Arc<RemoteSource>, sink: SourceSink) {
    let mut control = source.control.subscribe();
    let mut delay = 1u64;
    let mut replacement = None;
    let mut status = ConnectionStatus::default();
    loop {
        if control.borrow().stop {
            break;
        }
        if control.borrow().asleep {
            let _ = control.wait_for(|s| s.stop || !s.asleep).await;
            continue;
        }
        status.connected = false;
        status.retrying = true;
        (source.on_status)(status.clone());
        let dial = async {
            match replacement.take() {
                Some(session) => Ok(session),
                None => {
                    crate::connect(&source.endpoints, source.local, Some(source.record.id)).await
                }
            }
        };
        let result = tokio::select! {
            result=dial=>result,
            _=control.wait_for(|s|s.stop)=>break,
        };
        let started = Instant::now();
        let result = match result {
            Ok(session) => {
                status.connected = true;
                status.route = Some(session.route);
                status.retrying = false;
                status.error = None;
                *source.caps.lock_ok() = session.hello.capabilities.clone();
                sink.live
                    .publish(BusMsg::Caps(Arc::new(session.hello.capabilities.clone())));
                (source.on_status)(status.clone());
                let mut stopping = source.control.subscribe();
                tokio::select! {
                    result=session_loop(&source,&sink,session,&mut control,&mut status)=>result,
                    _=stopping.wait_for(|s|s.stop||s.asleep)=>{ if source.control.borrow().stop { break; } else {Ok(None)} },
                }
            }
            Err(e) => Err(e),
        };
        if control.borrow().stop {
            break;
        }
        match result {
            Ok(Some(session)) => {
                replacement = Some(session);
                continue;
            }
            Ok(None) => {
                continue;
            }
            Err(e) => {
                status.connected = false;
                status.error = Some(e.to_string());
                status.catching_up = false;
                status.retrying = e.retryable();
                (source.on_status)(status.clone());
                if !e.retryable() {
                    let retry = control.borrow().retry;
                    let _ = control.wait_for(|s| s.stop || s.retry != retry).await;
                    continue;
                }
            }
        }
        if started.elapsed() > Duration::from_secs(30) {
            delay = 1;
        }
        let jitter = (kelvo_engine::wall_ms().unsigned_abs() % 250) as u64;
        tokio::select! {_=tokio::time::sleep(Duration::from_millis(delay*1000+jitter))=>{},_=control.changed()=>{}}
        delay = (delay * 2).min(60);
    }
    status.connected = false;
    status.retrying = false;
    status.catching_up = false;
    (source.on_status)(status);
}
async fn session_loop(
    source: &Arc<RemoteSource>,
    sink: &SourceSink,
    mut session: SshSession,
    control: &mut watch::Receiver<ControlState>,
    status: &mut ConnectionStatus,
) -> Result<Option<SshSession>> {
    if !session.negotiated.features.contains("rows.stable-ids")
        || !session.negotiated.features.contains("live.metadata")
    {
        return Err(RemoteError::Incompatible(
            "remote lacks stable row identities or live timing metadata".into(),
        ));
    }
    let epoch = session.hello.db_instance_uuid;
    let kinds = session.negotiated.sync_kinds();
    let (mut messages, task) = codec::reader_task(session.reader);
    let _reader = codec::AbortOnDrop(task);
    let initial = control.borrow_and_update().clone();
    codec::write(&mut session.writer, &Message::Interest(initial.interest)).await?;
    codec::write(
        &mut session.writer,
        &Message::Ping {
            nonce: 0,
            t0_ms: kelvo_engine::wall_ms(),
        },
    )
    .await?;
    let mut timer = tokio::time::interval(Duration::from_secs(1));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let (promotion_tx, mut promotion_rx) = tokio::sync::mpsc::channel(1);
    let mut state = Session {
        layouts: HashMap::new(),
        summary_layout: None,
        source_timeline: None,
        timeline: source.next_timeline.fetch_add(1, Ordering::Relaxed),
        retry: initial.retry,
        network: initial.network,
        last_sync: Instant::now() - Duration::from_secs(600),
        last_ping: Instant::now(),
        ping_nonce: 0,
        outstanding_ping: Some((0, Instant::now())),
        page_bytes: 16 * 1024,
        unpruned_bytes: 0,
        unpruned_pages: 0,
        tiers: [Tier::S10, Tier::M1, Tier::M15],
        next_tier: 0,
        in_flight: None,
        timed_out: HashMap::new(),
        history_available: false,
        history_failed: false,
        floors: HashMap::new(),
        restart_cursor: HashSet::new(),
        last_recent: Instant::now() - Duration::from_secs(60),
        promotion_tx,
        promotion_task: None,
        recent_pending: false,
        recent_timed_out: None,
        clock_ready: false,
        epoch,
        kinds,
        source,
        sink,
        control,
        status,
        writer: &mut session.writer,
        route: session.route,
    };
    loop {
        if state.request_sync().await? {
            continue;
        }
        let action = tokio::select! {
            _ = state.control.changed() => state.control_changed().await?,
            promoted = promotion_rx.recv(), if state.promotion_task.is_some() => state.promoted(promoted)?,
            incoming = messages.recv() => state.incoming(incoming).await?,
            _ = timer.tick() => state.tick().await?,
        };
        match action {
            SessionAction::Continue => {}
            SessionAction::Stop => return Ok(None),
            SessionAction::Replace(session) => return Ok(Some(*session)),
        }
    }
}

enum SessionAction {
    Continue,
    Stop,
    Replace(Box<SshSession>),
}
struct Session<'a> {
    layouts: HashMap<u32, Arc<FrameLayout>>,
    summary_layout: Option<Arc<FrameLayout>>,
    source_timeline: Option<u32>,
    timeline: u32,
    retry: u64,
    network: u64,
    last_sync: Instant,
    last_ping: Instant,
    ping_nonce: u64,
    outstanding_ping: Option<(u64, Instant)>,
    page_bytes: u32,
    unpruned_bytes: usize,
    unpruned_pages: usize,
    tiers: [Tier; 3],
    next_tier: usize,
    in_flight: Option<(Tier, Option<Cursor>, Instant)>,
    /// Tiers whose request timed out, and when: a late reply is dropped, not misread.
    timed_out: HashMap<Tier, Instant>,
    history_available: bool,
    history_failed: bool,
    floors: HashMap<Tier, i64>,
    restart_cursor: HashSet<Tier>,
    last_recent: Instant,
    promotion_tx: tokio::sync::mpsc::Sender<Result<SshSession>>,
    promotion_task: Option<codec::AbortOnDrop>,
    recent_pending: bool,
    recent_timed_out: Option<Instant>,
    clock_ready: bool,
    epoch: uuid::Uuid,
    kinds: SyncKinds,
    source: &'a Arc<RemoteSource>,
    sink: &'a SourceSink,
    control: &'a mut watch::Receiver<ControlState>,
    status: &'a mut ConnectionStatus,
    writer: &'a mut std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>>,
    route: Route,
}
impl Session<'_> {
    async fn request_sync(&mut self) -> Result<bool> {
        // No wire request IDs: never overlap requests for a tier awaiting a late reply.
        while self.next_tier < self.tiers.len()
            && self.timed_out.contains_key(&self.tiers[self.next_tier])
        {
            self.next_tier += 1;
        }
        if self.clock_ready
            && self.source.clock.now_ms().is_some()
            && self.history_available
            && !self.history_failed
            && !self.recent_pending
            && self.in_flight.is_none()
            && self.next_tier < self.tiers.len()
            && let Some(mirror) = &self.source.mirror
        {
            let tier = self.tiers[self.next_tier];
            let now = self
                .source
                .clock
                .now_ms()
                .unwrap_or_else(kelvo_engine::wall_ms);
            let after = if self.restart_cursor.remove(&tier) {
                None
            } else {
                match mirror.cursor(tier, self.epoch, self.kinds) {
                    Ok(cursor) => cursor,
                    Err(error) => {
                        self.history_failed = true;
                        self.status.history_error = Some(error.to_string());
                        self.status.catching_up = false;
                        (self.source.on_status)(self.status.clone());
                        return Ok(true);
                    }
                }
            };
            let floor = mirror
                .history_floor(tier, now)
                .max(self.floors.get(&tier).copied().unwrap_or(i64::MIN));
            codec::write(
                &mut self.writer,
                &Message::SyncRequest(SyncRequest {
                    tier,
                    after,
                    max_rows: 2000,
                    from_ms: Some(floor),
                    max_bytes: Some(self.page_bytes),
                }),
            )
            .await?;
            self.in_flight = Some((tier, after, Instant::now()));
            self.status.catching_up = true;
            (self.source.on_status)(self.status.clone());
            if self.next_tier == 0 {
                self.last_sync = Instant::now();
            }
        }

        Ok(false)
    }
    async fn control_changed(&mut self) -> Result<SessionAction> {
        let next = self.control.borrow_and_update().clone();
        if next.stop || next.asleep {
            return Ok(SessionAction::Stop);
        }
        if next.retry != self.retry {
            return Ok(SessionAction::Stop);
        }
        self.retry = next.retry;
        if next.network != self.network && self.route == Route::Tailscale {
            self.network = next.network;
            if let Some(lan) = &self.source.endpoints.lan {
                let config = EndpointConfig {
                    lan: Some(lan.clone()),
                    tailscale: None,
                    helper_path: self.source.endpoints.helper_path.clone(),
                };
                // Debounced, cancellable promotion runs beside the old stream.
                let tx = self.promotion_tx.clone();
                let local = self.source.local;
                let expected = self.source.record.id;
                self.promotion_task = Some(codec::AbortOnDrop(tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    let result = crate::connect(&config, local, Some(expected)).await;
                    let _ = tx.send(result).await;
                })));
            }
        }
        codec::write(&mut self.writer, &Message::Interest(next.interest)).await?;
        Ok(SessionAction::Continue)
    }
    fn promoted(&mut self, promoted: Option<Result<SshSession>>) -> Result<SessionAction> {
        self.promotion_task = None;
        if let Some(Ok(replacement)) = promoted {
            return Ok(SessionAction::Replace(Box::new(replacement)));
        }
        Ok(SessionAction::Continue)
    }
    #[expect(
        clippy::too_many_lines,
        reason = "service split tracked in architecture refactor package G"
    )]
    async fn incoming(&mut self, incoming: Option<Result<Message>>) -> Result<SessionAction> {
        let message =
            incoming.ok_or_else(|| RemoteError::Transport("remote stream closed".into()))??;
        match message {
            Message::LayoutDef(layout) => {
                if layout.series.len() > 8192 || self.layouts.len() >= 1024 {
                    return Err(RemoteError::Protocol("layout cardinality exceeded".into()));
                }
                if self.layouts.contains_key(&layout.layout_no) {
                    return Err(RemoteError::Protocol(
                        "layout number reused in session".into(),
                    ));
                }
                let remote_no = layout.layout_no;
                let layout = Arc::new(FrameLayout {
                    layout_no: self.source.next_layout.fetch_add(1, Ordering::Relaxed),
                    series: layout.series.into(),
                });
                self.layouts.insert(remote_no, layout.clone());
                self.sink.live.publish(BusMsg::Layout(layout));
            }
            Message::Live(frame) => {
                let layout = self
                    .layouts
                    .get(&frame.layout_no)
                    .ok_or_else(|| RemoteError::Protocol("unknown layout".into()))?
                    .clone();
                let n = layout.series.len();
                if frame.values.len() != n
                    || frame.held.len() != n
                    || frame.holds.len() != n
                    || frame.interval_ms == 0
                {
                    return Err(RemoteError::Protocol(
                        "invalid live frame dimensions or cadence".into(),
                    ));
                }
                if self.source_timeline.is_some_and(|t| t != frame.timeline) {
                    self.timeline = self.source.next_timeline.fetch_add(1, Ordering::Relaxed);
                    if self.outstanding_ping.is_none() {
                        self.ping_nonce = self.ping_nonce.wrapping_add(1);
                        codec::write(
                            &mut self.writer,
                            &Message::Ping {
                                nonce: self.ping_nonce,
                                t0_ms: kelvo_engine::wall_ms(),
                            },
                        )
                        .await?;
                        self.outstanding_ping = Some((self.ping_nonce, Instant::now()));
                        self.last_ping = Instant::now();
                    }
                }
                self.source_timeline = Some(frame.timeline);
                self.status.last_seen_ms = Some(kelvo_engine::wall_ms());
                self.sink.live.publish(BusMsg::Frame(Arc::new(LiveFrame {
                    ts_ms: frame.ts_ms,
                    interval_ms: frame.interval_ms,
                    layout,
                    timeline: self.timeline,
                    values: frame.values.into(),
                    held: frame.held.into(),
                    holds: frame.holds.into(),
                })));
                (self.source.on_status)(self.status.clone());
            }
            Message::HostSummary(summary) => {
                if summary.headline.len() > 8192 {
                    return Err(RemoteError::Protocol("summary too large".into()));
                }
                self.status.last_seen_ms = Some(kelvo_engine::wall_ms());
                let series: Arc<[_]> = summary.headline.iter().map(|(k, _)| k.clone()).collect();
                let layout = match &self.summary_layout {
                    Some(layout) if layout.series == series => layout.clone(),
                    _ => {
                        let layout = Arc::new(FrameLayout {
                            layout_no: self.source.next_layout.fetch_add(1, Ordering::Relaxed),
                            series,
                        });
                        self.summary_layout = Some(layout.clone());
                        self.sink.live.publish(BusMsg::Layout(layout.clone()));
                        layout
                    }
                };
                let held: Arc<[f32]> = summary.headline.iter().map(|(_, v)| *v).collect();
                self.sink.live.publish(BusMsg::Frame(Arc::new(LiveFrame {
                    ts_ms: summary.ts_ms,
                    interval_ms: 5000,
                    timeline: self.timeline,
                    holds: vec![12500; held.len()].into(),
                    values: vec![f32::NAN; held.len()].into(),
                    held,
                    layout,
                })));
                (self.source.on_status)(self.status.clone());
            }
            Message::SourceStatus(wire) => {
                self.history_available = wire.history_available;
                if !self.history_available {
                    self.status.history_error =
                        Some("Remote history is disabled or unavailable".into());
                } else if self.status.history_error.as_deref()
                    == Some("Remote history is disabled or unavailable")
                {
                    self.status.history_error = None;
                }
                let s = server::status_from_wire(wire);
                *self.source.status.lock_ok() = s.clone();
                self.sink.live.publish(BusMsg::Status(s));
                (self.source.on_status)(self.status.clone());
            }
            Message::CapabilitiesChanged(c) => {
                *self.source.caps.lock_ok() = c.clone();
                self.sink.live.publish(BusMsg::Caps(Arc::new(c)));
            }
            Message::LiveProcesses(p) => {
                if p.rows.len() > 32768 {
                    return Err(RemoteError::Protocol("process row limit exceeded".into()));
                }
                self.sink
                    .live
                    .publish(BusMsg::Processes(Arc::new(ProcessBatch {
                        ts_ms: p.ts_ms,
                        rows: p.rows.into_iter().map(server::process_from_wire).collect(),
                        gpu_span_ms: p.gpu_span_ms,
                    })));
            }
            Message::Pong {
                nonce, t_remote_ms, ..
            } => {
                if let Some((expected, sent)) = self.outstanding_ping.take() {
                    if nonce != expected {
                        return Err(RemoteError::Protocol("heartbeat nonce mismatch".into()));
                    }
                    let rtt = sent.elapsed().as_millis().min(10_000) as i64;
                    self.source
                        .clock
                        .observe(t_remote_ms.saturating_add(rtt / 2));
                    self.clock_ready = true;
                    self.status.clock_offset_ms = self
                        .source
                        .clock
                        .now_ms()
                        .unwrap_or(t_remote_ms)
                        .saturating_sub(kelvo_engine::wall_ms());
                    (self.source.on_status)(self.status.clone());
                }
            }
            Message::SyncPage(page) => {
                if self.timed_out.remove(&page.tier).is_some() {
                    tracing::debug!(tier = ?page.tier, "dropping late sync page");
                    return Ok(SessionAction::Continue);
                }
                if !self
                    .in_flight
                    .as_ref()
                    .is_some_and(|(tier, _, _)| *tier == page.tier)
                {
                    return Err(RemoteError::Transport("unsolicited sync page".into()));
                }
                let (tier, after, requested) = self
                    .in_flight
                    .take()
                    .ok_or_else(|| RemoteError::Transport("unsolicited sync page".into()))?;
                if page.epoch != self.epoch {
                    return Err(RemoteError::Transport(
                        "source history was reset; reconnecting".into(),
                    ));
                }
                if page.tier != tier
                    || page.last_seq < after.map_or(0, |c| c.seq)
                    || (page.more && page.last_seq == after.map_or(0, |c| c.seq))
                {
                    return Err(RemoteError::Protocol(
                        "sync cursor did not advance or epoch changed".into(),
                    ));
                }
                let received_bytes =
                    kelvo_proto::encode_message(&Message::SyncPage(page.clone()))?.len();
                if received_bytes > 256 * 1024 {
                    return Err(RemoteError::Protocol("oversized history page".into()));
                }
                let more = page.more;
                if let Some(mirror) = self.source.mirror.clone() {
                    self.unpruned_bytes = self.unpruned_bytes.saturating_add(received_bytes);
                    self.unpruned_pages += 1;
                    let prune = self.unpruned_bytes >= 1024 * 1024 || self.unpruned_pages >= 16;
                    if prune {
                        self.unpruned_bytes = 0;
                        self.unpruned_pages = 0;
                    }
                    let now = self
                        .source
                        .clock
                        .now_ms()
                        .unwrap_or_else(kelvo_engine::wall_ms);
                    let kinds = self.kinds;
                    let result = tokio::task::spawn_blocking(move || {
                        let result = mirror
                            .ingest(page, kinds)
                            .and_then(|()| mirror.enforce_budget(now));
                        if prune || result.is_err() {
                            mirror.prune(now)?;
                        }
                        result
                    })
                    .await
                    .map_err(|e| RemoteError::Transport(e.to_string()))?;
                    if let Err(error) = result {
                        self.history_failed = true;
                        self.next_tier = self.tiers.len();
                        self.last_sync = Instant::now();
                        self.status.history_error = Some(error.to_string());
                        self.status.catching_up = false;
                        (self.source.on_status)(self.status.clone());
                    } else {
                        self.status.history_error = None;
                    }
                }
                if !more {
                    self.next_tier += 1;
                }
                self.page_bytes = adapt_page_bytes(self.page_bytes, requested.elapsed());
                tokio::task::yield_now().await;
            }
            Message::Truncated {
                tier,
                earliest_ts_ms,
                epoch: remote_epoch,
            } => {
                if self.timed_out.remove(&tier).is_some() {
                    tracing::debug!(?tier, "dropping late truncation");
                    return Ok(SessionAction::Continue);
                }
                if !self
                    .in_flight
                    .as_ref()
                    .is_some_and(|(expected, _, _)| *expected == tier)
                {
                    return Err(RemoteError::Transport("unsolicited truncation".into()));
                }
                let expected = self
                    .in_flight
                    .take()
                    .ok_or_else(|| RemoteError::Transport("unsolicited truncation".into()))?;
                if remote_epoch != self.epoch {
                    return Err(RemoteError::Transport(
                        "source history was reset; reconnecting".into(),
                    ));
                }
                if tier != expected.0 {
                    return Err(RemoteError::Protocol("invalid truncation".into()));
                }
                if let Some(mirror) = &self.source.mirror
                    && let Err(error) = mirror.truncated(tier, self.epoch, earliest_ts_ms)
                {
                    self.history_failed = true;
                    self.status.history_error = Some(error.to_string());
                }
                self.floors.insert(tier, earliest_ts_ms);
                self.restart_cursor.insert(tier);
            }
            Message::RecentRows {
                epoch: remote_epoch,
                tier,
                complete_to_ms,
                layouts,
                rows,
            } => {
                if self.recent_timed_out.take().is_some() {
                    tracing::debug!("dropping late recent snapshot");
                    return Ok(SessionAction::Continue);
                }
                if !self.recent_pending {
                    return Err(RemoteError::Transport("unsolicited recent snapshot".into()));
                }
                self.recent_pending = false;
                if remote_epoch != self.epoch {
                    return Err(RemoteError::Transport(
                        "source history was reset; reconnecting".into(),
                    ));
                }
                if let Some(mirror) = self.source.mirror.clone()
                    && let Err(error) =
                        mirror.recent(remote_epoch, tier, complete_to_ms, layouts, rows)
                {
                    self.status.history_error = Some(error.to_string());
                }
            }
            Message::Error { message, .. } => {
                if !self.timed_out.is_empty() || self.recent_timed_out.is_some() {
                    // Errors have no tier either, so reconnect rather than misattribute one.
                    return Err(RemoteError::Transport(message));
                }
                if self.recent_pending {
                    self.recent_pending = false;
                } else {
                    self.history_failed = true;
                    self.in_flight = None;
                    self.next_tier = self.tiers.len();
                    self.last_sync = Instant::now();
                }
                self.status.history_error = Some(message);
                self.status.catching_up = false;
                (self.source.on_status)(self.status.clone());
            }
            Message::Unknown => {}
            _ => return Err(RemoteError::Protocol("unexpected server message".into())),
        }
        Ok(SessionAction::Continue)
    }
    #[expect(
        clippy::too_many_lines,
        reason = "service split tracked in architecture refactor package G"
    )]
    async fn tick(&mut self) -> Result<SessionAction> {
        // A reply that never arrives would otherwise park its tier (or recent history)
        // for the rest of the session; reconnecting resynchronizes the stream.
        let lost = Duration::from_secs(120);
        if self.timed_out.values().any(|at| at.elapsed() > lost)
            || self.recent_timed_out.is_some_and(|at| at.elapsed() > lost)
        {
            return Err(RemoteError::Transport(
                "a history reply never arrived; reconnecting".into(),
            ));
        }
        if self
            .outstanding_ping
            .is_some_and(|(_, sent)| sent.elapsed() > Duration::from_secs(30))
        {
            return Err(RemoteError::Transport(
                "remote application heartbeat timed out".into(),
            ));
        }
        if self.outstanding_ping.is_none() && self.last_ping.elapsed() >= Duration::from_secs(15) {
            self.ping_nonce = self.ping_nonce.wrapping_add(1);
            codec::write(
                &mut self.writer,
                &Message::Ping {
                    nonce: self.ping_nonce,
                    t0_ms: kelvo_engine::wall_ms(),
                },
            )
            .await?;
            self.last_ping = Instant::now();
            self.outstanding_ping = Some((self.ping_nonce, self.last_ping));
        }
        if self.recent_pending && self.last_recent.elapsed() > Duration::from_secs(30) {
            self.recent_pending = false;
            self.recent_timed_out = Some(Instant::now());
            self.status.history_error = Some("recent history request timed out".into());
        }
        if self
            .in_flight
            .as_ref()
            .is_some_and(|(_, _, at)| at.elapsed() > Duration::from_secs(30))
        {
            self.history_failed = true;
            if let Some((tier, _, _)) = self.in_flight.take() {
                self.timed_out.insert(tier, Instant::now());
            }
            self.next_tier = self.tiers.len();
            self.last_sync = Instant::now();
            self.status.history_error = Some("history request timed out".into());
            self.status.catching_up = false;
            (self.source.on_status)(self.status.clone());
        }
        let period = if self.source.controller_battery.load(Ordering::Relaxed) {
            300
        } else {
            60
        };
        if self.in_flight.is_none() && self.next_tier >= self.tiers.len() {
            if self.status.catching_up {
                if let Some(mirror) = self.source.mirror.clone() {
                    let now = self
                        .source
                        .clock
                        .now_ms()
                        .unwrap_or_else(kelvo_engine::wall_ms);
                    if let Err(error) = tokio::task::spawn_blocking(move || mirror.prune(now))
                        .await
                        .map_err(|e| RemoteError::Transport(e.to_string()))?
                    {
                        self.status.history_error = Some(error.to_string());
                    }
                }
                self.status.catching_up = false;
                (self.source.on_status)(self.status.clone());
            }
            if self.last_sync.elapsed() >= Duration::from_secs(period) {
                self.next_tier = 0;
                self.history_failed = false;
            }
        }
        if self.history_available
            && self.control.borrow().interest.detail
            && self.last_recent.elapsed() >= Duration::from_secs(30)
            && self.in_flight.is_none()
            && !self.recent_pending
            && self.recent_timed_out.is_none()
            && self.next_tier >= self.tiers.len()
        {
            let now = self
                .source
                .clock
                .now_ms()
                .unwrap_or_else(kelvo_engine::wall_ms);
            codec::write(
                &mut self.writer,
                &Message::RecentRequest {
                    from_ms: now.saturating_sub(300_000),
                    to_ms: now,
                },
            )
            .await?;
            self.last_recent = Instant::now();
            self.recent_pending = true;
        }

        Ok(SessionAction::Continue)
    }
}

fn adapt_page_bytes(current: u32, elapsed: Duration) -> u32 {
    if elapsed > Duration::from_millis(500) {
        (current / 2).max(1024)
    } else if elapsed < Duration::from_millis(100) {
        current.saturating_mul(2).min(256 * 1024)
    } else {
        current
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_source(mirror: Option<Arc<dyn MirrorSink>>) -> Arc<RemoteSource> {
        use crate::ServerProvider;
        let hello = crate::server::tests::provider().hello();
        RemoteSource::new(
            HostRecord::from_identity(hello.host, false),
            HostId(uuid::Uuid::nil()),
            EndpointConfig {
                lan: Some("fixture".into()),
                tailscale: None,
                helper_path: String::new(),
            },
            Arc::new(|_| {}),
            mirror,
        )
    }
    fn fixture_session<'a>(
        source: &'a Arc<RemoteSource>,
        sink: &'a SourceSink,
        control: &'a mut watch::Receiver<ControlState>,
        status: &'a mut ConnectionStatus,
        writer: &'a mut std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>>,
    ) -> Session<'a> {
        Session {
            layouts: HashMap::new(),
            summary_layout: None,
            source_timeline: None,
            timeline: 1,
            retry: 0,
            network: 0,
            last_sync: Instant::now(),
            last_ping: Instant::now(),
            ping_nonce: 0,
            outstanding_ping: None,
            page_bytes: 16384,
            unpruned_bytes: 0,
            unpruned_pages: 0,
            tiers: [Tier::S10, Tier::M1, Tier::M15],
            next_tier: 0,
            in_flight: None,
            timed_out: HashMap::new(),
            history_available: true,
            history_failed: false,
            floors: HashMap::new(),
            restart_cursor: HashSet::new(),
            last_recent: Instant::now(),
            promotion_tx: tokio::sync::mpsc::channel(1).0,
            promotion_task: None,
            recent_pending: false,
            recent_timed_out: None,
            clock_ready: true,
            epoch: uuid::Uuid::from_u128(2),
            kinds: SyncKinds::default(),
            source,
            sink,
            control,
            status,
            writer,
            route: Route::Lan,
        }
    }
    struct CountingMirror(AtomicU32);
    impl MirrorSink for CountingMirror {
        fn cursor(
            &self,
            _: Tier,
            _: uuid::Uuid,
            _: SyncKinds,
        ) -> std::result::Result<Option<Cursor>, MirrorError> {
            Ok(None)
        }
        fn ingest(&self, _: SyncPage, _: SyncKinds) -> std::result::Result<(), MirrorError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    fn page(tier: Tier) -> Message {
        Message::SyncPage(SyncPage {
            tier,
            epoch: uuid::Uuid::from_u128(2),
            layouts: vec![],
            rows: vec![],
            gaps: vec![],
            events: vec![],
            last_seq: 1,
            more: false,
        })
    }
    #[tokio::test(start_paused = true)]
    async fn late_history_replies_are_dropped_and_next_sync_succeeds() {
        let mirror = Arc::new(CountingMirror(AtomicU32::new(0)));
        let source = fixture_source(Some(mirror.clone()));
        source.clock.observe(9_000_000);
        let sink = SourceSink {
            live: Default::default(),
            store: None,
        };
        let mut control = source.control.subscribe();
        let mut status = ConnectionStatus::default();
        let (client, mut peer) = tokio::io::duplex(65536);
        let (mut reader, writer) = tokio::io::split(client);
        let mut writer: std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>> = Box::pin(writer);
        let mut session = fixture_session(&source, &sink, &mut control, &mut status, &mut writer);
        session.request_sync().await.unwrap();
        assert!(
            matches!(codec::read(&mut peer).await.unwrap(), Message::SyncRequest(r) if r.tier == Tier::S10)
        );
        tokio::time::advance(Duration::from_secs(31)).await;
        session.tick().await.unwrap();
        codec::write(&mut peer, &page(Tier::S10)).await.unwrap();
        let reply = codec::read(&mut reader).await;
        session.incoming(Some(reply)).await.unwrap();
        assert_eq!(
            mirror.0.load(Ordering::SeqCst),
            0,
            "late page must not be ingested"
        );
        tokio::time::advance(Duration::from_secs(60)).await;
        session.outstanding_ping = None;
        session.last_ping = Instant::now();
        session.tick().await.unwrap();
        session.request_sync().await.unwrap();
        // The timeout tick also sent a heartbeat; drain it before the next request.
        assert!(matches!(
            codec::read(&mut peer).await.unwrap(),
            Message::Ping { .. }
        ));
        assert!(
            matches!(codec::read(&mut peer).await.unwrap(), Message::SyncRequest(r) if r.tier == Tier::S10)
        );
        codec::write(&mut peer, &page(Tier::S10)).await.unwrap();
        let reply = codec::read(&mut reader).await;
        session.incoming(Some(reply)).await.unwrap();
        assert_eq!(mirror.0.load(Ordering::SeqCst), 1);
        assert!(!session.history_failed);
    }
    #[tokio::test(start_paused = true)]
    async fn a_reply_that_never_arrives_reconnects_instead_of_parking_the_tier() {
        let source = fixture_source(None);
        let sink = SourceSink {
            live: Default::default(),
            store: None,
        };
        let mut control = source.control.subscribe();
        let mut status = ConnectionStatus::default();
        let mut writer: std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>> =
            Box::pin(tokio::io::sink());
        let mut session = fixture_session(&source, &sink, &mut control, &mut status, &mut writer);
        session.in_flight = Some((Tier::M1, None, Instant::now()));
        tokio::time::advance(Duration::from_secs(31)).await;
        session.tick().await.unwrap();
        session.outstanding_ping = None;
        tokio::time::advance(Duration::from_secs(60)).await;
        session.last_ping = Instant::now();
        session.tick().await.unwrap();
        tokio::time::advance(Duration::from_secs(61)).await;
        session.outstanding_ping = None;
        session.last_ping = Instant::now();
        let error = session
            .tick()
            .await
            .err()
            .expect("lost reply must reconnect");
        assert!(error.retryable(), "{error}");
    }
    #[tokio::test(start_paused = true)]
    async fn late_truncation_and_recent_snapshot_are_dropped_but_unsolicited_replies_retry() {
        let source = fixture_source(None);
        let sink = SourceSink {
            live: Default::default(),
            store: None,
        };
        let mut control = source.control.subscribe();
        let mut status = ConnectionStatus::default();
        let mut writer: std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>> =
            Box::pin(tokio::io::sink());
        let mut session = fixture_session(&source, &sink, &mut control, &mut status, &mut writer);
        session.in_flight = Some((Tier::M1, None, Instant::now()));
        session.recent_pending = true;
        tokio::time::advance(Duration::from_secs(31)).await;
        session.tick().await.unwrap();
        let truncated = Message::Truncated {
            tier: Tier::M1,
            earliest_ts_ms: 1,
            epoch: session.epoch,
        };
        session.incoming(Some(Ok(truncated.clone()))).await.unwrap();
        assert!(session.floors.is_empty());
        let recent = Message::RecentRows {
            epoch: session.epoch,
            tier: Tier::S10,
            complete_to_ms: 0,
            layouts: vec![],
            rows: vec![],
        };
        session.incoming(Some(Ok(recent.clone()))).await.unwrap();
        for reply in [truncated, recent, page(Tier::S10)] {
            let error = session.incoming(Some(Ok(reply))).await.err().unwrap();
            assert!(matches!(error, RemoteError::Transport(_)));
            assert!(error.retryable());
        }
    }
    #[tokio::test]
    async fn summary_layout_reuses_arc_and_announces_changed_label_keys_before_frame() {
        use kelvo_schema::{MetricId, SeriesKey};
        let source = fixture_source(None);
        let sink = SourceSink {
            live: Default::default(),
            store: None,
        };
        let mut events = sink.live.subscribe();
        let mut control = source.control.subscribe();
        let mut status = ConnectionStatus::default();
        let mut writer: std::pin::Pin<Box<dyn tokio::io::AsyncWrite + Send>> =
            Box::pin(tokio::io::sink());
        let mut session = fixture_session(&source, &sink, &mut control, &mut status, &mut writer);
        let key = SeriesKey::new(
            MetricId::from_static("net.rx"),
            kelvo_schema::Labels::single("iface", "en0"),
        );
        let summary = |ts_ms, headline| {
            Message::HostSummary(kelvo_proto::HostSummary {
                ts_ms,
                online: true,
                headline,
            })
        };
        session
            .incoming(Some(Ok(summary(1, vec![(key.clone(), 10.0)]))))
            .await
            .unwrap();
        let Some(BusMsg::Layout(first)) = events.recv().await else {
            panic!("layout must precede frame")
        };
        let Some(BusMsg::Frame(frame)) = events.recv().await else {
            panic!("expected frame")
        };
        assert!(Arc::ptr_eq(&first, &frame.layout));
        session
            .incoming(Some(Ok(summary(5001, vec![(key.clone(), 20.0)]))))
            .await
            .unwrap();
        let Some(BusMsg::Frame(frame)) = events.recv().await else {
            panic!("same keys must reuse layout")
        };
        assert!(
            Arc::ptr_eq(&first, &frame.layout),
            "same summaries must occupy one ring segment"
        );
        let segments = sink.live.backfill(0);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].rows.len(), 2);
        let mut vpn = key.clone();
        vpn.labels = kelvo_schema::Labels::single("iface", "utun0");
        session
            .incoming(Some(Ok(summary(
                10001,
                vec![(key.clone(), 30.0), (vpn.clone(), 40.0)],
            ))))
            .await
            .unwrap();
        let Some(BusMsg::Layout(next)) = events.recv().await else {
            panic!("changed keys need a Layout before the frame")
        };
        assert_ne!(first.layout_no, next.layout_no);
        let Some(BusMsg::Frame(frame)) = events.recv().await else {
            panic!("expected frame")
        };
        assert!(Arc::ptr_eq(&next, &frame.layout));
        assert_eq!(&*frame.layout.series, &[key, vpn]);
        assert_eq!(&*frame.held, &[30.0, 40.0]);
    }
    #[tokio::test(start_paused = true)]
    async fn an_authenticated_but_stalled_application_times_out_and_closes_its_stream() {
        use crate::ServerProvider;
        use tokio::io::AsyncReadExt;

        let hello = crate::server::tests::provider().hello();
        let source = RemoteSource::new(
            HostRecord::from_identity(hello.host.clone(), false),
            HostId(uuid::Uuid::nil()),
            EndpointConfig {
                lan: Some("fixture".into()),
                tailscale: None,
                helper_path: String::new(),
            },
            Arc::new(|_| {}),
            None,
        );
        let (client, mut application) = tokio::io::duplex(65536);
        let session = SshSession::test(client, hello);
        let live = kelvo_engine::LiveHub::default();
        let mut events = live.subscribe();
        let sink = SourceSink { live, store: None };
        let runner = source.clone();
        let task = tokio::spawn(async move {
            let mut control = runner.control.subscribe();
            session_loop(
                &runner,
                &sink,
                session,
                &mut control,
                &mut ConnectionStatus::default(),
            )
            .await
        });

        assert!(matches!(
            codec::read(&mut application).await.unwrap(),
            Message::Interest(_)
        ));
        let Message::Ping { nonce, t0_ms } = codec::read(&mut application).await.unwrap() else {
            panic!("a connected source must check application liveness");
        };
        codec::write(
            &mut application,
            &Message::Pong {
                nonce,
                t0_ms,
                t_remote_ms: 9_000_000,
            },
        )
        .await
        .unwrap();
        codec::write(
            &mut application,
            &Message::LayoutDef(WireLayout {
                layout_no: 0,
                series: vec![kelvo_schema::SeriesKey::bare(
                    kelvo_schema::MetricId::from_static("cpu.total"),
                )],
            }),
        )
        .await
        .unwrap();
        codec::write(
            &mut application,
            &Message::Live(kelvo_proto::LiveFrame {
                ts_ms: 9_000_000,
                layout_no: 0,
                values: vec![42.0],
                held: vec![42.0],
                interval_ms: 1000,
                timeline: 0,
                holds: vec![2500],
            }),
        )
        .await
        .unwrap();
        // An observed sample is the barrier: the first Pong and its preceding bytes
        // have actually crossed the codec/client loop before virtual time moves.
        loop {
            if let Some(BusMsg::Frame(frame)) = events.recv().await {
                assert_eq!(&*frame.values, &[42.0]);
                break;
            }
        }
        tokio::time::advance(Duration::from_secs(15)).await;
        let Message::Ping { nonce: next, .. } = codec::read(&mut application).await.unwrap() else {
            panic!("the client must recheck liveness after a healthy interval");
        };
        assert_ne!(next, nonce);
        // SSH/the byte stream remains open, but the application no longer answers.
        // Use virtual runtime time so this exercises the real thirty-second policy
        // without adding thirty seconds to every verification run.
        tokio::time::advance(Duration::from_secs(29)).await;
        tokio::task::yield_now().await;
        assert!(
            !task.is_finished(),
            "a heartbeat may use its full response window"
        );
        tokio::time::advance(Duration::from_secs(2)).await;
        let outcome = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("missing application heartbeat must terminate the session")
            .unwrap();
        assert!(
            matches!(outcome, Err(RemoteError::Transport(message)) if message.contains("application heartbeat timed out"))
        );
        let eof = tokio::time::timeout(Duration::from_secs(1), application.read_u8())
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(eof.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn catchup_continues_immediately_after_pages_and_stale_samples_do_not_freeze_clock() {
        use crate::ServerProvider;
        struct Mirror {
            cursors: Mutex<HashMap<Tier, Cursor>>,
            pages: AtomicU32,
            budgets: AtomicU32,
            truncated: AtomicU32,
        }
        impl MirrorSink for Mirror {
            fn cursor(
                &self,
                tier: Tier,
                _epoch: uuid::Uuid,
                _kinds: SyncKinds,
            ) -> std::result::Result<Option<Cursor>, MirrorError> {
                Ok(self.cursors.lock_ok().get(&tier).copied())
            }
            fn ingest(
                &self,
                page: SyncPage,
                _kinds: SyncKinds,
            ) -> std::result::Result<(), MirrorError> {
                self.cursors.lock_ok().insert(
                    page.tier,
                    Cursor {
                        epoch: page.epoch,
                        seq: page.last_seq,
                    },
                );
                self.pages.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            fn truncated(
                &self,
                _tier: Tier,
                _epoch: uuid::Uuid,
                _earliest: i64,
            ) -> std::result::Result<(), MirrorError> {
                self.truncated.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            fn enforce_budget(&self, _now: i64) -> std::result::Result<(), MirrorError> {
                self.budgets.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
        let hello = crate::server::tests::provider().hello();
        let epoch = hello.db_instance_uuid;
        let mirror = Arc::new(Mirror {
            cursors: Mutex::new(HashMap::new()),
            pages: AtomicU32::new(0),
            budgets: AtomicU32::new(0),
            truncated: AtomicU32::new(0),
        });
        let source = RemoteSource::new(
            HostRecord::from_identity(hello.host.clone(), false),
            HostId(uuid::Uuid::nil()),
            EndpointConfig {
                lan: Some("test".into()),
                tailscale: None,
                helper_path: String::new(),
            },
            Arc::new(|_| {}),
            Some(mirror.clone()),
        );
        let (client, mut peer) = tokio::io::duplex(1024 * 1024);
        let session = SshSession::test(client, hello);
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let remote = tokio::spawn(async move {
            codec::write(
                &mut peer,
                &Message::SourceStatus(kelvo_proto::SourceStatus {
                    history_available: true,
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
            let mut truncated = false;
            let mut seen = 0;
            loop {
                match codec::read(&mut peer).await.unwrap() {
                    Message::Ping { nonce, t0_ms } => {
                        codec::write(
                            &mut peer,
                            &Message::Pong {
                                nonce,
                                t0_ms,
                                t_remote_ms: 9_000_000,
                            },
                        )
                        .await
                        .unwrap();
                        codec::write(
                            &mut peer,
                            &Message::HostSummary(kelvo_proto::HostSummary {
                                ts_ms: 1,
                                online: true,
                                headline: vec![],
                            }),
                        )
                        .await
                        .unwrap();
                    }
                    Message::SyncRequest(request) => {
                        if !truncated {
                            truncated = true;
                            codec::write(
                                &mut peer,
                                &Message::Truncated {
                                    tier: request.tier,
                                    earliest_ts_ms: 100,
                                    epoch,
                                },
                            )
                            .await
                            .unwrap();
                            continue;
                        }
                        if seen == 0 {
                            assert!(request.after.is_none());
                            assert!(request.from_ms.unwrap() >= 100);
                        }
                        let last = request.after.map_or(0, |c| c.seq) + 1;
                        codec::write(
                            &mut peer,
                            &Message::SyncPage(SyncPage {
                                tier: request.tier,
                                epoch,
                                layouts: vec![],
                                rows: vec![],
                                gaps: vec![],
                                events: vec![],
                                last_seq: last,
                                more: last < 4,
                            }),
                        )
                        .await
                        .unwrap();
                        seen += 1;
                        if seen == 12 {
                            let _ = done_tx.send(());
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            break;
                        }
                    }
                    Message::Interest(_) => {}
                    message => panic!("unexpected {message:?}"),
                }
            }
        });
        let runner = source.clone();
        let sink = SourceSink {
            live: Default::default(),
            store: None,
        };
        let task = tokio::spawn(async move {
            let mut control = runner.control.subscribe();
            let mut status = ConnectionStatus::default();
            session_loop(&runner, &sink, session, &mut control, &mut status).await
        });
        tokio::time::timeout(Duration::from_secs(2), done_rx)
            .await
            .unwrap()
            .unwrap();
        for _ in 0..30 {
            if mirror.pages.load(Ordering::SeqCst) == 12 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(mirror.pages.load(Ordering::SeqCst), 12);
        assert_eq!(mirror.budgets.load(Ordering::SeqCst), 12);
        assert_eq!(mirror.truncated.load(Ordering::SeqCst), 1);
        assert!(
            source.clock.now_ms().unwrap() >= 9_000_000,
            "stale summary must not replace Pong wall clock"
        );
        source.control.send_modify(|state| state.stop = true);
        assert!(
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .is_none()
        );
        remote.abort();
    }
    #[test]
    fn page_targets_shrink_for_slow_links_and_stay_bounded() {
        assert_eq!(
            adapt_page_bytes(16 * 1024, Duration::from_secs(2)),
            8 * 1024
        );
        assert_eq!(adapt_page_bytes(1024, Duration::from_secs(2)), 1024);
        assert_eq!(
            adapt_page_bytes(16 * 1024, Duration::from_millis(30)),
            32 * 1024
        );
        assert_eq!(
            adapt_page_bytes(256 * 1024, Duration::from_millis(30)),
            256 * 1024
        );
    }
}

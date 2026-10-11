use crate::{EndpointConfig, HostClock, RemoteError, Result, Route, SshSession, codec, server};
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
    time::{Duration, Instant},
};
use tokio::sync::watch;

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
/// The shell owns the isolated mirror and its reset generation. Methods must reject
/// writes after that mirror was detached/reset. Failed sync never stops live metrics.
pub trait MirrorSink: Send + Sync + 'static {
    fn cursor(
        &self,
        tier: Tier,
        epoch: uuid::Uuid,
        kinds: SyncKinds,
    ) -> std::result::Result<Option<Cursor>, String>;
    fn ingest(&self, page: SyncPage, kinds: SyncKinds) -> std::result::Result<(), String>;
    fn recent(
        &self,
        _epoch: uuid::Uuid,
        _tier: Tier,
        _complete_to_ms: i64,
        _layouts: Vec<WireLayout>,
        _rows: Vec<WireBucket>,
    ) -> std::result::Result<(), String> {
        Ok(())
    }
    fn truncated(
        &self,
        _tier: Tier,
        _epoch: uuid::Uuid,
        _earliest_ms: i64,
    ) -> std::result::Result<(), String> {
        Ok(())
    }
    fn enforce_budget(&self, _now_ms: i64) -> std::result::Result<(), String> {
        Ok(())
    }
    fn prune(&self, _now_ms: i64) -> std::result::Result<(), String> {
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
    let mut layouts = HashMap::<u32, Arc<FrameLayout>>::new();
    let mut source_timeline = None;
    let mut timeline = source.next_timeline.fetch_add(1, Ordering::Relaxed);
    let initial = control.borrow_and_update().clone();
    let mut retry = initial.retry;
    let mut network = initial.network;
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
    let mut last_sync = Instant::now() - Duration::from_secs(600);
    let mut last_ping = Instant::now();
    let mut ping_nonce = 0u64;
    let mut outstanding_ping = Some((0u64, Instant::now()));
    let mut page_bytes = 16 * 1024;
    let mut unpruned_bytes = 0usize;
    let mut unpruned_pages = 0usize;
    let tiers = [Tier::S10, Tier::M1, Tier::M15];
    let mut next_tier = 0;
    let mut in_flight: Option<(Tier, Option<Cursor>, Instant)> = None;
    let mut history_available = false;
    let mut history_failed = false;
    let mut floors = HashMap::<Tier, i64>::new();
    let mut restart_cursor = HashSet::<Tier>::new();
    let mut last_recent = Instant::now() - Duration::from_secs(60);
    let (promotion_tx, mut promotion_rx) = tokio::sync::mpsc::channel(1);
    let mut promotion_task: Option<codec::AbortOnDrop> = None;
    let mut recent_pending = false;
    let mut clock_ready = false;
    loop {
        if clock_ready
            && source.clock.now_ms().is_some()
            && history_available
            && !history_failed
            && !recent_pending
            && in_flight.is_none()
            && next_tier < tiers.len()
            && let Some(mirror) = &source.mirror
        {
            let tier = tiers[next_tier];
            let now = source.clock.now_ms().unwrap_or_else(kelvo_engine::wall_ms);
            let after = if restart_cursor.remove(&tier) {
                None
            } else {
                match mirror.cursor(tier, epoch, kinds) {
                    Ok(cursor) => cursor,
                    Err(error) => {
                        history_failed = true;
                        status.history_error = Some(error);
                        status.catching_up = false;
                        (source.on_status)(status.clone());
                        continue;
                    }
                }
            };
            let floor = mirror
                .history_floor(tier, now)
                .max(floors.get(&tier).copied().unwrap_or(i64::MIN));
            codec::write(
                &mut session.writer,
                &Message::SyncRequest(SyncRequest {
                    tier,
                    after,
                    max_rows: 2000,
                    from_ms: Some(floor),
                    max_bytes: Some(page_bytes),
                }),
            )
            .await?;
            in_flight = Some((tier, after, Instant::now()));
            status.catching_up = true;
            (source.on_status)(status.clone());
            if next_tier == 0 {
                last_sync = Instant::now();
            }
        }
        tokio::select! {
            _=control.changed()=>{
                let next=control.borrow_and_update().clone();
                if next.stop||next.asleep{return Ok(None);}
                if next.retry!=retry{return Ok(None);}
                retry=next.retry;
                if next.network!=network && session.route==Route::Tailscale {
                    network=next.network;
                    if let Some(lan)=&source.endpoints.lan {
                        let config=EndpointConfig{lan:Some(lan.clone()),tailscale:None,helper_path:source.endpoints.helper_path.clone()};
                        // Debounced, cancellable promotion runs beside the old stream.
                        let tx=promotion_tx.clone();let local=source.local;let expected=source.record.id;
                        promotion_task=Some(codec::AbortOnDrop(tokio::spawn(async move{
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            let result=crate::connect(&config,local,Some(expected)).await;
                            let _=tx.send(result).await;
                        })));
                    }
                }
                codec::write(&mut session.writer,&Message::Interest(next.interest)).await?;
            },
            promoted=promotion_rx.recv(),if promotion_task.is_some()=>{
                promotion_task=None;
                if let Some(Ok(replacement))=promoted {return Ok(Some(replacement));}
            },
            incoming=messages.recv()=>{
                let message=incoming.ok_or_else(||RemoteError::Transport("remote stream closed".into()))??;
                match message {
                    Message::LayoutDef(layout)=>{
                        if layout.series.len()>8192 || layouts.len()>=1024 {return Err(RemoteError::Protocol("layout cardinality exceeded".into()));}
                        if layouts.contains_key(&layout.layout_no){return Err(RemoteError::Protocol("layout number reused in session".into()));}
                        let remote_no=layout.layout_no;let layout=Arc::new(FrameLayout{layout_no:source.next_layout.fetch_add(1,Ordering::Relaxed),series:layout.series.into()});layouts.insert(remote_no,layout.clone());sink.live.publish(BusMsg::Layout(layout));
                    },
                    Message::Live(frame)=>{
                        let layout=layouts.get(&frame.layout_no).ok_or_else(||RemoteError::Protocol("unknown layout".into()))?.clone();
                        let n=layout.series.len();if frame.values.len()!=n || frame.held.len()!=n || frame.holds.len()!=n || frame.interval_ms==0 {return Err(RemoteError::Protocol("invalid live frame dimensions or cadence".into()));}
                        if source_timeline.is_some_and(|t|t!=frame.timeline){
                            timeline=source.next_timeline.fetch_add(1,Ordering::Relaxed);
                            if outstanding_ping.is_none(){ping_nonce=ping_nonce.wrapping_add(1);codec::write(&mut session.writer,&Message::Ping{nonce:ping_nonce,t0_ms:kelvo_engine::wall_ms()}).await?;outstanding_ping=Some((ping_nonce,Instant::now()));last_ping=Instant::now();}
                        }
                        source_timeline=Some(frame.timeline);
                        status.last_seen_ms=Some(kelvo_engine::wall_ms());
                        sink.live.publish(BusMsg::Frame(Arc::new(LiveFrame{ts_ms:frame.ts_ms,interval_ms:frame.interval_ms,layout,timeline,values:frame.values.into(),held:frame.held.into(),holds:frame.holds.into()})));
                        (source.on_status)(status.clone());
                    },
                    Message::HostSummary(summary)=>{
                        if summary.headline.len()>8192{return Err(RemoteError::Protocol("summary too large".into()));}
                        status.last_seen_ms=Some(kelvo_engine::wall_ms());
                        let layout=Arc::new(FrameLayout{layout_no:u32::MAX-1,series:summary.headline.iter().map(|(k,_)|k.clone()).collect()});
                        let held:Arc<[f32]>=summary.headline.iter().map(|(_,v)|*v).collect();
                        sink.live.publish(BusMsg::Frame(Arc::new(LiveFrame{ts_ms:summary.ts_ms,interval_ms:5000,timeline,holds:vec![12500;held.len()].into(),values:vec![f32::NAN;held.len()].into(),held,layout})));
                        (source.on_status)(status.clone());
                    },
                    Message::SourceStatus(wire)=>{
                        history_available=wire.history_available;
                        if !history_available{status.history_error=Some("Remote history is disabled or unavailable".into());}
                        else if status.history_error.as_deref()==Some("Remote history is disabled or unavailable"){status.history_error=None;}
                        let s=server::status_from_wire(wire);*source.status.lock_ok()=s.clone();sink.live.publish(BusMsg::Status(s));(source.on_status)(status.clone());
                    },
                    Message::CapabilitiesChanged(c)=>{*source.caps.lock_ok()=c.clone();sink.live.publish(BusMsg::Caps(Arc::new(c)));},
                    Message::LiveProcesses(p)=>{if p.rows.len()>32768{return Err(RemoteError::Protocol("process row limit exceeded".into()));}sink.live.publish(BusMsg::Processes(Arc::new(ProcessBatch{ts_ms:p.ts_ms,rows:p.rows.into_iter().map(server::process_from_wire).collect(),gpu_span_ms:p.gpu_span_ms})));},
                    Message::Pong{nonce,t_remote_ms,..}=>{
                        if let Some((expected,sent))=outstanding_ping.take(){
                            if nonce!=expected{return Err(RemoteError::Protocol("heartbeat nonce mismatch".into()));}
                            let rtt=sent.elapsed().as_millis().min(10_000) as i64;
                            source.clock.observe(t_remote_ms.saturating_add(rtt/2));clock_ready=true;status.clock_offset_ms=source.clock.now_ms().unwrap_or(t_remote_ms).saturating_sub(kelvo_engine::wall_ms());
                            (source.on_status)(status.clone());
                        }
                    },
                    Message::SyncPage(page)=>{
                        let (tier,after,requested)=in_flight.take().ok_or_else(||RemoteError::Protocol("unsolicited sync page".into()))?;
                        if page.epoch!=epoch {return Err(RemoteError::Transport("source history was reset; reconnecting".into()));}
                        if page.tier!=tier || page.last_seq<after.map_or(0,|c|c.seq) || (page.more && page.last_seq==after.map_or(0,|c|c.seq)) {return Err(RemoteError::Protocol("sync cursor did not advance or epoch changed".into()));}
                        let received_bytes=kelvo_proto::encode_message(&Message::SyncPage(page.clone()))?.len();
                        if received_bytes>256*1024{return Err(RemoteError::Protocol("oversized history page".into()));}
                        let more=page.more;
                        if let Some(mirror)=source.mirror.clone(){
                            unpruned_bytes=unpruned_bytes.saturating_add(received_bytes);unpruned_pages+=1;
                            let prune=unpruned_bytes>=1024*1024||unpruned_pages>=16;
                            if prune{unpruned_bytes=0;unpruned_pages=0;}
                            let now=source.clock.now_ms().unwrap_or_else(kelvo_engine::wall_ms);
                            let result=tokio::task::spawn_blocking(move||{
                                let result=mirror.ingest(page,kinds).and_then(|()|mirror.enforce_budget(now));
                                if prune||result.is_err(){mirror.prune(now)?;}
                                result
                            }).await.map_err(|e|RemoteError::Transport(e.to_string()))?;
                            if let Err(error)=result {history_failed=true;next_tier=tiers.len();last_sync=Instant::now();status.history_error=Some(error);status.catching_up=false;(source.on_status)(status.clone());} else {status.history_error=None;}
                        }
                        if !more{next_tier+=1;}
                        page_bytes=adapt_page_bytes(page_bytes,requested.elapsed());
                        tokio::task::yield_now().await;
                    },
                    Message::Truncated{tier,earliest_ts_ms,epoch:remote_epoch}=>{
                        let expected=in_flight.take().ok_or_else(||RemoteError::Protocol("unsolicited truncation".into()))?;
                        if remote_epoch!=epoch{return Err(RemoteError::Transport("source history was reset; reconnecting".into()));}
                        if tier!=expected.0{return Err(RemoteError::Protocol("invalid truncation".into()));}
                        if let Some(mirror)=&source.mirror && let Err(error)=mirror.truncated(tier,epoch,earliest_ts_ms){history_failed=true;status.history_error=Some(error);}
                        floors.insert(tier,earliest_ts_ms);restart_cursor.insert(tier);
                    },
                    Message::RecentRows{epoch:remote_epoch,tier,complete_to_ms,layouts,rows}=>{
                        if !recent_pending{return Err(RemoteError::Protocol("unsolicited recent snapshot".into()));}
                        recent_pending=false;
                        if remote_epoch!=epoch{return Err(RemoteError::Transport("source history was reset; reconnecting".into()));}
                        if let Some(mirror)=source.mirror.clone() && let Err(error)=mirror.recent(remote_epoch,tier,complete_to_ms,layouts,rows){status.history_error=Some(error);}
                    },
                    Message::Error{message,..}=>{if recent_pending{recent_pending=false;}else{history_failed=true;in_flight=None;next_tier=tiers.len();last_sync=Instant::now();}status.history_error=Some(message);status.catching_up=false;(source.on_status)(status.clone());},
                    Message::Unknown=>{},
                    _=>return Err(RemoteError::Protocol("unexpected server message".into())),
                }
            },
            _=timer.tick()=>{
                if outstanding_ping.is_some_and(|(_,sent)|sent.elapsed()>Duration::from_secs(30)){return Err(RemoteError::Transport("remote application heartbeat timed out".into()));}
                if outstanding_ping.is_none()&&last_ping.elapsed()>=Duration::from_secs(15){ping_nonce=ping_nonce.wrapping_add(1);codec::write(&mut session.writer,&Message::Ping{nonce:ping_nonce,t0_ms:kelvo_engine::wall_ms()}).await?;last_ping=Instant::now();outstanding_ping=Some((ping_nonce,last_ping));}
                if recent_pending&&last_recent.elapsed()>Duration::from_secs(30){recent_pending=false;status.history_error=Some("recent history request timed out".into());}
                if in_flight.as_ref().is_some_and(|(_,_,at)|at.elapsed()>Duration::from_secs(30)){history_failed=true;in_flight=None;next_tier=tiers.len();last_sync=Instant::now();status.history_error=Some("history request timed out".into());status.catching_up=false;(source.on_status)(status.clone());}
                let period=if source.controller_battery.load(Ordering::Relaxed){300}else{60};
                if in_flight.is_none()&&next_tier>=tiers.len(){
                    if status.catching_up {
                        if let Some(mirror)=source.mirror.clone(){let now=source.clock.now_ms().unwrap_or_else(kelvo_engine::wall_ms);if let Err(error)=tokio::task::spawn_blocking(move||mirror.prune(now)).await.map_err(|e|RemoteError::Transport(e.to_string()))?{status.history_error=Some(error);}}
                        status.catching_up=false;(source.on_status)(status.clone());
                    }
                    if last_sync.elapsed()>=Duration::from_secs(period){next_tier=0;history_failed=false;}
                }
                if history_available&&control.borrow().interest.detail&&last_recent.elapsed()>=Duration::from_secs(30)&&in_flight.is_none()&&!recent_pending&&next_tier>=tiers.len(){
                    let now=source.clock.now_ms().unwrap_or_else(kelvo_engine::wall_ms);
                    codec::write(&mut session.writer,&Message::RecentRequest{from_ms:now.saturating_sub(300_000),to_ms:now}).await?;last_recent=Instant::now();recent_pending=true;
                }

            }
        }
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
            ) -> std::result::Result<Option<Cursor>, String> {
                Ok(self.cursors.lock_ok().get(&tier).copied())
            }
            fn ingest(&self, page: SyncPage, _kinds: SyncKinds) -> std::result::Result<(), String> {
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
            ) -> std::result::Result<(), String> {
                self.truncated.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            fn enforce_budget(&self, _now: i64) -> std::result::Result<(), String> {
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

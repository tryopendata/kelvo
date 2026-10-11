//! Independent, opt-in Docker clock. Finite polling, events and logs use separate budgets.
#[cfg(test)]
mod integration_tests;
mod logs;
mod normalize;
mod persistence;
use futures_util::{StreamExt, stream};
use kelvo_collect::docker::{DockerAdapter, DockerCounters, DockerError, candidates, now_ms};
use kelvo_schema::{HostId, docker::*, lock::LockExt};
pub use persistence::DockerRecorder;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, watch};

pub use logs::LogSubscription;
#[derive(Clone, Debug, PartialEq)]
struct Control {
    settings: DockerSettings,
    generation: u32,
    visible: u32,
    paused: bool,
    min_interval_ms: u32,
    shutdown: bool,
}
#[derive(Clone)]
pub struct DockerHandle {
    control: watch::Sender<Control>,
    changed: watch::Sender<DockerSnapshot>,
    inner: Arc<Mutex<State>>,
    home: PathBuf,
    gate: Arc<Mutex<()>>,
    pub events: broadcast::Sender<DockerEvent>,
    logs: Arc<Mutex<logs::LogRegistry>>,
    pub recorder: Arc<DockerRecorder>,
}
struct State {
    adapter: Option<DockerAdapter>,
    events: VecDeque<DockerEvent>,
}
impl DockerHandle {
    pub fn new(host_id: HostId, settings: DockerSettings, home: PathBuf) -> Self {
        let (control, _) = watch::channel(Control {
            settings: settings.clone(),
            generation: 1,
            visible: 0,
            paused: false,
            min_interval_ms: 1000,
            shutdown: false,
        });
        let (events, _) = broadcast::channel(256);
        let (changed, _) = watch::channel(DockerSnapshot {
            host_id,
            generation: 1,
            status: if settings.enabled {
                DockerStatus::Connecting
            } else {
                DockerStatus::Disabled
            },
            message: None,
            engine: None,
            settings,
            containers: Vec::new(),
            aggregate: DockerAggregate {
                monitoring_limit: 128,
                ..Default::default()
            },
            runtime: unavailable_runtime(""),
            coverage: Vec::new(),
            observed_ms: now_ms(),
            interval_ms: 10000,
            population_revision: 0,
            persistence: DockerPersistence {
                status: "live_only".into(),
                logical_bytes: 0.,
                allowance_bytes: 37_500_000.,
                retained_from_ms: None,
            },
        });
        Self {
            control,
            changed,
            inner: Arc::new(Mutex::new(State {
                adapter: None,
                events: VecDeque::new(),
            })),
            home,
            gate: Arc::new(Mutex::new(())),
            events,
            logs: Arc::new(Mutex::new(logs::LogRegistry::default())),
            recorder: Arc::new(DockerRecorder::new(host_id)),
        }
    }
    pub fn glance(&self) -> (DockerStatus, DockerAggregate) {
        let s = self.changed.borrow();
        (s.status.clone(), s.aggregate.clone())
    }
    pub fn snapshot(&self) -> DockerSnapshot {
        self.changed.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<DockerSnapshot> {
        self.changed.subscribe()
    }
    pub fn configure(&self, settings: DockerSettings) -> Result<(), DockerError> {
        if settings.monitored_containers.len() > 128
            || settings
                .monitored_containers
                .iter()
                .any(|id| !kelvo_collect::docker::valid_id(id))
        {
            return Err(DockerError::Invalid);
        }
        if settings.selected_engine.as_ref().is_some_and(|s| {
            !std::path::Path::new(s).is_absolute() || s.contains("://") || s.len() > 4096
        }) {
            return Err(DockerError::Endpoint);
        }
        let _gate = self.gate.lock_ok();
        self.gap(if settings.enabled {
            "engine_switch"
        } else {
            "disabled"
        });
        self.cancel();
        self.control.send_modify(|c| {
            c.settings = settings.clone();
            c.generation = c.generation.wrapping_add(1);
        });
        let c = self.control.borrow().clone();
        self.changed.send_modify(|s| {
            s.settings = settings;
            s.generation = c.generation;
            s.status = if c.settings.enabled {
                DockerStatus::Connecting
            } else {
                DockerStatus::Disabled
            };
            s.containers.clear();
            s.aggregate = DockerAggregate {
                monitoring_limit: 128,
                ..Default::default()
            };
            s.engine = None;
            s.coverage.clear();
        });
        Ok(())
    }
    pub fn invalidate_history(&self) -> u32 {
        let _gate = self.gate.lock_ok();
        self.cancel();
        self.control
            .send_modify(|c| c.generation = c.generation.wrapping_add(1));
        let generation = self.control.borrow().generation;
        self.inner.lock_ok().events.clear();
        self.changed.send_modify(|s| {
            s.generation = generation;
            s.containers.clear();
            s.coverage.clear();
            s.aggregate = DockerAggregate {
                monitoring_limit: 128,
                ..Default::default()
            };
        });
        generation
    }
    pub fn resume(&self) {
        let _gate = self.gate.lock_ok();
        self.cancel();
        self.control
            .send_modify(|c| c.generation = c.generation.wrapping_add(1));
    }
    pub fn set_visible(&self, visible: bool) {
        self.control.send_modify(|c| {
            c.visible = if visible {
                c.visible.saturating_add(1)
            } else {
                c.visible.saturating_sub(1)
            }
        });
    }
    pub fn set_power(&self, paused: bool, min_interval_ms: u32) {
        let _gate = self.gate.lock_ok();
        let old = self.control.borrow().clone();
        if old.paused == paused && old.min_interval_ms == min_interval_ms.max(1000) {
            return;
        }
        if old.paused != paused {
            self.cancel();
        }
        self.control.send_modify(|c| {
            if c.paused != paused {
                c.generation = c.generation.wrapping_add(1);
            }
            c.paused = paused;
            c.min_interval_ms = min_interval_ms.max(1000);
        });
    }
    pub fn shutdown(&self) {
        let _gate = self.gate.lock_ok();
        self.gap("shutdown");
        self.cancel();
        self.control.send_modify(|c| {
            c.shutdown = true;
            c.generation = c.generation.wrapping_add(1);
        });
        // Every producer is fenced by this gate. The store closes immediately after
        // this call, so seal the final partial bucket before releasing it.
        if self.recorder.shutdown_flush().is_err() {
            tracing::warn!("Docker history shutdown flush failed");
        }
    }
    fn cancel(&self) {
        self.inner.lock_ok().adapter = None;
        self.logs.lock_ok().cancel_all();
    }
    pub async fn discover(&self) -> Vec<DockerEngine> {
        let home = self.home.clone();
        let list = tokio::task::spawn_blocking(move || candidates(&home))
            .await
            .unwrap_or_default();
        let mut results = stream::iter(list)
            .map(|candidate| async move {
                match DockerAdapter::connect(candidate.clone()).await {
                    Ok(a) => a.engine,
                    Err(e) => DockerEngine {
                        error: Some(e.to_string()),
                        ..candidate
                    },
                }
            })
            .buffer_unordered(8)
            .collect::<Vec<_>>()
            .await;
        results.sort_by(|a, b| b.preferred.cmp(&a.preferred).then(a.socket.cmp(&b.socket)));
        let mut seen = BTreeSet::new();
        results.retain(|e| {
            e.daemon_id
                .as_ref()
                .is_none_or(|id| seen.insert(id.clone()))
        });
        results
    }
    pub fn detail(&self, engine: &str, id: &str) -> Option<DockerContainer> {
        self.snapshot()
            .containers
            .into_iter()
            .find(|c| c.engine_id == engine && c.id == id)
            .or_else(|| self.recorder.detail(engine, id))
    }
    pub async fn detail_live(&self, engine: &str, id: &str) -> Result<DockerContainer, String> {
        let live = self
            .changed
            .borrow()
            .containers
            .iter()
            .find(|c| c.engine_id == engine && c.id == id)
            .cloned();
        let Some(mut row) = live else {
            return self
                .recorder
                .detail(engine, id)
                .ok_or_else(|| "Container is not available in retained history".into());
        };
        if row.lifecycle == DockerLifecycle::Removed {
            return Ok(row);
        }
        let adapter = self.inner.lock_ok().adapter.clone();
        if let Some(adapter) = adapter.filter(|a| a.engine.id == engine) {
            match adapter.inspect(row.clone()).await {
                Ok(fresh) => Ok(fresh),
                Err(DockerError::NotFound) => {
                    row.lifecycle = DockerLifecycle::Removed;
                    row.metadata_observed_ms = Some(now_ms());
                    row.observed_ms = None;
                    row.interval_ms = None;
                    row.metrics = DockerMetrics::default();
                    Ok(row)
                }
                Err(error) => Err(error.to_string()),
            }
        } else {
            Ok(row)
        }
    }

    pub fn retained(&self, request: &DockerRetainedRequest) -> Result<DockerRetainedPage, String> {
        self.recorder.retained(request)
    }
    pub fn history(&self, request: &DockerHistoryRequest) -> Result<DockerHistoryPage, String> {
        self.recorder.history(request)
    }
    pub fn query_events(&self, r: &DockerEventsRequest) -> Result<DockerEventsPage, String> {
        self.recorder.events(r)
    }
    pub fn open_logs(&self, request: DockerLogsRequest) -> Result<LogSubscription, String> {
        logs::open(self, request)
    }
    pub fn cancel_logs(&self, id: u32, reason: &'static str) {
        self.logs.lock_ok().cancel(id, reason);
    }
    pub fn close_logs(&self, id: u32) {
        self.logs.lock_ok().close(id);
    }
    fn publish(&self, mut snapshot: DockerSnapshot) {
        let _gate = self.gate.lock_ok();
        if self.control.borrow().generation != snapshot.generation {
            return;
        }
        self.recorder.record(&snapshot);
        snapshot.persistence = self.recorder.status();
        self.changed.send_replace(snapshot);
    }
    fn gap(&self, reason: &str) {
        self.changed.send_modify(|s| {
            if s.coverage
                .last()
                .is_some_and(|g| g.reason == reason && g.end_ms.is_none())
            {
                return;
            }
            let now = now_ms();
            if let Some(g) = s.coverage.last_mut()
                && g.end_ms.is_none()
            {
                g.end_ms = Some(now.max(g.start_ms));
            }
            s.coverage.push(DockerCoverage {
                start_ms: now,
                end_ms: None,
                container_id: None,
                reason: reason.into(),
            });
            if s.coverage.len() > 128 {
                s.coverage.drain(0..64);
            }
        });
        self.recorder.record(&self.snapshot());
    }
    fn gap_for(&self, generation: u32, reason: &str) {
        let _gate = self.gate.lock_ok();
        if self.control.borrow().generation == generation {
            self.gap(reason);
        }
    }
    fn transition(
        &self,
        generation: u32,
        status: DockerStatus,
        message: Option<String>,
        reason: &str,
    ) -> bool {
        let _gate = self.gate.lock_ok();
        if self.control.borrow().generation != generation {
            return false;
        }
        self.changed.send_modify(|s| {
            s.status = status;
            s.message = message;
            s.generation = generation;
        });
        self.gap(reason);
        true
    }
    pub async fn run(self) {
        let mut controls = self.control.subscribe();
        let mut retry = 1;
        loop {
            let c = controls.borrow_and_update().clone();
            if c.shutdown {
                break;
            }
            if !c.settings.enabled || c.paused {
                if !self.transition(
                    c.generation,
                    if c.paused {
                        DockerStatus::Paused
                    } else {
                        DockerStatus::Disabled
                    },
                    None,
                    if c.paused { "paused" } else { "disabled" },
                ) {
                    continue;
                }
                if controls.changed().await.is_err() {
                    break;
                }
                continue;
            }
            let generation = c.generation;
            let result = tokio::select! {biased;_=controls.changed()=>continue,result=self.connect_selected(&c.settings)=>result};
            let adapter = match result {
                Ok(a) => a,
                Err(e) => {
                    let status = match e {
                        DockerError::Incompatible => DockerStatus::Incompatible,
                        DockerError::PermissionDenied => DockerStatus::PermissionDenied,
                        _ => DockerStatus::Unavailable,
                    };
                    if !self.transition(generation, status, Some(e.to_string()), "disconnected") {
                        continue;
                    }
                    self.wait_retry(&mut controls, generation, retry).await;
                    retry = (retry * 2).min(30);
                    continue;
                }
            };
            {
                let _gate = self.gate.lock_ok();
                if self.control.borrow().generation != generation {
                    continue;
                }
                self.inner.lock_ok().adapter = Some(adapter.clone());
                self.changed.send_modify(|s| {
                    if s.engine.as_ref().is_none_or(|e| e.id != adapter.engine.id) {
                        s.coverage.clear();
                        s.containers.clear();
                        s.aggregate = DockerAggregate {
                            monitoring_limit: 128,
                            ..Default::default()
                        };
                    }
                    s.engine = Some(adapter.engine.clone());
                    s.generation = generation;
                });
            }
            let session = self
                .session(adapter, generation, &mut controls, &mut retry)
                .await;
            self.cancel();
            if session == SessionEnd::Disconnected {
                if !self.transition(
                    generation,
                    DockerStatus::Unavailable,
                    Some("Docker stream disconnected; retrying".into()),
                    "disconnected",
                ) {
                    continue;
                }
                self.wait_retry(&mut controls, generation, retry).await;
                retry = (retry * 2).min(30);
            }
            if session == SessionEnd::Idle {
                if !self.transition(
                    generation,
                    DockerStatus::IdleSuspended,
                    None,
                    "idle_suspended",
                ) {
                    continue;
                }
                loop {
                    if controls.changed().await.is_err() {
                        return;
                    }
                    let next = controls.borrow_and_update().clone();
                    if next.generation != generation || next.shutdown {
                        break;
                    }
                }
            }
        }
    }
    async fn wait_retry(
        &self,
        controls: &mut watch::Receiver<Control>,
        generation: u32,
        seconds: u64,
    ) {
        let delay = tokio::time::sleep(Duration::from_secs(seconds));
        tokio::pin!(delay);
        loop {
            tokio::select! {biased;
                changed = controls.changed() => {
                    if changed.is_err() || controls.borrow().generation != generation || controls.borrow().shutdown { return; }
                }
                _ = &mut delay => return,
            }
        }
    }
    async fn connect_selected(&self, s: &DockerSettings) -> Result<DockerAdapter, DockerError> {
        let list = self.discover_candidates().await;
        let chosen = if let Some(socket) = &s.selected_engine {
            list.into_iter()
                .find(|e| e.socket == *socket)
                .or_else(|| {
                    Some(DockerEngine {
                        id: socket.clone(),
                        socket: socket.clone(),
                        runtime: "Local Docker (unverified)".into(),
                        context: None,
                        daemon_id: None,
                        reachable: false,
                        preferred: false,
                        error: None,
                        api_version: None,
                        capacity_bytes: None,
                        capacity_observed_ms: None,
                    })
                })
                .ok_or(DockerError::Unavailable)?
        } else {
            let found = self.discover().await;
            found
                .iter()
                .find(|e| e.preferred && e.reachable)
                .cloned()
                .or_else(|| {
                    let mut reachable = found.into_iter().filter(|e| e.reachable);
                    let only = reachable.next()?;
                    reachable.next().is_none().then_some(only)
                })
                .ok_or(DockerError::Unavailable)?
        };
        DockerAdapter::connect(chosen).await
    }
    async fn discover_candidates(&self) -> Vec<DockerEngine> {
        let home = self.home.clone();
        tokio::task::spawn_blocking(move || candidates(&home))
            .await
            .unwrap_or_default()
    }
    async fn session(
        &self,
        mut adapter: DockerAdapter,
        generation: u32,
        controls: &mut watch::Receiver<Control>,
        retry: &mut u64,
    ) -> SessionEnd {
        let mut healthy_since = None;
        let mut baseline: BTreeMap<String, (DockerCounters, Instant)> = BTreeMap::new();
        let mut cursor = 0usize;
        let mut inventory_at = Instant::now() - Duration::from_secs(60);
        let mut rows = Vec::<DockerContainer>::new();
        let mut inventory_complete = true;
        let mut population = BTreeSet::new();
        let mut population_revision = self.snapshot().population_revision;
        let live_since = self
            .inner
            .lock_ok()
            .events
            .iter()
            .rev()
            .find(|e| e.engine_id == adapter.engine.id)
            .map(|e| {
                format!(
                    "{}.{:09}",
                    e.time_ms / 1000,
                    e.time_ns.parse::<i64>().unwrap_or(0) % 1_000_000_000
                )
            });
        let recorder = Arc::clone(&self.recorder);
        let daemon = adapter.engine.id.clone();
        let durable = tokio::task::spawn_blocking(move || recorder.event_cursor(&daemon))
            .await
            .ok()
            .flatten();
        let since = live_since.or_else(|| {
            durable
                .and_then(|s| s.parse::<i64>().ok())
                .map(|ns| format!("{}.{:09}", ns / 1_000_000_000, ns % 1_000_000_000))
        });
        // Docker retains only 256 replay events: reconnect cannot prove complete coverage.
        self.gap_for(generation, "event_replay_uncertain");
        let mut events = Box::pin(adapter.events_since(since));
        let mut event_live = true;
        let mut refresh = true;
        let mut event_rate = (Instant::now(), 0usize);
        let mut restarts: BTreeMap<String, VecDeque<i64>> = BTreeMap::new();
        let mut clock = (now_ms(), Instant::now());
        let mut last_observation: Option<Instant> = None;
        loop {
            let control = controls.borrow_and_update().clone();
            if control.generation != generation || control.shutdown {
                return SessionEnd::Changed;
            }
            let interval =
                control
                    .min_interval_ms
                    .max(if control.visible > 0 { 1000 } else { 10000 });
            let wall = now_ms();
            let mono = Instant::now();
            if wall
                .saturating_sub(clock.0)
                .saturating_sub(mono.duration_since(clock.1).as_millis() as i64)
                .abs()
                > 2000
            {
                baseline.clear();
                {
                    let _gate = self.gate.lock_ok();
                    if self.control.borrow().generation == generation {
                        self.recorder.clock_step(wall);
                        self.gap("clock_step");
                    }
                }
            }
            clock = (wall, mono);
            let cycle_start = Instant::now();
            let mut event_refresh = false;
            let ok = {
                let cycle = self.cycle(
                    &mut adapter,
                    &mut rows,
                    &mut baseline,
                    &mut cursor,
                    &mut inventory_at,
                    &mut inventory_complete,
                    &mut population,
                    &mut population_revision,
                    refresh,
                    &control.settings,
                    generation,
                );
                tokio::pin!(cycle);
                loop {
                    tokio::select! {biased;
                        _=controls.changed()=>{if controls.borrow().generation!=generation{return SessionEnd::Changed;}},
                        result=&mut cycle=>break result,
                        event=events.next(),if event_live=>{match event{Some(Ok(e))=>{if admit_event(&mut event_rate){self.record_event(e,&mut restarts,generation);event_refresh=true;}else{event_live=false;self.gap_for(generation,"events_dropped_rate_limit");}},_=>{event_live=false;self.gap_for(generation,"events_disconnected");}}}
                    }
                }
            };
            refresh = event_refresh;
            if ok && event_live {
                if healthy_since.get_or_insert_with(Instant::now).elapsed()
                    >= Duration::from_secs(60)
                {
                    *retry = 1;
                }
            } else {
                healthy_since = None;
            }
            if !ok {
                self.gap_for(generation, "partial_cycle");
            }
            if rows.is_empty() && inventory_at.elapsed() > Duration::from_secs(30) {
                return SessionEnd::Disconnected;
            }
            let observed = now_ms();
            for row in &mut rows {
                let n = restarts
                    .get_mut(&row.id)
                    .map(|q| {
                        q.retain(|t| *t >= observed - 300000);
                        q.len() as u32
                    })
                    .unwrap_or(0);
                row.observed_restarts = n;
                row.repeated_restarts = n >= 3;
            }
            let mut snapshot = self.snapshot();
            snapshot.generation = generation;
            snapshot.status = DockerStatus::Connected;
            snapshot.message = None;
            snapshot.engine = Some(adapter.engine.clone());
            snapshot.runtime = unavailable_runtime(&adapter.engine.runtime);
            snapshot.settings = control.settings.clone();
            snapshot.observed_ms = observed;
            let observed_mono = Instant::now();
            snapshot.interval_ms = last_observation
                .map(|t| {
                    observed_mono
                        .duration_since(t)
                        .as_millis()
                        .min(u32::MAX as u128) as u32
                })
                .unwrap_or(0);
            last_observation = Some(observed_mono);
            snapshot.population_revision = population_revision;
            // Receipt time differs per request; preserve it and publish source intervals.
            snapshot.containers = rows.clone();
            snapshot.aggregate = aggregate(
                &rows,
                adapter.engine.capacity_bytes,
                inventory_complete,
                observed,
                interval,
            );
            if ok
                && let Some(g) = snapshot.coverage.last_mut()
                && g.end_ms.is_none()
            {
                g.end_ms = Some(observed);
            }
            self.publish(snapshot);
            if !rows.iter().any(|c| c.lifecycle == DockerLifecycle::Running) {
                return SessionEnd::Idle;
            }
            let sleep = tokio::time::sleep(
                Duration::from_millis(interval as u64).saturating_sub(cycle_start.elapsed()),
            );
            tokio::pin!(sleep);
            loop {
                tokio::select! {biased;
                    _=controls.changed()=>{if controls.borrow().generation!=generation{return SessionEnd::Changed;}break;},
                    event=events.next(),if event_live=>{match event{Some(Ok(e))=>{if admit_event(&mut event_rate){self.record_event(e,&mut restarts,generation);}else{event_live=false;self.gap_for(generation,"events_dropped_rate_limit");}refresh=true;},_=>{self.gap_for(generation,"events_disconnected");event_live=false;}}},
                    _=&mut sleep=>break
                }
            }
            if !event_live {
                return SessionEnd::Disconnected;
            }
        }
    }
    #[allow(clippy::too_many_arguments)] // One cycle's explicitly scoped mutable state; never shared with other tasks.
    async fn cycle(
        &self,
        adapter: &mut DockerAdapter,
        rows: &mut Vec<DockerContainer>,
        baseline: &mut BTreeMap<String, (DockerCounters, Instant)>,
        cursor: &mut usize,
        inventory_at: &mut Instant,
        inventory_complete: &mut bool,
        population: &mut BTreeSet<String>,
        population_revision: &mut u32,
        refresh: bool,
        settings: &DockerSettings,
        generation: u32,
    ) -> bool {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        if refresh || inventory_at.elapsed() >= Duration::from_secs(30) {
            if adapter
                .engine
                .capacity_observed_ms
                .is_none_or(|t| now_ms().saturating_sub(t) >= 30_000)
            {
                let _ = tokio::time::timeout_at(deadline, adapter.refresh_capacity()).await;
            }
            match tokio::time::timeout_at(deadline, adapter.inventory()).await {
                Ok(Ok((mut fresh, complete))) => {
                    let previous: BTreeMap<_, _> =
                        rows.iter().map(|c| (c.id.clone(), c.clone())).collect();
                    for row in &mut fresh {
                        if let Some(old) = previous.get(&row.id) {
                            let lifecycle = row.lifecycle;
                            let name = row.name.clone();
                            let image = row.image.clone();
                            *row = old.clone();
                            row.lifecycle = lifecycle;
                            row.name = name;
                            row.image = image;
                            row.last_seen_ms = now_ms();
                        }
                    }
                    let fresh_ids: BTreeSet<_> = fresh.iter().map(|c| c.id.as_str()).collect();
                    let _gate = self.gate.lock_ok();
                    if self.control.borrow().generation != generation {
                        return false;
                    }
                    for old in rows
                        .iter()
                        .filter(|c| complete && !fresh_ids.contains(c.id.as_str()))
                    {
                        let mut gone = old.clone();
                        gone.lifecycle = DockerLifecycle::Removed;
                        gone.metrics = DockerMetrics::default();
                        self.recorder.removed(&gone, generation);
                    }
                    fresh.sort_by(|a, b| a.id.cmp(&b.id));
                    *rows = fresh;
                    *inventory_complete = complete;
                    *inventory_at = Instant::now();
                }
                _ => return false,
            }
        }
        let selected: BTreeSet<_> = if !settings.explicit_selection {
            let mut selected: BTreeSet<String> = population
                .iter()
                .filter(|id| rows.iter().any(|c| c.id == **id))
                .cloned()
                .collect();
            for row in rows
                .iter()
                .filter(|c| c.lifecycle == DockerLifecycle::Running)
                .chain(
                    rows.iter()
                        .filter(|c| c.lifecycle != DockerLifecycle::Running),
                )
            {
                if selected.len() >= 128 {
                    break;
                }
                selected.insert(row.id.clone());
            }
            selected
        } else {
            settings.monitored_containers.iter().cloned().collect()
        };
        if *population != selected {
            *population = selected;
            *population_revision = population_revision.wrapping_add(1);
        }
        baseline.retain(|id, _| {
            population.contains(id)
                && rows
                    .iter()
                    .any(|c| c.id == *id && c.lifecycle == DockerLifecycle::Running)
        });
        for row in rows.iter_mut() {
            row.monitored = population.contains(&row.id);
            if row.lifecycle != DockerLifecycle::Running || !row.monitored {
                row.metrics = DockerMetrics::default();
                row.observed_ms = None;
                row.interval_ms = None;
            }
        }
        let ids: Vec<_> = rows
            .iter()
            .enumerate()
            .filter(|(_, c)| c.monitored)
            .map(|(i, _)| i)
            .collect();
        if ids.is_empty() {
            return true;
        }
        let offset = *cursor % ids.len();
        let mut ordered = ids.clone();
        ordered.rotate_left(offset);
        let jobs: Vec<_> = ordered
            .into_iter()
            .filter_map(|index| rows.get(index).cloned().map(|row| (index, row)))
            .collect();
        let adapter: &DockerAdapter = adapter;
        let pending = stream::iter(jobs)
            .map(|(index, row)| async move {
                let inspected = if refresh || row.health == DockerHealth::Unknown {
                    adapter.inspect(row.clone()).await.unwrap_or(row)
                } else {
                    row
                };
                let stats = if inspected.lifecycle == DockerLifecycle::Running {
                    Some(adapter.stats(&inspected.id).await)
                } else {
                    None
                };
                (index, inspected, stats, Instant::now(), now_ms())
            })
            .buffer_unordered(8);
        tokio::pin!(pending);
        let mut served = 0;
        let mut all_ok = true;
        loop {
            match tokio::time::timeout_at(deadline, pending.next()).await {
                Ok(Some((index, mut row, Some(Ok(raw)), mono, wall))) => {
                    let previous = baseline.get(&row.id);
                    let elapsed = previous
                        .map(|(_, t)| {
                            mono.duration_since(*t).as_millis().min(u32::MAX as u128) as u32
                        })
                        .unwrap_or(0);
                    row.metrics = normalize::normalize(&raw, previous.map(|(r, _)| r), elapsed);
                    row.observed_ms = Some(wall);
                    row.interval_ms = Some(elapsed);
                    baseline.insert(row.id.clone(), (raw, mono));
                    if let Some(target) = rows.get_mut(index) {
                        *target = row;
                    }
                    served += 1;
                }
                Ok(Some((index, mut row, Some(Err(_)), _, _))) => {
                    baseline.remove(&row.id);
                    row.metrics = DockerMetrics::default();
                    row.observed_ms = None;
                    if let Some(target) = rows.get_mut(index) {
                        *target = row;
                    }
                    served += 1;
                    all_ok = false;
                }
                Ok(Some((index, mut row, None, _, _))) => {
                    baseline.remove(&row.id);
                    row.metrics = DockerMetrics::default();
                    row.observed_ms = None;
                    row.interval_ms = None;
                    if let Some(target) = rows.get_mut(index) {
                        *target = row;
                    }
                    served += 1;
                }
                Ok(None) => break,
                Err(_) => {
                    all_ok = false;
                    break;
                }
            }
        }
        *cursor = (offset + served.max(1)) % ids.len();
        all_ok
    }
    fn record_event(
        &self,
        event: DockerEvent,
        restarts: &mut BTreeMap<String, VecDeque<i64>>,
        generation: u32,
    ) {
        let _gate = self.gate.lock_ok();
        if generation != self.control.borrow().generation {
            return;
        }
        let cutoff = now_ms() - 300_000;
        restarts.retain(|_, q| {
            q.retain(|t| *t >= cutoff);
            !q.is_empty()
        });
        let mut state = self.inner.lock_ok();
        if state.events.iter().any(|e| e.id == event.id) {
            return;
        }
        let mut dropped = false;
        let restarted = event.action == "restart"
            || (event.action == "start"
                && state
                    .events
                    .iter()
                    .rev()
                    .find(|e| {
                        e.engine_id == event.engine_id
                            && e.container_id == event.container_id
                            && matches!(e.action.as_str(), "start" | "die" | "stop" | "restart")
                    })
                    .is_some_and(|e| {
                        matches!(e.action.as_str(), "die" | "stop")
                            && event.time_ms.saturating_sub(e.time_ms) <= 300_000
                    }));
        if restarted && let Some(id) = &event.container_id {
            if restarts.contains_key(id) || restarts.len() < 4096 {
                let q = restarts.entry(id.clone()).or_default();
                if event.action != "restart"
                    || q.back()
                        .is_none_or(|last| event.time_ms.saturating_sub(*last) > 2000)
                {
                    q.push_back(event.time_ms);
                }
                while q.len() > 128 {
                    q.pop_front();
                }
            } else {
                dropped = true;
            }
        }
        state.events.push_back(event.clone());
        while state.events.len() > 4096 {
            state.events.pop_front();
        }
        drop(state);
        if dropped {
            self.gap("restart_evidence_dropped");
        }
        self.recorder.event(&event, generation);
        let _ = self.events.send(event);
    }
}
#[derive(PartialEq)]
enum SessionEnd {
    Changed,
    Idle,
    Disconnected,
}
fn unavailable_runtime(runtime: &str) -> DockerRuntimeFootprint {
    DockerRuntimeFootprint{scope:if runtime=="OrbStack"{"shared"}else{"unavailable"}.into(),explanation:if runtime=="OrbStack"{"OrbStack shares its runtime with other workloads. Docker-only process attribution has not been verified."}else{"Runtime process attribution is unavailable; container metrics do not measure the Mac runtime's footprint."}.into(),cpu_pct:None,footprint_bytes:None,observed_ms:None,processes:Vec::new()}
}
fn aggregate(
    rows: &[DockerContainer],
    capacity: Option<f64>,
    inventory_complete: bool,
    now: i64,
    interval: u32,
) -> DockerAggregate {
    let running = rows
        .iter()
        .filter(|c| c.lifecycle == DockerLifecycle::Running)
        .count() as u32;
    let expected = rows
        .iter()
        .filter(|c| c.monitored && c.lifecycle == DockerLifecycle::Running)
        .count() as u32;
    let fresh = |c: &&DockerContainer| {
        c.monitored
            && c.lifecycle == DockerLifecycle::Running
            && c.observed_ms
                .is_some_and(|t| now.saturating_sub(t) <= interval.max(5000) as i64 * 2)
    };
    let cpu: Vec<_> = rows
        .iter()
        .filter(fresh)
        .filter_map(|c| c.metrics.cpu_pct)
        .collect();
    let memory: Vec<_> = rows
        .iter()
        .filter(fresh)
        .filter_map(|c| c.metrics.working_set_bytes)
        .collect();
    DockerAggregate {
        monitoring_limit: 128,
        cpu_cores: (!cpu.is_empty()).then(|| cpu.iter().sum::<f64>() / 100.),
        working_set_bytes: (!memory.is_empty()).then(|| memory.iter().sum()),
        capacity_bytes: capacity,
        cpu_measured: cpu.len() as u32,
        memory_measured: memory.len() as u32,
        expected,
        running,
        total: rows.len() as u32,
        monitored: rows.iter().filter(|c| c.monitored).count() as u32,
        partial: expected < running
            || cpu.len() < expected as usize
            || memory.len() < expected as usize
            || !inventory_complete,
        inventory_complete,
    }
}

fn admit_event(rate: &mut (Instant, usize)) -> bool {
    if rate.0.elapsed() >= Duration::from_secs(1) {
        *rate = (Instant::now(), 0);
    }
    rate.1 += 1;
    rate.1 <= 1000
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn idle_engine_stays_fully_suspended_until_explicit_resume() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("docker.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let counter = count.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut conn, _) = listener.accept().await.unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut bytes = vec![];
                    let mut one = [0];
                    while conn.read_exact(&mut one).await.is_ok() {
                        bytes.push(one[0]);
                        if bytes.ends_with(b"\r\n\r\n") {
                            break;
                        }
                    }
                    let req = String::from_utf8_lossy(&bytes);
                    if req.contains("events") {
                        let _ = conn
                            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                            .await;
                        let _ = conn.read(&mut one).await;
                        return;
                    }
                    let body = if req.contains("version") {
                        r#"{"ApiVersion":"1.49","MinAPIVersion":"1.40"}"#
                    } else if req.contains("info") {
                        r#"{"ID":"idle-engine","MemTotal":4096}"#
                    } else {
                        "[]"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = conn.write_all(response.as_bytes()).await;
                });
            }
        });
        let handle = DockerHandle::new(
            HostId(uuid::Uuid::new_v4()),
            DockerSettings {
                enabled: true,
                selected_engine: Some(socket.to_string_lossy().into()),
                ..Default::default()
            },
            dir.path().to_path_buf(),
        );
        let mut status = handle.subscribe();
        let worker = tokio::spawn(handle.clone().run());
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                status.changed().await.unwrap();
                if status.borrow().status == DockerStatus::IdleSuspended {
                    break;
                }
            }
        })
        .await
        .unwrap();
        let before = count.load(Ordering::SeqCst);
        handle.set_visible(true);
        handle.set_power(false, 1000);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            count.load(Ordering::SeqCst),
            before,
            "visibility cannot wake idle daemon"
        );
        handle.resume();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                status.changed().await.unwrap();
                if status.borrow().status == DockerStatus::IdleSuspended
                    && count.load(Ordering::SeqCst) > before
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        handle.configure(DockerSettings::default()).unwrap();
        assert_eq!(handle.snapshot().status, DockerStatus::Disabled);
        handle.shutdown();
        worker.await.unwrap();
        server.abort();
    }
    #[test]
    fn aggregate_counts_missing_values_without_inventing_zero() {
        let total = aggregate(&[], None, false, 0, 1000);
        assert!(total.partial);
        assert_eq!(total.cpu_cores, None);
        assert_eq!(total.working_set_bytes, None);
    }
    #[tokio::test]
    #[ignore = "Read-only live worker verification against the installed local engine"]
    async fn installed_worker_normalizes_and_cancels() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let handle = DockerHandle::new(
            HostId(uuid::Uuid::new_v4()),
            DockerSettings::default(),
            home,
        );
        let engines = handle.discover().await;
        let engine = engines.iter().find(|e| e.preferred && e.reachable).unwrap();
        handle
            .configure(DockerSettings {
                enabled: true,
                selected_engine: Some(engine.socket.clone()),
                ..Default::default()
            })
            .unwrap();
        handle.set_visible(true);
        let mut status = handle.subscribe();
        let task = tokio::spawn(handle.clone().run());
        tokio::time::timeout(Duration::from_secs(10),async{loop{status.changed().await.unwrap();let snapshot=status.borrow();if snapshot.aggregate.cpu_measured>0{assert!(!snapshot.aggregate.partial);eprintln!("Worker measured {} running containers; CPU cores {:?}; memory bytes {:?}; generation {}",snapshot.aggregate.running,snapshot.aggregate.cpu_cores,snapshot.aggregate.working_set_bytes,snapshot.generation);break;}}}).await.unwrap();
        let old = handle.snapshot().generation;
        handle.configure(DockerSettings::default()).unwrap();
        assert!(handle.snapshot().generation > old);
        assert_eq!(handle.snapshot().status, DockerStatus::Disabled);
        handle.shutdown();
        task.await.unwrap();
    }
}

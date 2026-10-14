//! Independent, opt-in Docker clock. Finite polling, events and logs use separate budgets.
mod error;
pub use error::DockerServiceError;
mod event_lane;
#[cfg(test)]
mod integration_tests;
mod logs;
mod publication;
mod sampling;
mod worker;
use event_lane::admit_event;
use publication::unavailable_runtime;
#[cfg(test)]
use publication::{aggregate, project_aggregates};
use sampling::{Inventory, Samples};
mod normalize;
mod persistence;
pub mod storage;
use futures_util::{StreamExt, stream};
use kelvo_collect::docker::{DockerAdapter, DockerCounters, DockerError, candidates, now_ms};
use kelvo_schema::{HostId, docker::*, lock::LockExt};
pub use persistence::DockerRecorder;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{broadcast, watch},
    time::Instant,
};

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
    storage: Arc<storage::StorageManager>,
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
            project_aggregates: Vec::new(),
            runtime: unavailable_runtime(""),
            coverage: Vec::new(),
            observed_ms: now_ms(),
            now_ms: now_ms(),
            interval_ms: 10000,
            population_revision: 0,
            persistence: DockerPersistence {
                error: None,
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
            storage: Arc::new(storage::StorageManager::default()),
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
    pub fn configure(&self, settings: DockerSettings) -> Result<(), DockerServiceError> {
        if settings.selected_engine.as_ref().is_some_and(|s| {
            !std::path::Path::new(s).is_absolute() || s.contains("://") || s.len() > 4096
        }) {
            return Err(DockerError::Endpoint.into());
        }
        let _gate = self.gate.lock_ok();
        let current = self.control.borrow().settings.clone();
        if current.enabled == settings.enabled
            && current.selected_engine == settings.selected_engine
        {
            // Presentation preferences do not interrupt sampling, connection attempts,
            // log streams or pending actions. The next worker read sees the new settings.
            self.control.send_if_modified(|c| {
                c.settings = settings.clone();
                false
            });
            self.changed.send_modify(|s| {
                s.now_ms = now_ms();
                s.settings = settings;
            });
            return Ok(());
        }
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
            s.now_ms = now_ms();
            s.settings = settings;
            s.generation = c.generation;
            s.status = if c.settings.enabled {
                DockerStatus::Connecting
            } else {
                DockerStatus::Disabled
            };
            s.containers.clear();
            s.project_aggregates.clear();
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
            s.now_ms = now_ms();
            s.generation = generation;
            s.containers.clear();
            s.project_aggregates.clear();
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
        self.storage.resumed_generation.store(
            self.control.borrow().generation,
            std::sync::atomic::Ordering::SeqCst,
        );
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
    pub async fn detail_live(
        &self,
        engine: &str,
        id: &str,
    ) -> Result<DockerContainer, DockerServiceError> {
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
                .ok_or(DockerServiceError::NotFound(
                    "Container is not available in retained history",
                ));
        };
        if row.lifecycle == DockerLifecycle::Removed {
            return Ok(row);
        }
        let generation = self.control.borrow().generation;
        let adapter = self.inner.lock_ok().adapter.clone();
        let inspected = if let Some(adapter) = adapter.filter(|a| a.engine.id == engine) {
            match adapter.inspect_detail(row.clone()).await {
                Ok(fresh) => fresh,
                Err(DockerError::NotFound) => {
                    row.lifecycle = DockerLifecycle::Removed;
                    row.metadata_observed_ms = Some(now_ms());
                    row.observed_ms = None;
                    row.interval_ms = None;
                    row.metrics = DockerMetrics::default();
                    row
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            row
        };
        let _gate = self.gate.lock_ok();
        if self.control.borrow().generation != generation {
            return Err(DockerServiceError::SnapshotExpired(
                "Docker configuration changed during inspection",
            ));
        }
        let mut reconciled = inspected.clone();
        self.changed.send_modify(|s| {
            s.now_ms = now_ms();
            if let Some(live) = s
                .containers
                .iter_mut()
                .find(|c| c.engine_id == engine && c.id == id)
            {
                reconciled = reconcile(live.clone(), &inspected);
                *live = reconciled.clone();
            }
        });
        Ok(reconciled)
    }

    pub fn retained(
        &self,
        request: &DockerRetainedRequest,
    ) -> Result<DockerRetainedPage, DockerServiceError> {
        self.recorder.retained(request)
    }
    pub fn comparison(
        &self,
        request: &DockerComparisonRequest,
    ) -> Result<DockerComparisonPage, DockerServiceError> {
        self.recorder.comparison(request)
    }
    pub fn history(
        &self,
        request: &DockerHistoryRequest,
    ) -> Result<DockerHistoryPage, DockerServiceError> {
        self.recorder.history(request)
    }
    pub fn query_events(
        &self,
        r: &DockerEventsRequest,
    ) -> Result<DockerEventsPage, DockerServiceError> {
        self.recorder.events(r)
    }
    pub fn open_logs(
        &self,
        request: DockerLogsRequest,
    ) -> Result<LogSubscription, DockerServiceError> {
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
        snapshot.now_ms = now_ms();
        // A presentation preference can change while this sample is in flight.
        snapshot.settings = self.control.borrow().settings.clone();
        let current = self.changed.borrow();
        let inspected: BTreeMap<_, _> = current
            .containers
            .iter()
            .map(|row| ((row.engine_id.as_str(), row.id.as_str()), row))
            .collect();
        for row in &mut snapshot.containers {
            if let Some(previous) = inspected.get(&(row.engine_id.as_str(), row.id.as_str())) {
                *row = reconcile(row.clone(), previous);
            }
        }
        drop(inspected);
        drop(current);
        self.recorder.record(&snapshot);
        snapshot.persistence = self.recorder.status();
        self.changed.send_replace(snapshot);
    }
    fn gap(&self, reason: &str) {
        self.changed.send_modify(|s| {
            s.now_ms = now_ms();
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
            s.now_ms = now_ms();
            s.status = status;
            s.message = message;
            s.generation = generation;
        });
        self.gap(reason);
        true
    }
}
/// Inventory/stats and inspection carry separate evidence clocks.
fn reconcile(mut live: DockerContainer, inspected: &DockerContainer) -> DockerContainer {
    let newer_state = inspected
        .metadata_observed_ms
        .is_some_and(|t| t >= live.last_seen_ms);
    if newer_state && inspected.lifecycle == DockerLifecycle::Removed {
        return inspected.clone();
    }
    if newer_state {
        live.lifecycle = inspected.lifecycle;
        live.health = inspected.health;
    }
    let newer_metadata = inspected
        .metadata_observed_ms
        .is_some_and(|t| live.metadata_observed_ms.is_none_or(|old| t >= old));
    if inspected.metadata_available && (!live.metadata_available || newer_metadata) {
        live.metadata_available = inspected.metadata_available;
        live.metadata_observed_ms = inspected.metadata_observed_ms;
        live.auto_remove = inspected.auto_remove;
        live.restart_count = inspected.restart_count;
        live.oom_killed = inspected.oom_killed;
        live.explicit_limit_bytes = inspected.explicit_limit_bytes;
        live.cpu_quota = inspected.cpu_quota;
        live.restart_policy = inspected.restart_policy.clone();
        live.ports = inspected.ports.clone();
        live.mounts = inspected.mounts.clone();
        live.created = inspected.created.clone();
    }
    live
}
#[derive(PartialEq)]
enum SessionEnd {
    Changed,
    Idle,
    Disconnected,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn docker_interval_clock_advances_with_tokio_time() {
        let mut rate = (Instant::now(), 1000);
        assert!(!admit_event(&mut rate));
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(admit_event(&mut rate));
        assert_eq!(rate.1, 1);
    }
    #[test]
    fn display_preferences_preserve_collection_and_survive_inflight_samples() {
        let handle = DockerHandle::new(
            HostId(uuid::Uuid::new_v4()),
            DockerSettings {
                enabled: true,
                selected_engine: Some("/tmp/docker.sock".into()),
                ..Default::default()
            },
            PathBuf::new(),
        );
        handle.changed.send_modify(|s| {
            s.status = DockerStatus::Connected;
            s.observed_ms = 123;
            s.aggregate.running = 5;
            s.aggregate.expected = 5;
            s.coverage.push(DockerCoverage {
                start_ms: 10,
                end_ms: Some(20),
                container_id: None,
                reason: "disconnected".into(),
            });
        });
        let worker = handle.control.subscribe();
        let mut subscriber = handle.subscribe();
        for (popover, tray) in [(true, false), (true, true), (false, true)] {
            let mut before = handle.snapshot();
            let inflight = before.clone();
            let settings = DockerSettings {
                show_popover: popover,
                show_tray_count: tray,
                ..before.settings.clone()
            };
            handle.configure(settings.clone()).unwrap();
            assert!(
                !worker.has_changed().unwrap(),
                "display preferences wake the worker"
            );
            assert_eq!(worker.borrow().settings, settings);
            assert!(subscriber.has_changed().unwrap());
            let after = subscriber.borrow_and_update().clone();
            before.settings = settings.clone();
            before.now_ms = after.now_ms;
            assert_eq!(after, before, "display preferences reset observations");
            handle.publish(inflight);
            assert_eq!(
                handle.snapshot().settings,
                settings,
                "old sample reverts preferences"
            );
        }
        let mut disabled = handle.snapshot().settings;
        disabled.enabled = false;
        handle.configure(disabled).unwrap();
        assert!(worker.has_changed().unwrap());
        assert_eq!(handle.snapshot().generation, 2);
        assert_eq!(handle.snapshot().status, DockerStatus::Disabled);
    }
    #[test]
    fn transitions_publish_now_without_fabricating_observations() {
        let handle = DockerHandle::new(
            HostId(uuid::Uuid::new_v4()),
            DockerSettings::default(),
            PathBuf::new(),
        );
        handle.changed.send_modify(|s| {
            s.observed_ms = 123;
            s.now_ms = 0;
        });
        assert!(handle.transition(1, DockerStatus::Unavailable, None, "disconnected"));
        let snapshot = handle.snapshot();
        assert!(snapshot.now_ms > 123);
        assert_eq!(snapshot.observed_ms, 123);
        handle.changed.send_modify(|s| s.now_ms = 0);
        handle.publish(handle.snapshot());
        assert!(handle.snapshot().now_ms > 123);
        assert_eq!(handle.snapshot().observed_ms, 123);
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

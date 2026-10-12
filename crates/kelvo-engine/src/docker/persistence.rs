//! Bounded Docker ring and SQLite coordination. Persistence failures release write pins;
//! immutable reader snapshots own their own copies until the bounded query completes.
use kelvo_schema::{HostId, Tier, docker as api, lock::LockExt};
use kelvo_store::{Reader, Writer, docker as db};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const RING_BYTES: usize = 64 * 1024 * 1024;
const HOUR: i64 = 3_600_000;
type Key = (String, String, i64);
#[derive(Clone)]
struct Attached {
    writer: Writer,
    path: PathBuf,
    coordination: Arc<Mutex<()>>,
}
struct Entry {
    bucket: Arc<db::DockerBucket>,
    bytes: usize,
    pins: usize,
    durable: i64,
}
struct Receipt {
    rx: mpsc::Receiver<db::DockerAck>,
    keys: Vec<Key>,
    engine: String,
    start: i64,
    end: i64,
}
struct State {
    generation: u32,
    reset_epoch: u64,
    suspended: bool,
    generation_floor: u32,
    revision: i64,
    entries: BTreeMap<Key, Entry>,
    metadata: BTreeMap<(String, String), api::DockerContainer>,
    observed: BTreeMap<(String, String), i64>,
    fingerprints: BTreeMap<(String, String), Vec<u8>>,
    facts: BTreeMap<(String, String), db::DockerEntity>,
    metadata_history: VecDeque<(String, db::DockerEntity)>,
    population_facts: BTreeMap<(String, i64), db::DockerPopulation>,
    populations: BTreeMap<String, (i64, Vec<String>)>,
    events: VecDeque<api::DockerEvent>,
    coverage: BTreeMap<String, Vec<api::DockerCoverage>>,
    pending: Vec<Receipt>,
    bytes: usize,
    aux_bytes: usize,
    reader_bytes: usize,
    error: Option<String>,
    runtime: BTreeMap<String, String>,
}
struct ReadPin<'a> {
    state: &'a Mutex<State>,
    bytes: usize,
    generation: u32,
    reset_epoch: u64,
    started: Instant,
}
impl ReadPin<'_> {
    fn valid(&self) -> bool {
        let s = self.state.lock_ok();
        s.generation == self.generation
            && s.reset_epoch == self.reset_epoch
            && self.started.elapsed() <= Duration::from_secs(2)
    }
}
impl Drop for ReadPin<'_> {
    fn drop(&mut self) {
        let mut s = self.state.lock_ok();
        s.reader_bytes = s.reader_bytes.saturating_sub(self.bytes);
    }
}
pub struct DockerRecorder {
    host: HostId,
    epoch: AtomicI64,
    attached: Mutex<Option<Attached>>,
    local_coordination: Arc<Mutex<()>>,
    state: Mutex<State>,
}
impl DockerRecorder {
    pub fn new(host: HostId) -> Self {
        let epoch = wall_ms();
        Self {
            host,
            epoch: AtomicI64::new(epoch),
            attached: Mutex::new(None),
            local_coordination: Arc::new(Mutex::new(())),
            state: Mutex::new(State {
                generation: 0,
                reset_epoch: 0,
                suspended: false,
                generation_floor: 0,
                revision: epoch.saturating_mul(1000),
                entries: BTreeMap::new(),
                metadata: BTreeMap::new(),
                observed: BTreeMap::new(),
                fingerprints: BTreeMap::new(),
                facts: BTreeMap::new(),
                metadata_history: VecDeque::new(),
                population_facts: BTreeMap::new(),
                populations: BTreeMap::new(),
                events: VecDeque::new(),
                coverage: BTreeMap::new(),
                pending: Vec::new(),
                bytes: 0,
                aux_bytes: 0,
                reader_bytes: 0,
                error: None,
                runtime: BTreeMap::new(),
            }),
        }
    }
    /// Stop producers, settle earlier revisions, then persist the newest open
    /// buckets. Never hold the snapshot lock while waiting for a writer commit.
    pub fn shutdown_flush(&self) -> Result<(), String> {
        let (attached, coord) = self.context();
        let Some(a) = attached else {
            return Ok(());
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        {
            let _guard = coord.lock_ok();
            self.state.lock_ok().suspended = true;
        }
        a.writer
            .flush_docker(deadline.saturating_duration_since(Instant::now()))
            .map_err(|e| e.to_string())?;
        loop {
            {
                let _guard = coord.lock_ok();
                let mut s = self.state.lock_ok();
                drain(&mut s);
                let Some(engine) = s
                    .entries
                    .iter()
                    .find(|(_, e)| e.bucket.revision > e.durable)
                    .map(|((engine, _, _), _)| engine.clone())
                else {
                    return Ok(());
                };
                if Instant::now() >= deadline {
                    return Err("Docker shutdown flush timed out".into());
                }
                let keys: Vec<_> = s
                    .entries
                    .iter()
                    .filter(|((daemon, _, _), e)| {
                        daemon == &engine && e.pins == 0 && e.bucket.revision > e.durable
                    })
                    .take(64)
                    .map(|(k, _)| k.clone())
                    .collect();
                if keys.is_empty() {
                    return Err("Docker shutdown still has unacknowledged writes".into());
                }
                let generation = s.generation;
                let mut batch = self.batch(&mut s, &engine, generation, wall_ms());
                batch.buckets = keys
                    .iter()
                    .filter_map(|k| s.entries.get(k).map(|e| (*e.bucket).clone()))
                    .collect();
                let populations: BTreeSet<_> = batch
                    .buckets
                    .iter()
                    .flat_map(|b| b.populations.iter().map(|p| p.revision))
                    .collect();
                batch.populations = populations
                    .into_iter()
                    .filter_map(|revision| {
                        s.population_facts.get(&(engine.clone(), revision)).cloned()
                    })
                    .collect();
                let entities: BTreeSet<_> =
                    batch.buckets.iter().map(|b| b.entity.clone()).collect();
                batch.entities = entities
                    .into_iter()
                    .filter_map(|id| s.facts.get(&(engine.clone(), id)).cloned())
                    .collect();
                batch.start_ms = batch
                    .buckets
                    .iter()
                    .map(|b| b.bucket_ms)
                    .min()
                    .unwrap_or(batch.end_ms);
                queue(&mut s, Some(&a), batch, keys);
            }
            a.writer
                .flush_docker(deadline.saturating_duration_since(Instant::now()))
                .map_err(|e| e.to_string())?;
            let _guard = coord.lock_ok();
            let mut s = self.state.lock_ok();
            drain(&mut s);
            if let Some(error) = &s.error {
                return Err(error.clone());
            }
        }
    }
    /// Resume the live ring on every reset exit, including store/path failures.
    pub fn with_suspended<T>(&self, operation: impl FnOnce() -> T) -> T {
        struct Resume<'a>(&'a DockerRecorder);
        impl Drop for Resume<'_> {
            fn drop(&mut self) {
                self.0.resume();
            }
        }
        self.suspend();
        let _resume = Resume(self);
        operation()
    }
    pub fn suspend(&self) {
        let (_, coord) = self.context();
        let _g = coord.lock_ok();
        self.state.lock_ok().suspended = true;
    }
    pub fn resume(&self) {
        self.state.lock_ok().suspended = false;
    }
    pub fn detach(&self) {
        *self.attached.lock_ok() = None;
    }
    pub fn attach(&self, writer: Writer, path: PathBuf) {
        let coordination = writer.docker_snapshot_lock();
        if let Ok(reader) = Reader::open(&path)
            && let Ok((generation, revision)) = reader.docker_watermark(self.host)
        {
            self.epoch
                .fetch_max(generation.saturating_add(1), Ordering::AcqRel);
            let mut s = self.state.lock_ok();
            s.revision = s.revision.max(revision.saturating_add(1));
        }
        self.state.lock_ok().suspended = false;
        *self.attached.lock_ok() = Some(Attached {
            writer,
            path,
            coordination,
        });
    }
    fn context(&self) -> (Option<Attached>, Arc<Mutex<()>>) {
        let a = self.attached.lock_ok().clone();
        let c = a
            .as_ref()
            .map(|a| Arc::clone(&a.coordination))
            .unwrap_or_else(|| Arc::clone(&self.local_coordination));
        (a, c)
    }
    pub fn record(&self, snapshot: &api::DockerSnapshot) {
        let Some(engine) = &snapshot.engine else {
            return;
        };
        let Some(daemon) = &engine.daemon_id else {
            return;
        };
        let (attached, coord) = self.context();
        let _guard = coord.lock_ok();
        let mut s = self.state.lock_ok();
        if s.suspended || snapshot.generation < s.generation_floor {
            return;
        }
        drain(&mut s);
        s.generation = snapshot.generation;
        s.runtime.insert(daemon.clone(), engine.runtime.clone());
        let now = snapshot.observed_ms;
        evict(&mut s, now);
        let mut batch = self.batch(&mut s, daemon, snapshot.generation, now);
        let capacity = engine
            .capacity_bytes
            .filter(|v| v.is_finite() && *v >= 0.)
            .map(|v| v as i64);
        if s.facts
            .get(&(daemon.clone(), String::new()))
            .is_none_or(|e| e.capacity != capacity)
        {
            let fact = db::DockerEntity {
                external_id: String::new(),
                effective_ms: engine.capacity_observed_ms.unwrap_or(now).min(now),
                revision: next(&mut s),
                name: engine.runtime.clone(),
                capacity,
                last_seen_ms: now,
                ..Default::default()
            };
            s.facts
                .insert((daemon.clone(), String::new()), fact.clone());
            s.metadata_history.push_back((daemon.clone(), fact.clone()));
            batch.entities.push(fact);
        }

        let members: Vec<_> = snapshot
            .containers
            .iter()
            .filter(|c| c.monitored)
            .map(|c| c.id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if s.populations
            .get(daemon)
            .is_none_or(|(_, old)| *old != members)
        {
            let revision = next(&mut s);
            s.populations.insert(daemon.clone(), (revision, members));
        }
        let (population, members) = s.populations.get(daemon).cloned().unwrap_or_default();
        let fact = db::DockerPopulation {
            revision: population,
            effective_ms: now,
            members,
        };
        s.population_facts
            .entry((daemon.clone(), population))
            .or_insert(fact);
        for c in &snapshot.containers {
            metadata(&mut s, &mut batch, daemon, c);
            if !c.monitored {
                continue;
            }
            let Some(time) = c.observed_ms else {
                continue;
            };
            let span = c.interval_ms.unwrap_or(0).min(60_000) as i64;
            if span == 0
                || s.observed
                    .get(&(daemon.clone(), c.id.clone()))
                    .is_some_and(|v| *v >= time)
            {
                continue;
            }
            s.observed.insert((daemon.clone(), c.id.clone()), time);
            observe(&mut s, daemon, &c.id, time, span, &c.metrics, None);
        }
        if !s
            .observed
            .get(&(daemon.clone(), String::new()))
            .is_some_and(|v| *v >= now)
        {
            s.observed.insert((daemon.clone(), String::new()), now);
            let metrics = api::DockerMetrics {
                cpu_pct: snapshot.aggregate.cpu_cores.map(|v| v * 100.),
                working_set_bytes: snapshot.aggregate.working_set_bytes,
                ..Default::default()
            };
            observe(
                &mut s,
                daemon,
                "",
                now,
                snapshot.interval_ms.min(60_000) as i64,
                &metrics,
                Some((
                    population,
                    snapshot.aggregate.cpu_measured == snapshot.aggregate.expected,
                    snapshot.aggregate.memory_measured == snapshot.aggregate.expected,
                )),
            );
        }
        let old_coverage = s.coverage.get(daemon).cloned().unwrap_or_default();
        for c in &snapshot.coverage {
            if !old_coverage.contains(c) {
                batch.coverage.push(db::DockerCoverage {
                    entity: c.container_id.clone(),
                    metric: None,
                    start_ms: c.start_ms,
                    end_ms: c.end_ms,
                    reason: c.reason.clone(),
                    revision: next(&mut s),
                    sampled_ms: 0,
                    expected_ms: c.end_ms.unwrap_or(now).saturating_sub(c.start_ms),
                });
            }
        }
        let gaps = s.coverage.entry(daemon.clone()).or_default();
        for c in &snapshot.coverage {
            if let Some(old) = gaps.iter_mut().find(|old| {
                old.start_ms == c.start_ms
                    && old.container_id == c.container_id
                    && old.reason == c.reason
            }) {
                *old = c.clone();
            } else {
                gaps.push(c.clone());
            }
        }
        bound_coverage(gaps);
        let cut = now.div_euclid(10_000) * 10_000;
        let keys: Vec<_> = s
            .entries
            .iter()
            .filter(|((e, _, t), v)| {
                e == daemon && *t < cut && v.pins == 0 && v.bucket.revision > v.durable
            })
            .map(|(k, _)| k.clone())
            .collect();
        for key in &keys {
            if let Some(e) = s.entries.get(key) {
                batch.buckets.push((*e.bucket).clone());
            }
        }
        let needed: BTreeSet<_> = batch
            .buckets
            .iter()
            .flat_map(|b| b.populations.iter().map(|p| p.revision))
            .collect();
        batch.populations = needed
            .into_iter()
            .filter_map(|revision| s.population_facts.get(&(daemon.clone(), revision)).cloned())
            .collect();
        let needed: BTreeSet<_> = batch.buckets.iter().map(|b| b.entity.clone()).collect();
        for id in needed {
            if !batch.entities.iter().any(|e| e.external_id == id)
                && let Some(fact) = s.facts.get(&(daemon.clone(), id))
            {
                batch.entities.push(fact.clone());
            }
        }
        recount_aux(&mut s);
        evict(&mut s, now);
        batch.revision = s.revision;
        batch.start_ms = batch
            .buckets
            .iter()
            .map(|b| b.bucket_ms)
            .min()
            .unwrap_or(now);
        queue(&mut s, attached.as_ref(), batch, keys);
    }
    fn batch(&self, s: &mut State, daemon: &str, generation: u32, time: i64) -> db::DockerBatch {
        db::DockerBatch {
            host: self.host,
            daemon_id: daemon.into(),
            generation: self.epoch.load(Ordering::Acquire) + i64::from(generation),
            revision: next(s),
            start_ms: time,
            end_ms: time,
            runtime: s
                .runtime
                .get(daemon)
                .cloned()
                .unwrap_or_else(|| "Docker".into()),
            entities: Vec::new(),
            buckets: Vec::new(),
            events: Vec::new(),
            coverage: Vec::new(),
            cursor: None,
            populations: Vec::new(),
        }
    }
    pub fn clock_step(&self, from_ms: i64) {
        let (a, coord) = self.context();
        let _g = coord.lock_ok();
        let mut s = self.state.lock_ok();
        s.observed.clear();
        let remove: Vec<_> = s
            .entries
            .keys()
            .filter(|(_, _, t)| *t + 10_000 > from_ms)
            .cloned()
            .collect();
        for key in remove {
            if let Some(e) = s.entries.remove(&key) {
                s.bytes = s.bytes.saturating_sub(e.bytes);
            }
        }
        s.metadata_history.retain(|(_, e)| e.effective_ms < from_ms);
        s.facts.clear();
        for (engine, fact) in s.metadata_history.clone() {
            s.facts.insert((engine, fact.external_id.clone()), fact);
        }
        // Reseed current observations at their new wall time, even when values did
        // not change. A future fingerprint must never suppress its replacement.
        s.fingerprints.clear();
        s.metadata.clear();
        s.populations.clear();
        s.population_facts.retain(|_, p| p.effective_ms < from_ms);
        for gaps in s.coverage.values_mut() {
            gaps.retain(|g| g.start_ms < from_ms);
            for gap in gaps {
                if gap.end_ms.is_none_or(|end| end > from_ms) {
                    gap.end_ms = Some(from_ms);
                }
            }
        }
        recount_aux(&mut s);
        s.events.retain(|e| e.time_ms < from_ms);
        s.reset_epoch = s.reset_epoch.wrapping_add(1);
        if let Some(a) = a
            && let Err(e) = a.writer.discard_from_soon(self.host, from_ms)
        {
            s.error = Some(e.to_string());
        }
    }
    pub fn removed(&self, c: &api::DockerContainer, generation: u32) {
        let (a, coord) = self.context();
        let _g = coord.lock_ok();
        let mut s = self.state.lock_ok();
        if s.suspended || generation < s.generation_floor {
            return;
        }
        let mut batch = self.batch(&mut s, &c.engine_id, generation, c.last_seen_ms);
        metadata(&mut s, &mut batch, &c.engine_id, c);
        batch.revision = s.revision;
        queue(&mut s, a.as_ref(), batch, Vec::new());
    }
    pub fn event(&self, event: &api::DockerEvent, generation: u32) {
        let (a, coord) = self.context();
        let _g = coord.lock_ok();
        let mut s = self.state.lock_ok();
        drain(&mut s);
        if s.suspended || generation < s.generation_floor {
            return;
        }
        if s.events.iter().any(|e| e.id == event.id) {
            return;
        }
        let attributes = event.attributes.clone();
        let Ok(time_ns) = event.time_ns.parse() else {
            return;
        };
        let e = db::DockerEvent {
            entity: event.container_id.clone(),
            time_ns,
            action: event.action.clone(),
            attributes,
        };
        let Ok(key) = e.key(&event.engine_id) else {
            return;
        };
        let mut event = event.clone();
        event.id = key.clone();
        s.events.push_back(event.clone());
        while s.events.len() > 4096 {
            s.events.pop_front();
        }
        recount_aux(&mut s);
        evict(&mut s, event.time_ms);
        let mut batch = self.batch(&mut s, &event.engine_id, generation, event.time_ms);
        batch.cursor = Some(db::DockerCursor {
            time_ns,
            event_key: key,
        });
        if let Some(id) = &event.container_id
            && let Some(fact) = s.facts.get(&(event.engine_id.clone(), id.clone()))
        {
            batch.entities.push(fact.clone());
        }
        batch.events.push(e);
        queue(&mut s, a.as_ref(), batch, Vec::new());
    }
    pub fn status(&self) -> api::DockerPersistence {
        let (a, coord) = self.context();
        let _g = coord.lock_ok();
        let mut s = self.state.lock_ok();
        drain(&mut s);
        let status = a.map(|a| a.writer.docker_storage_status());
        api::DockerPersistence {
            status: if s.error.is_some() {
                "write_failed"
            } else if status.as_ref().is_some_and(|v| v.paused) {
                "paused"
            } else if status.is_some() {
                "recording"
            } else {
                "live_only"
            }
            .into(),
            logical_bytes: status.as_ref().map_or(0., |v| v.logical_bytes as f64),
            allowance_bytes: status.map_or(37_500_000., |v| v.allowance_bytes as f64),
            retained_from_ms: s.entries.keys().map(|(_, _, t)| *t).min(),
        }
    }
    pub fn reset(&self, generation: u32) -> Result<(), String> {
        let (a, coord) = self.context();
        {
            let _g = coord.lock_ok();
            let mut s = self.state.lock_ok();
            s.generation = generation;
            s.generation_floor = generation;
            s.reset_epoch = s.reset_epoch.wrapping_add(1);
            s.entries.clear();
            s.metadata.clear();
            s.observed.clear();
            s.fingerprints.clear();
            s.facts.clear();
            s.metadata_history.clear();
            s.population_facts.clear();
            s.populations.clear();
            s.events.clear();
            s.coverage.clear();
            s.pending.clear();
            s.bytes = 0;
            s.aux_bytes = 0;
        }
        if let Some(a) = a {
            a.writer
                .reset_docker(
                    self.host,
                    self.epoch.load(Ordering::Acquire) + i64::from(generation),
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    /// Captures retained metadata without contacting a daemon or enabling collection.
    pub fn retained(
        &self,
        q: &api::DockerRetainedRequest,
    ) -> Result<api::DockerRetainedPage, String> {
        if q.limit == 0
            || q.limit > 100
            || q.offset > 100_000
            || q.search.len() > 128
            || q.engine_id.as_ref().is_some_and(|id| id.len() > 256)
        {
            return Err("Retained catalog search or page bounds exceeded".into());
        }
        let (attached, coordination) = self.context();
        let mut reader = attached
            .as_ref()
            .map(|a| Reader::open(&a.path))
            .transpose()
            .map_err(|e| e.to_string())?;
        let guard = coordination.lock_ok();
        let snapshot = reader
            .as_mut()
            .map(|r| r.docker_snapshot(self.host, q.engine_id.as_deref().unwrap_or("")))
            .transpose()
            .map_err(|e| e.to_string())?;
        let mut state = self.state.lock_ok();
        drain(&mut state);
        let mut engine_count = 0usize;
        let mut descriptor_bytes = 0usize;
        let mut previous_engine = None;
        for (engine, id) in state.facts.keys() {
            if id.is_empty() || previous_engine == Some(engine) {
                continue;
            }
            previous_engine = Some(engine);
            engine_count += 1;
            if engine_count > 256 {
                return Err("Retained catalog engine bound exceeded".into());
            }
            descriptor_bytes +=
                engine.len() + state.runtime.get(engine).map_or(22, String::len) + 64;
        }
        let held = state
            .facts
            .iter()
            .filter(|((engine, id), _)| {
                !id.is_empty()
                    && q.engine_id
                        .as_ref()
                        .is_none_or(|selected| selected == engine)
            })
            .map(|(_, fact)| fact.extras.len() + 512)
            .sum::<usize>()
            .saturating_add(descriptor_bytes);
        if held > db::MAX_QUERY_BYTES
            || state
                .bytes
                .saturating_add(state.aux_bytes)
                .saturating_add(state.reader_bytes)
                .saturating_add(held)
                > RING_BYTES
        {
            return Err("Retained catalog snapshot byte limit reached".into());
        }
        state.reader_bytes += held;
        let pin = ReadPin {
            state: &self.state,
            bytes: held,
            generation: state.generation,
            reset_epoch: state.reset_epoch,
            started: Instant::now(),
        };
        let overlay: Vec<_> = state
            .facts
            .iter()
            .filter(|((engine, id), _)| {
                !id.is_empty()
                    && q.engine_id
                        .as_ref()
                        .is_none_or(|selected| selected == engine)
            })
            .map(|((engine, _), fact)| db::DockerCatalogEntity {
                engine_id: engine.clone(),
                metadata: fact.clone(),
            })
            .collect();
        let engines: Vec<_> = state
            .facts
            .keys()
            .filter(|(_, id)| !id.is_empty())
            .map(|(engine, _)| engine.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|id| db::DockerCatalogEngine {
                runtime: state
                    .runtime
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| "Retained Docker engine".into()),
                id,
            })
            .collect();
        drop(state);
        drop(guard);
        let catalog = if let Some(snapshot) = &snapshot {
            snapshot
                .catalog_with_overlay(
                    q.engine_id.as_deref(),
                    &q.search,
                    q.offset as usize,
                    q.limit as usize,
                    &overlay,
                    &engines,
                )
                .map_err(|e| e.to_string())?
        } else {
            let search = q.search.to_ascii_lowercase();
            let mut entities: Vec<_> = overlay
                .into_iter()
                .filter(|row| {
                    let m = &row.metadata;
                    format!(
                        "{} {} {} {} {}",
                        m.name,
                        m.image,
                        m.project.as_deref().unwrap_or(""),
                        m.service.as_deref().unwrap_or(""),
                        m.external_id
                    )
                    .to_ascii_lowercase()
                    .contains(&search)
                })
                .collect();
            entities.sort_by(|a, b| {
                (
                    a.metadata.name.to_ascii_lowercase(),
                    &a.engine_id,
                    &a.metadata.external_id,
                )
                    .cmp(&(
                        b.metadata.name.to_ascii_lowercase(),
                        &b.engine_id,
                        &b.metadata.external_id,
                    ))
            });
            let total = entities.len();
            let entities = entities
                .into_iter()
                .skip(q.offset as usize)
                .take(q.limit as usize)
                .collect();
            db::DockerCatalog {
                engines,
                entities,
                total,
            }
        };
        let mut containers = Vec::with_capacity(catalog.entities.len());
        for row in catalog.entities {
            let mut container: api::DockerContainer = serde_json::from_slice(&row.metadata.extras)
                .map_err(|_| "Retained container metadata is unavailable")?;
            container.engine_id = row.engine_id;
            container.metrics = api::DockerMetrics::default();
            container.monitored = false;
            containers.push(container);
        }
        let consumed = q.offset.saturating_add(containers.len() as u32);
        let total = catalog.total.min(u32::MAX as usize) as u32;
        let page = api::DockerRetainedPage {
            engines: catalog
                .engines
                .into_iter()
                .map(|e| api::DockerRetainedEngine {
                    id: e.id,
                    runtime: e.runtime,
                })
                .collect(),
            containers,
            total,
            next_offset: (consumed < total).then_some(consumed),
        };
        if !pin.valid() {
            return Err("History was reset or catalog read expired".into());
        }
        if serde_json::to_vec(&page).map_err(|e| e.to_string())?.len() > db::MAX_QUERY_BYTES {
            return Err("Retained catalog response byte limit exceeded".into());
        }
        Ok(page)
    }
    pub fn detail(&self, engine: &str, id: &str) -> Option<api::DockerContainer> {
        if let Some(c) = self
            .state
            .lock_ok()
            .metadata
            .get(&(engine.into(), id.into()))
            .cloned()
        {
            return Some(c);
        }
        let now = wall_ms();
        self.history(&api::DockerHistoryRequest {
            engine_id: engine.into(),
            container_id: Some(id.into()),
            from_ms: now - db::MAX_RANGE_MS + 1,
            to_ms: now + 1,
            max_points: 1000,
        })
        .ok()?
        .metadata
        .pop()
    }
    pub fn history(&self, q: &api::DockerHistoryRequest) -> Result<api::DockerHistoryPage, String> {
        if q.to_ms <= q.from_ms
            || q.to_ms.saturating_sub(q.from_ms) > db::MAX_RANGE_MS
            || q.max_points == 0
            || q.max_points > 10_000
        {
            return Err("History request exceeds range or point bounds".into());
        }
        let started = Instant::now();
        let range = q.to_ms - q.from_ms;
        let desired = range / i64::from(q.max_points);
        let tier = if desired <= 10_000 && range <= 86_400_000 {
            Tier::S10
        } else if desired <= 60_000 && range <= 7 * 86_400_000 {
            Tier::M1
        } else {
            Tier::M15
        };
        // Capture enough predecessor ring data for a retained coarser tier too.
        let from = q.from_ms.div_euclid(900_000) * 900_000;
        let (a, coord) = self.context();
        let mut reader = a
            .as_ref()
            .map(|a| Reader::open(&a.path))
            .transpose()
            .map_err(|e| e.to_string())?;
        let guard = coord.lock_ok();
        let snapshot = reader
            .as_mut()
            .map(|r| r.docker_snapshot(self.host, &q.engine_id))
            .transpose()
            .map_err(|e| e.to_string())?;
        let mut s = self.state.lock_ok();
        drain(&mut s);
        let generation = s.generation;
        let reset_epoch = s.reset_epoch;
        let entity = q.container_id.as_deref().unwrap_or("");
        let ring: Vec<_> = s
            .entries
            .iter()
            .filter(|((engine, id, time), _)| {
                engine == &q.engine_id && id == entity && *time >= from && *time < q.to_ms
            })
            .map(|(_, e)| Arc::clone(&e.bucket))
            .collect();
        let held: usize = ring.iter().map(|b| bucket_bytes(b)).sum::<usize>()
            + s.metadata_history
                .iter()
                .filter(|(engine, e)| engine == &q.engine_id && e.external_id == entity)
                .map(|(_, e)| e.extras.len() + 512)
                .sum::<usize>();
        if held > db::MAX_QUERY_BYTES
            || s.reader_bytes
                .saturating_add(s.bytes)
                .saturating_add(s.aux_bytes)
                .saturating_add(held)
                > RING_BYTES
        {
            return Err("History snapshot byte limit reached".into());
        }
        s.reader_bytes += held;
        let metadata: Vec<_> = s
            .metadata_history
            .iter()
            .filter(|(engine, e)| {
                engine == &q.engine_id && e.external_id == entity && e.effective_ms < q.to_ms
            })
            .map(|(_, e)| e.clone())
            .collect();
        let coverage = s.coverage.get(&q.engine_id).cloned().unwrap_or_default();
        drop(s);
        drop(guard);
        let result = (|| {
            let mut tier = tier;
            let mut from = q.from_ms.div_euclid(tier.bucket_ms().unwrap_or(10_000))
                * tier.bucket_ms().unwrap_or(10_000);
            let mut persisted = if let Some(snapshot) = &snapshot {
                snapshot
                    .history(&db::DockerHistoryQuery {
                        entity: entity.into(),
                        start_ms: from,
                        end_ms: q.to_ms,
                        tier,
                        max_points: 10_000,
                    })
                    .map_err(|e| e.to_string())?
            } else {
                db::DockerHistory::default()
            };
            if let Some(snapshot) = &snapshot {
                // Retention and pressure remove tiers independently. Prefer a
                // coarser whole range whenever it contains evidence missing from
                // the selected tier, avoiding overlapping mixed-tier additions.
                for candidate in [Tier::M1, Tier::M15] {
                    let width = candidate.bucket_ms().unwrap_or(900_000);
                    if width <= tier.bucket_ms().unwrap_or(10_000) {
                        continue;
                    }
                    let start = q.from_ms.div_euclid(width) * width;
                    let older = snapshot
                        .history(&db::DockerHistoryQuery {
                            entity: entity.into(),
                            start_ms: start,
                            end_ms: q.to_ms,
                            tier: candidate,
                            max_points: 10_000,
                        })
                        .map_err(|e| e.to_string())?;
                    let end = (q.to_ms - 1).div_euclid(width).saturating_add(1) * width;
                    let fine_width = tier.bucket_ms().unwrap_or(10000);
                    let comparable = if (end - start) / fine_width <= 10000 {
                        snapshot
                            .history(&db::DockerHistoryQuery {
                                entity: entity.into(),
                                start_ms: start,
                                end_ms: end,
                                tier,
                                max_points: 10000,
                            })
                            .map_err(|e| e.to_string())?
                            .buckets
                    } else {
                        persisted.buckets.clone()
                    };
                    let finer = fold_ring(
                        &comparable.into_iter().map(Arc::new).collect::<Vec<_>>(),
                        candidate,
                    );
                    let additional = older.buckets.iter().any(|b| {
                        finer
                            .iter()
                            .find(|fine| fine.bucket_ms == b.bucket_ms)
                            .is_none_or(|fine| {
                                b.metrics.iter().any(|m| {
                                    fine.metrics
                                        .iter()
                                        .find(|n| n.key == m.key)
                                        .is_none_or(|n| n.weight_ms < m.weight_ms)
                                })
                            })
                    });
                    if additional {
                        tier = candidate;
                        from = start;
                        persisted = older;
                    }
                }
            }
            let width = tier.bucket_ms().unwrap_or(10_000);
            let ring: Vec<_> = ring
                .iter()
                .filter(|b| b.bucket_ms >= from)
                .cloned()
                .collect();
            let mut buckets: BTreeMap<i64, db::DockerBucket> = persisted
                .buckets
                .into_iter()
                .map(|b| (b.bucket_ms, b))
                .collect();
            // Capture covered fine buckets from the *same* SQLite snapshot, then
            // replace whole fine revisions with ring revisions before coarse folding.
            let mut fine: BTreeMap<i64, Arc<db::DockerBucket>> = BTreeMap::new();
            if tier != Tier::S10 && !ring.is_empty() {
                let left = ring
                    .iter()
                    .map(|b| b.bucket_ms.div_euclid(width) * width)
                    .min()
                    .unwrap_or(from);
                let right = ring
                    .iter()
                    .map(|b| (b.bucket_ms.div_euclid(width) + 1) * width)
                    .max()
                    .unwrap_or(q.to_ms);
                if let Some(snapshot) = &snapshot {
                    for b in snapshot
                        .history(&db::DockerHistoryQuery {
                            entity: entity.into(),
                            start_ms: left,
                            end_ms: right,
                            tier: Tier::S10,
                            max_points: 10_000,
                        })
                        .map_err(|e| e.to_string())?
                        .buckets
                    {
                        fine.insert(b.bucket_ms, Arc::new(b));
                    }
                }
            }
            for b in &ring {
                if fine
                    .get(&b.bucket_ms)
                    .is_none_or(|old| b.revision > old.revision)
                {
                    fine.insert(b.bucket_ms, Arc::clone(b));
                }
            }
            let folded = fold_ring(&fine.into_values().collect::<Vec<_>>(), tier);
            for b in folded {
                // Pressure may have removed fine rows while retaining the coarse span.
                // Never overwrite that coarse evidence with a smaller fine population.
                let covers_old = buckets.get(&b.bucket_ms).is_none_or(|old| {
                    old.metrics.iter().all(|m| {
                        b.metrics
                            .iter()
                            .any(|n| n.key == m.key && n.weight_ms >= m.weight_ms)
                    })
                });
                if buckets
                    .get(&b.bucket_ms)
                    .is_none_or(|old| covers_old && b.revision > old.revision)
                {
                    buckets.insert(b.bucket_ms, b);
                }
            }
            let mut display_width = width;
            while buckets.len() > q.max_points as usize {
                display_width = display_width.saturating_mul(2);
                let mut coarse: BTreeMap<i64, db::DockerBucket> = BTreeMap::new();
                for b in buckets.into_values() {
                    let at = b.bucket_ms.div_euclid(display_width) * display_width;
                    let target = coarse.entry(at).or_insert_with(|| db::DockerBucket {
                        entity: b.entity.clone(),
                        tier,
                        bucket_ms: at,
                        revision: 0,
                        metrics: Vec::new(),
                        populations: Vec::new(),
                    });
                    target.revision = target.revision.max(b.revision);
                    for m in b.metrics {
                        if let Some(old) = target.metrics.iter_mut().find(|old| old.key == m.key) {
                            old.merge(&m).map_err(|e| e.to_string())?;
                        } else {
                            target.metrics.push(m);
                        }
                    }
                    target.populations.extend(b.populations);
                }
                for b in coarse.values_mut() {
                    for m in &mut b.metrics {
                        m.expected_ms = display_width as u64;
                    }
                }
                buckets = coarse;
            }
            let mut points = Vec::new();
            for b in buckets.into_values() {
                let mut p = point(&b);
                p.duration_ms = display_width.min(i64::from(u32::MAX)) as u32;
                p.expected_ms = p.duration_ms;
                points.push(p);
            }
            if points.len() > q.max_points as usize {
                return Err("Select a coarser history range or more points".into());
            }
            let mut meta_rows: BTreeMap<(i64, i64), db::DockerEntity> = persisted
                .metadata
                .into_iter()
                .chain(metadata)
                .map(|e| ((e.effective_ms, e.revision), e))
                .collect();
            let predecessor = meta_rows
                .keys()
                .rfind(|(time, _)| *time < q.from_ms)
                .copied();
            meta_rows.retain(|key, _| key.0 >= q.from_ms || Some(*key) == predecessor);
            for point in &mut points {
                point.engine_capacity_bytes = meta_rows
                    .range(..=(point.time_ms, i64::MAX))
                    .next_back()
                    .and_then(|(_, e)| e.capacity.map(|v| v as f64));
                point.explicit_limit_bytes = meta_rows
                    .range(..=(point.time_ms, i64::MAX))
                    .next_back()
                    .and_then(|(_, e)| e.explicit_limit.map(|v| v as f64));
            }
            let metas: Vec<api::DockerContainer> = meta_rows
                .values()
                .filter_map(|e| serde_json::from_slice(&e.extras).ok())
                .collect();
            let mut gaps: Vec<_> = persisted
                .coverage
                .into_iter()
                .map(|c| api::DockerCoverage {
                    start_ms: c.start_ms,
                    end_ms: c.end_ms,
                    container_id: c.entity,
                    reason: c.reason,
                })
                .collect();
            for c in coverage {
                if !gaps.contains(&c) {
                    gaps.push(c);
                }
            }
            if started.elapsed() > Duration::from_secs(2) {
                return Err("History snapshot expired".into());
            }
            let mut persistence = self.status();
            let durable_start = snapshot
                .as_ref()
                .map(|snap| snap.retained_range())
                .transpose()
                .map_err(|e| e.to_string())?
                .flatten()
                .map(|(start, _)| start);
            persistence.retained_from_ms = durable_start
                .into_iter()
                .chain(ring.iter().map(|b| b.bucket_ms))
                .min();
            let page = api::DockerHistoryPage {
                generation,
                points,
                coverage: gaps,
                metadata: metas,
                persistence,
            };
            if serde_json::to_vec(&page).map_err(|e| e.to_string())?.len() > db::MAX_QUERY_BYTES {
                return Err("History response byte limit exceeded".into());
            }
            Ok(page)
        })();
        drop(snapshot);
        let mut s = self.state.lock_ok();
        s.reader_bytes = s.reader_bytes.saturating_sub(held);
        if s.generation != generation || s.reset_epoch != reset_epoch {
            return Err("History was reset or engine changed".into());
        }
        result
    }
    /// Durable cursor only; callers may replay from it but must still disclose Docker's
    /// bounded replay uncertainty. Invoke from a blocking worker, never a sampler.
    pub fn event_cursor(&self, engine: &str) -> Option<String> {
        let (a, coord) = self.context();
        let a = a?;
        let mut reader = Reader::open(&a.path).ok()?;
        let _g = coord.lock_ok();
        reader
            .docker_snapshot(self.host, engine)
            .ok()?
            .cursor()
            .ok()?
            .map(|c| c.time_ns.to_string())
    }
    pub fn events(&self, q: &api::DockerEventsRequest) -> Result<api::DockerEventsPage, String> {
        if q.to_ms <= q.from_ms || q.to_ms.saturating_sub(q.from_ms) > db::MAX_RANGE_MS {
            return Err("Invalid events range".into());
        }
        let (a, coord) = self.context();
        let mut reader = a
            .as_ref()
            .map(|a| Reader::open(&a.path))
            .transpose()
            .map_err(|e| e.to_string())?;
        let _g = coord.lock_ok();
        let snapshot = reader
            .as_mut()
            .map(|r| r.docker_snapshot(self.host, &q.engine_id))
            .transpose()
            .map_err(|e| e.to_string())?;
        let mut s = self.state.lock_ok();
        let ring: Vec<_> = s
            .events
            .iter()
            .filter(|e| {
                e.engine_id == q.engine_id
                    && e.time_ms >= q.from_ms
                    && e.time_ms < q.to_ms
                    && q.container_id
                        .as_ref()
                        .is_none_or(|id| e.container_id.as_ref() == Some(id))
            })
            .cloned()
            .collect();
        let coverage = s.coverage.get(&q.engine_id).cloned().unwrap_or_default();
        let held = ring
            .iter()
            .map(|e| serde_json::to_vec(e).map_or(db::MAX_QUERY_BYTES, |v| v.len() + 128))
            .sum::<usize>()
            + coverage.len() * 512;
        if held > db::MAX_QUERY_BYTES || s.bytes + s.aux_bytes + s.reader_bytes + held > RING_BYTES
        {
            return Err("Events snapshot byte limit reached".into());
        }
        s.reader_bytes += held;
        let pin = ReadPin {
            state: &self.state,
            bytes: held,
            generation: s.generation,
            reset_epoch: s.reset_epoch,
            started: Instant::now(),
        };
        drop(s);
        drop(_g);
        let after = q
            .before_id
            .as_ref()
            .and_then(|s| s.split_once(':'))
            .and_then(|(t, id)| Some((t.parse::<i64>().ok()?, id.to_owned())));
        let mut coverage = coverage;
        if let Some(snapshot) = &snapshot {
            for c in snapshot
                .event_coverage(q.container_id.as_deref(), q.from_ms, q.to_ms)
                .map_err(|e| e.to_string())?
            {
                let gap = api::DockerCoverage {
                    start_ms: c.start_ms,
                    end_ms: c.end_ms,
                    container_id: c.entity,
                    reason: c.reason,
                };
                if !coverage.iter().any(|live| {
                    live.start_ms == gap.start_ms
                        && live.container_id == gap.container_id
                        && live.reason == gap.reason
                }) {
                    coverage.push(gap);
                }
            }
        }
        coverage.retain(|c| {
            c.start_ms < q.to_ms
                && c.end_ms.is_none_or(|end| end > q.from_ms)
                && q.container_id
                    .as_ref()
                    .is_none_or(|id| c.container_id.as_ref().is_none_or(|owner| owner == id))
        });
        if coverage.len() > 10_000 {
            return Err("Events coverage count exceeded".into());
        }
        let rows = if let Some(snapshot) = &snapshot {
            snapshot
                .events(&db::DockerEventQuery {
                    entity: q.container_id.clone(),
                    start_ms: q.from_ms,
                    end_ms: q.to_ms,
                    after: after.clone(),
                    limit: q.limit.min(1000) as usize,
                })
                .map_err(|e| e.to_string())?
        } else {
            Vec::new()
        };
        let mut events: BTreeMap<String, api::DockerEvent> = rows
            .into_iter()
            .map(|r| {
                let e = api::DockerEvent {
                    id: r.event_key.clone(),
                    engine_id: q.engine_id.clone(),
                    container_id: r.event.entity,
                    time_ns: r.event.time_ns.to_string(),
                    time_ms: r.event.time_ns / 1_000_000,
                    action: r.event.action,
                    name: r.event.attributes.get("name").cloned(),
                    exit_code: r.event.attributes.get("exitCode").cloned(),
                    attributes: r.event.attributes,
                };
                (e.id.clone(), e)
            })
            .collect();
        for e in ring {
            if after
                .as_ref()
                .is_none_or(|(t, key)| (e.time_ns.parse::<i64>().unwrap_or(0), &e.id) > (*t, key))
            {
                events.insert(e.id.clone(), e);
            }
        }
        let mut events: Vec<_> = events.into_values().collect();
        events.sort_by(|a, b| {
            a.time_ns
                .parse::<i64>()
                .unwrap_or(0)
                .cmp(&b.time_ns.parse::<i64>().unwrap_or(0))
                .then(a.id.cmp(&b.id))
        });
        events.truncate(if q.limit == 0 { 200 } else { q.limit.min(1000) } as usize);
        let next_id = events.last().map(|e| format!("{}:{}", e.time_ns, e.id));
        if !pin.valid() {
            return Err("Events query reset or expired".into());
        }
        let page = api::DockerEventsPage {
            events,
            next_id,
            coverage,
        };
        if serde_json::to_vec(&page).map_err(|e| e.to_string())?.len() > db::MAX_QUERY_BYTES {
            return Err("Events response byte limit exceeded".into());
        }
        Ok(page)
    }
}
fn wall_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
fn next(s: &mut State) -> i64 {
    s.revision = s.revision.saturating_add(1);
    s.revision
}
fn metadata(s: &mut State, b: &mut db::DockerBatch, engine: &str, c: &api::DockerContainer) {
    let key = (engine.into(), c.id.clone());
    if serde_json::to_vec(c).map_or(true, |bytes| bytes.len() > 16 * 1024) {
        gap(
            s,
            engine,
            c.last_seen_ms,
            c.last_seen_ms,
            "metadata_oversized",
        );
        return;
    }
    let mut stable = c.clone();
    stable.metrics = api::DockerMetrics {
        effective_limit_bytes: c.metrics.effective_limit_bytes,
        ..Default::default()
    };
    stable.observed_ms = None;
    stable.metadata_observed_ms = None;
    stable.interval_ms = None;
    stable.last_seen_ms = 0;
    let Ok(fingerprint) = serde_json::to_vec(&stable) else {
        return;
    };
    if s.fingerprints.get(&key) == Some(&fingerprint) {
        s.metadata.insert(key, c.clone());
        return;
    }
    let extra = fingerprint.len().saturating_mul(5).saturating_add(2048);
    if s.bytes + s.aux_bytes + s.reader_bytes + extra > RING_BYTES {
        gap(
            s,
            engine,
            c.last_seen_ms,
            c.last_seen_ms,
            "metadata_capacity",
        );
        return;
    }
    s.aux_bytes += extra;
    s.fingerprints.insert(key.clone(), fingerprint);
    s.metadata.insert(key, c.clone());
    let revision = next(s);
    let fact = db::DockerEntity {
        external_id: c.id.clone(),
        effective_ms: c.last_seen_ms,
        revision,
        name: c.name.clone(),
        image: c.image.clone(),
        project: c.project.clone(),
        service: c.service.clone(),
        lifecycle: format!("{:?}", c.lifecycle).to_lowercase(),
        explicit_limit: c.explicit_limit_bytes.map(|v| v as i64),
        effective_limit: c.metrics.effective_limit_bytes.map(|v| v as i64),
        capacity: None,
        last_seen_ms: c.last_seen_ms,
        tombstone: c.lifecycle == api::DockerLifecycle::Removed,
        extras: serde_json::to_vec(c).unwrap_or_default(),
    };
    s.facts.insert((engine.into(), c.id.clone()), fact.clone());
    s.metadata_history.push_back((engine.into(), fact.clone()));
    b.entities.push(fact);
}
fn metric_values(m: &api::DockerMetrics) -> [(&'static str, Option<f64>); 8] {
    [
        ("cpu_percent", m.cpu_pct),
        ("memory_working_set", m.working_set_bytes),
        ("memory_raw", m.raw_memory_bytes),
        ("memory_limit", m.effective_limit_bytes),
        ("network_rx", m.network_rx_bps),
        ("network_tx", m.network_tx_bps),
        ("block_read", m.block_read_bps),
        ("block_write", m.block_write_bps),
    ]
}
fn observe(
    s: &mut State,
    engine: &str,
    entity: &str,
    time: i64,
    span: i64,
    m: &api::DockerMetrics,
    population: Option<(i64, bool, bool)>,
) {
    let start = time.saturating_sub(span);
    let mut at = start;
    while at < time {
        let bucket = at.div_euclid(10_000) * 10_000;
        let end = (bucket + 10_000).min(time);
        let duration = (end - at) as u64;
        let key = (engine.into(), entity.into(), bucket);
        let revision = next(s);
        let mut b = s
            .entries
            .get(&key)
            .map(|e| (*e.bucket).clone())
            .unwrap_or(db::DockerBucket {
                entity: entity.into(),
                tier: Tier::S10,
                bucket_ms: bucket,
                revision,
                metrics: metric_values(m)
                    .iter()
                    .map(|(key, _)| db::WeightedMetric {
                        key: (*key).into(),
                        min: 0.,
                        max: 0.,
                        sum: 0.,
                        weight_ms: 0,
                        expected_ms: 10_000,
                    })
                    .collect(),
                populations: Vec::new(),
            });
        for (name, value) in metric_values(m) {
            let value = value.filter(|v| v.is_finite());
            let complete = population.is_none_or(|(_, cpu, memory)| {
                if name == "cpu_percent" {
                    cpu
                } else if name == "memory_working_set" {
                    memory
                } else {
                    true
                }
            });
            let Some(value) = value else {
                continue;
            };
            let weight = if complete { duration } else { 0 };
            let metric = db::WeightedMetric {
                key: name.into(),
                min: value,
                max: value,
                sum: value * weight as f64,
                weight_ms: weight,
                expected_ms: duration,
            };
            if let Some(old) = b.metrics.iter_mut().find(|v| v.key == name) {
                let _ = old.merge(&metric);
                old.expected_ms = 10_000;
                old.weight_ms = old.weight_ms.min(10_000);
            } else {
                b.metrics.push(db::WeightedMetric {
                    expected_ms: 10_000,
                    ..metric
                });
            }
        }
        if let Some((revision, _, _)) = population {
            if let Some(last) = b
                .populations
                .last_mut()
                .filter(|p| p.revision == revision && p.end_ms == at)
            {
                last.end_ms = end;
            } else {
                b.populations.push(db::PopulationSegment {
                    revision,
                    start_ms: at,
                    end_ms: end,
                });
            }
        }
        b.metrics.sort_by(|a, b| a.key.cmp(&b.key));
        b.revision = revision;
        let bytes = bucket_bytes(&b);
        let old_bytes = s.entries.get(&key).map_or(0, |e| e.bytes);
        if s.bytes
            .saturating_sub(old_bytes)
            .saturating_add(bytes)
            .saturating_add(s.aux_bytes)
            .saturating_add(s.reader_bytes)
            <= RING_BYTES
        {
            let pins = s.entries.get(&key).map_or(0, |e| e.pins);
            let durable = s.entries.get(&key).map_or(0, |e| e.durable);
            s.bytes = s.bytes.saturating_sub(old_bytes) + bytes;
            s.entries.insert(
                key,
                Entry {
                    bucket: Arc::new(b),
                    bytes,
                    pins,
                    durable,
                },
            );
        } else {
            gap(s, engine, at, end, "ring_capacity");
        }
        at = end;
    }
}
fn bucket_bytes(b: &db::DockerBucket) -> usize {
    256 + b.entity.len()
        + b.metrics.iter().map(|m| 80 + m.key.len()).sum::<usize>()
        + b.populations.len() * 32
}
fn queue(s: &mut State, a: Option<&Attached>, batch: db::DockerBatch, keys: Vec<Key>) {
    let Some(a) = a else {
        return;
    };
    if batch.buckets.is_empty()
        && batch.entities.is_empty()
        && batch.events.is_empty()
        && batch.coverage.is_empty()
    {
        return;
    }
    let engine = batch.daemon_id.clone();
    let start = batch.start_ms;
    let end = batch.end_ms;
    match a.writer.write_docker(batch) {
        Ok(rx) => {
            for key in &keys {
                if let Some(e) = s.entries.get_mut(key) {
                    e.pins += 1;
                }
            }
            s.pending.push(Receipt {
                rx,
                keys,
                engine,
                start,
                end,
            });
        }
        Err(e) => {
            s.error = Some(e.to_string());
            gap(s, &engine, start, end, "write_rejected");
        }
    }
}
fn drain(s: &mut State) {
    let mut keep = Vec::new();
    for p in std::mem::take(&mut s.pending) {
        match p.rx.try_recv() {
            Ok(ack) => {
                if matches!(
                    ack.outcome,
                    db::DockerOutcome::Failed | db::DockerOutcome::Rejected
                ) {
                    s.error = ack.reason;
                    gap(s, &p.engine, p.start, p.end, "write_failed");
                } else if ack.outcome == db::DockerOutcome::Persisted {
                    s.error = None;
                }
                for key in p.keys {
                    if let Some(e) = s.entries.get_mut(&key) {
                        e.pins = e.pins.saturating_sub(1);
                        if ack.outcome == db::DockerOutcome::Persisted {
                            e.durable = e.durable.max(ack.revision);
                        }
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => keep.push(p),
            Err(mpsc::TryRecvError::Disconnected) => {
                gap(s, &p.engine, p.start, p.end, "writer_disconnected");
                for key in p.keys {
                    if let Some(e) = s.entries.get_mut(&key) {
                        e.pins = e.pins.saturating_sub(1);
                    }
                }
            }
        }
    }
    s.pending = keep;
}
fn gap(s: &mut State, engine: &str, start: i64, end: i64, reason: &str) {
    let gaps = s.coverage.entry(engine.into()).or_default();
    if let Some(old) = gaps
        .iter_mut()
        .find(|g| g.reason == reason && g.container_id.is_none())
    {
        old.start_ms = old.start_ms.min(start);
        old.end_ms = Some(old.end_ms.unwrap_or(end).max(end));
    } else {
        gaps.push(api::DockerCoverage {
            start_ms: start,
            end_ms: Some(end),
            container_id: None,
            reason: reason.into(),
        });
    }
    bound_coverage(gaps);
}
fn bound_coverage(gaps: &mut Vec<api::DockerCoverage>) {
    if gaps.len() > 128 {
        let start = gaps.iter().map(|g| g.start_ms).min().unwrap_or(0);
        let end = gaps.iter().filter_map(|g| g.end_ms).max();
        gaps.clear();
        gaps.push(api::DockerCoverage {
            start_ms: start,
            end_ms: end,
            container_id: None,
            reason: "coverage_unknown".into(),
        });
    }
}
fn recount_aux(s: &mut State) {
    // Conservative owned-memory allowance: strings/serialized extras + map/allocator
    // overhead, including duplicate latest facts and fingerprints.
    s.aux_bytes = s
        .facts
        .values()
        .map(|e| e.extras.len() * 3 + e.name.len() + e.image.len() + 512)
        .sum::<usize>()
        + s.metadata_history
            .iter()
            .map(|(_, e)| e.extras.len() + 512)
            .sum::<usize>()
        + s.fingerprints
            .values()
            .map(|v| v.len() + 256)
            .sum::<usize>()
        + s.events
            .iter()
            .map(|e| {
                e.id.len()
                    + e.time_ns.len()
                    + e.action.len()
                    + e.name.as_ref().map_or(0, String::len)
                    + e.attributes
                        .iter()
                        .map(|(k, v)| k.len() + v.len() + 64)
                        .sum::<usize>()
                    + 512
            })
            .sum::<usize>()
        + s.population_facts
            .values()
            .map(|p| p.members.iter().map(|id| id.len() + 64).sum::<usize>() + 128)
            .sum::<usize>()
        + s.pending
            .iter()
            .map(|p| {
                p.keys
                    .iter()
                    .map(|(a, b, _)| a.len() + b.len() + 128)
                    .sum::<usize>()
                    + 256
            })
            .sum::<usize>()
        + s.observed.len() * 384
        + s.runtime.len() * 512
        + s.coverage.values().map(|g| g.len() * 512).sum::<usize>();
}
fn evict(s: &mut State, now: i64) {
    let keys: Vec<_> = s
        .entries
        .iter()
        .filter(|((_, _, t), e)| t.saturating_add(10_000) <= now - HOUR && e.pins == 0)
        .map(|(k, _)| k.clone())
        .collect();
    for key in keys {
        if let Some(e) = s.entries.remove(&key) {
            s.bytes = s.bytes.saturating_sub(e.bytes);
        }
    }
    s.events.retain(|e| e.time_ms >= now - HOUR);
    let active: BTreeSet<_> = s
        .entries
        .keys()
        .map(|(engine, id, _)| (engine.clone(), id.clone()))
        .collect();
    s.metadata
        .retain(|key, c| c.last_seen_ms >= now - HOUR || active.contains(key));
    s.facts
        .retain(|key, _| s.metadata.contains_key(key) || key.1.is_empty() && active.contains(key));
    s.fingerprints.retain(|key, _| s.metadata.contains_key(key));
    s.observed
        .retain(|key, t| *t >= now - HOUR || active.contains(key));
    // Keep one predecessor per entity for an hour's buckets.
    let mut newest_old = BTreeMap::new();
    for (engine, e) in &s.metadata_history {
        if e.effective_ms < now - HOUR {
            newest_old.insert(
                (engine.clone(), e.external_id.clone()),
                (e.effective_ms, e.revision),
            );
        }
    }
    s.metadata_history.retain(|(engine, e)| {
        e.effective_ms >= now - HOUR
            || newest_old.get(&(engine.clone(), e.external_id.clone()))
                == Some(&(e.effective_ms, e.revision))
                && active.contains(&(engine.clone(), e.external_id.clone()))
    });
    let needed: BTreeSet<_> = s
        .entries
        .iter()
        .flat_map(|((engine, _, _), e)| {
            e.bucket
                .populations
                .iter()
                .map(|p| (engine.clone(), p.revision))
        })
        .collect();
    s.population_facts.retain(|key, _| {
        needed.contains(key) || s.populations.get(&key.0).is_some_and(|(r, _)| *r == key.1)
    });
    let engines: BTreeSet<_> = s
        .entries
        .keys()
        .map(|(e, _, _)| e.clone())
        .chain(s.metadata.keys().map(|(e, _)| e.clone()))
        .collect();
    s.populations.retain(|e, _| engines.contains(e));
    s.runtime.retain(|e, _| engines.contains(e));
    s.coverage.retain(|e, _| engines.contains(e));
    recount_aux(s);
    while s.bytes + s.aux_bytes + s.reader_bytes > RING_BYTES {
        if let Some(key) = s
            .entries
            .iter()
            .filter(|(_, e)| e.pins == 0)
            .min_by_key(|((_, _, t), _)| *t)
            .map(|(key, _)| key.clone())
        {
            if let Some(e) = s.entries.remove(&key) {
                s.bytes = s.bytes.saturating_sub(e.bytes);
            }
            continue;
        }
        if s.metadata_history.pop_front().is_some() {
            recount_aux(s);
            continue;
        }
        if let Some(key) = s
            .metadata
            .iter()
            .min_by_key(|(_, c)| c.last_seen_ms)
            .map(|(k, _)| k.clone())
        {
            s.metadata.remove(&key);
            s.facts.remove(&key);
            s.observed.remove(&key);
            s.fingerprints.remove(&key);
            recount_aux(s);
            continue;
        }
        if s.events.pop_front().is_some() {
            recount_aux(s);
            continue;
        }
        break;
    }
}
fn fold_ring(ring: &[Arc<db::DockerBucket>], tier: Tier) -> Vec<db::DockerBucket> {
    let width = tier.bucket_ms().unwrap_or(10_000);
    let mut out: BTreeMap<i64, db::DockerBucket> = BTreeMap::new();
    for b in ring {
        let time = b.bucket_ms.div_euclid(width) * width;
        let dst = out.entry(time).or_insert_with(|| db::DockerBucket {
            entity: b.entity.clone(),
            tier,
            bucket_ms: time,
            revision: 0,
            metrics: Vec::new(),
            populations: Vec::new(),
        });
        dst.revision = dst.revision.max(b.revision.abs());
        for m in &b.metrics {
            if let Some(old) = dst.metrics.iter_mut().find(|v| v.key == m.key) {
                let _ = old.merge(m);
            } else {
                dst.metrics.push(m.clone());
            }
        }
        dst.populations.extend(b.populations.clone());
    }
    for b in out.values_mut() {
        for m in &mut b.metrics {
            m.expected_ms = width as u64;
        }
    }
    out.into_values().collect()
}
fn point(b: &db::DockerBucket) -> api::DockerHistoryPoint {
    let v = |key: &str| {
        b.metrics
            .iter()
            .find(|m| m.key == key)
            .and_then(db::WeightedMetric::average)
    };
    let measured = b
        .metrics
        .iter()
        .filter(|m| matches!(m.key.as_str(), "cpu_percent" | "memory_working_set"))
        .map(|m| m.weight_ms)
        .min()
        .unwrap_or(0);
    let expected = b.tier.bucket_ms().unwrap_or(10_000) as u32;
    api::DockerHistoryPoint {
        time_ms: b.bucket_ms,
        duration_ms: expected,
        metrics: api::DockerMetrics {
            cpu_pct: v("cpu_percent"),
            working_set_bytes: v("memory_working_set"),
            raw_memory_bytes: v("memory_raw"),
            effective_limit_bytes: v("memory_limit"),
            network_rx_bps: v("network_rx"),
            network_tx_bps: v("network_tx"),
            block_read_bps: v("block_read"),
            block_write_bps: v("block_write"),
        },
        measured_ms: measured.min(u32::MAX as u64) as u32,
        expected_ms: expected,
        covered: measured > 0 && measured >= u64::from(expected),
        population_revision: b.populations.last().map_or(0, |p| p.revision),
        revision: b.revision.abs(),
        explicit_limit_bytes: None,
        engine_capacity_bytes: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelvo_schema::{HostInfo, HostRecord, OsKind};
    use kelvo_store::{Store, StoreConfig};
    fn host() -> HostRecord {
        HostRecord {
            id: HostId(uuid::Uuid::from_u128(1)),
            is_local: true,
            display_name: "test".into(),
            info: HostInfo {
                os: OsKind::MacOs,
                os_version: "test".into(),
                model: None,
                chip: None,
                chip_known: false,
                cpu_topology: Vec::new(),
                mem_total_bytes: 1,
                boot_time_ms: 0,
                gpu_dvfs_mhz: Vec::new(),
                boot_mounts: Vec::new(),
            },
        }
    }
    fn setup() -> (tempfile::TempDir, Store, DockerRecorder) {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = StoreConfig::new(dir.path().join("h.sqlite"));
        cfg.commit_interval = Duration::from_secs(300);
        let store = Store::open(cfg).unwrap();
        store.writer().upsert_host(host()).unwrap();
        let recorder = DockerRecorder::new(host().id);
        recorder.attach(store.writer(), store.path().to_path_buf());
        (dir, store, recorder)
    }
    fn fixture(time: i64) -> api::DockerSnapshot {
        let handle = super::super::DockerHandle::new(
            host().id,
            api::DockerSettings::default(),
            PathBuf::new(),
        );
        let mut s = handle.snapshot();
        s.generation = 1;
        s.status = api::DockerStatus::Connected;
        s.observed_ms = time;
        s.interval_ms = 1000;
        s.engine = Some(api::DockerEngine {
            id: "daemon".into(),
            daemon_id: Some("daemon".into()),
            runtime: "OrbStack".into(),
            context: None,
            socket: "/tmp/docker.sock".into(),
            reachable: true,
            preferred: true,
            error: None,
            api_version: None,
            capacity_bytes: Some(1024.),
            capacity_observed_ms: Some(time),
        });
        s.containers = vec![api::DockerContainer {
            engine_id: "daemon".into(),
            metadata_available: true,
            metadata_observed_ms: Some(time),
            id: "c".into(),
            name: "before".into(),
            image: "image".into(),
            project: None,
            service: None,
            lifecycle: api::DockerLifecycle::Running,
            health: api::DockerHealth::NoCheck,
            restart_count: Some(0),
            observed_restarts: 0,
            repeated_restarts: false,
            observed_restart_window_ms: 300000,
            oom_killed: false,
            explicit_limit_bytes: Some(1024.),
            cpu_quota: None,
            restart_policy: None,
            ports: Vec::new(),
            mounts: Vec::new(),
            created: None,
            last_seen_ms: time,
            observed_ms: Some(time),
            interval_ms: Some(1000),
            monitored: true,
            metrics: api::DockerMetrics {
                cpu_pct: Some(100.),
                working_set_bytes: Some(128.),
                ..Default::default()
            },
        }];
        s.aggregate = api::DockerAggregate {
            cpu_cores: Some(1.),
            working_set_bytes: Some(128.),
            expected: 1,
            cpu_measured: 1,
            memory_measured: 1,
            running: 1,
            total: 1,
            monitored: 1,
            inventory_complete: true,
            ..Default::default()
        };
        s
    }
    #[test]
    fn detail_reconciles_state_metadata_and_metrics_by_evidence_clock() {
        let live = fixture(2000).containers.remove(0);
        let mut inspected = fixture(1000).containers.remove(0);
        inspected.explicit_limit_bytes = Some(4096.);
        inspected.ports = vec!["80/tcp".into()];
        inspected.lifecycle = api::DockerLifecycle::Exited;
        inspected.health = api::DockerHealth::Unhealthy;
        inspected.metrics.cpu_pct = Some(999.);
        assert_eq!(super::super::reconcile(live.clone(), &inspected), live);
        // Fresh inspection supplies state and metadata, while live metrics win.
        inspected.metadata_observed_ms = Some(3000);
        let merged = super::super::reconcile(live.clone(), &inspected);
        assert_eq!(merged.lifecycle, api::DockerLifecycle::Exited);
        assert_eq!(merged.health, api::DockerHealth::Unhealthy);
        assert_eq!(merged.explicit_limit_bytes, Some(4096.));
        assert_eq!(merged.ports, inspected.ports);
        assert_eq!(merged.metrics, live.metrics);
        // Metadata can be newer than the previous inspect but older than inventory.
        let mut inventory = live.clone();
        inventory.last_seen_ms = 4000;
        let merged = super::super::reconcile(inventory.clone(), &inspected);
        assert_eq!(merged.lifecycle, inventory.lifecycle);
        assert_eq!(merged.explicit_limit_bytes, Some(4096.));
        // Legacy rows without an inspect stamp accept newly stamped metadata.
        inventory.metadata_observed_ms = None;
        assert_eq!(
            super::super::reconcile(inventory.clone(), &inspected).explicit_limit_bytes,
            Some(4096.)
        );
        inspected.metadata_observed_ms = None;
        assert_eq!(
            super::super::reconcile(inventory.clone(), &inspected).explicit_limit_bytes,
            inventory.explicit_limit_bytes
        );
        // An uninspected inventory accepts available metadata even without a stamp.
        inventory.metadata_available = false;
        assert_eq!(
            super::super::reconcile(inventory, &inspected).explicit_limit_bytes,
            Some(4096.)
        );
        // A raced 404 is authoritative removal evidence, including cleared metrics.
        inspected.metadata_observed_ms = Some(3000);
        inspected.lifecycle = api::DockerLifecycle::Removed;
        inspected.observed_ms = None;
        inspected.interval_ms = None;
        inspected.metrics = api::DockerMetrics::default();
        assert_eq!(super::super::reconcile(live, &inspected), inspected);
    }
    #[test]
    fn history_points_publish_coverage_without_client_inference() {
        let mut bucket = db::DockerBucket {
            entity: "c".into(),
            tier: Tier::S10,
            bucket_ms: 0,
            revision: 1,
            metrics: vec![db::WeightedMetric {
                key: "cpu_percent".into(),
                min: 10.,
                max: 10.,
                sum: 100_000.,
                weight_ms: 10_000,
                expected_ms: 10_000,
            }],
            populations: vec![],
        };
        assert!(point(&bucket).covered);
        bucket.metrics[0].weight_ms = 9999;
        assert!(!point(&bucket).covered);
        bucket.metrics[0].weight_ms = 0;
        assert!(!point(&bucket).covered);
        bucket.metrics.clear();
        assert!(!point(&bucket).covered);
    }
    fn at(s: &mut api::DockerSnapshot, time: i64, interval: u32) {
        s.observed_ms = time;
        s.interval_ms = interval;
        for c in &mut s.containers {
            c.observed_ms = Some(time);
            c.last_seen_ms = time;
            c.interval_ms = Some(interval);
        }
    }
    fn query(start: i64, end: i64, max_points: u32) -> api::DockerHistoryRequest {
        api::DockerHistoryRequest {
            engine_id: "daemon".into(),
            container_id: Some("c".into()),
            from_ms: start,
            to_ms: end,
            max_points,
        }
    }
    #[test]
    fn failed_writes_release_pins_and_replay_metadata_with_samples() {
        let (_dir, store, r) = setup();
        let t = wall_ms().div_euclid(900_000) * 900_000;
        let mut s = fixture(t + 1000);
        r.record(&s);
        store.writer().flush().unwrap();
        store.writer().set_fail_commits(true);
        at(&mut s, t + 11_000, 10000);
        s.containers[0].name = "renamed".into();
        r.record(&s);
        assert!(store.writer().flush().is_err());
        r.status();
        {
            let state = r.state.lock_ok();
            assert!(state.entries.values().all(|e| e.pins == 0));
            assert!(!state.coverage["daemon"].is_empty());
        }
        store.writer().set_fail_commits(false);
        at(&mut s, t + 21_000, 10000);
        r.record(&s);
        store.writer().flush().unwrap();
        r.status();
        let mut reader = store.reader().unwrap();
        let snap = reader.docker_snapshot(host().id, "daemon").unwrap();
        let page = snap
            .history(&db::DockerHistoryQuery {
                entity: "c".into(),
                start_ms: t,
                end_ms: t + 60_000,
                tier: Tier::S10,
                max_points: 100,
            })
            .unwrap();
        assert!(page.metadata.iter().any(|e| e.name == "renamed"));
        assert!(!page.buckets.is_empty());
    }
    #[test]
    fn population_changes_keep_prior_segments_and_exact_revisions() {
        let (_dir, store, r) = setup();
        let t = wall_ms().div_euclid(900_000) * 900_000;
        let mut s = fixture(t + 1000);
        r.record(&s);
        at(&mut s, t + 6000, 5000);
        s.containers[0].id = "other".into();
        r.record(&s);
        at(&mut s, t + 11000, 5000);
        r.record(&s);
        store.writer().flush().unwrap();
        r.status();
        let mut reader = store.reader().unwrap();
        let snap = reader.docker_snapshot(host().id, "daemon").unwrap();
        let page = snap
            .history(&db::DockerHistoryQuery {
                entity: String::new(),
                start_ms: t,
                end_ms: t + 60000,
                tier: Tier::S10,
                max_points: 100,
            })
            .unwrap();
        let populations: &Vec<db::PopulationSegment> = &page.buckets[0].populations;
        assert_eq!(populations.len(), 2);
        assert_ne!(populations[0].revision, populations[1].revision);
        assert!(populations[0].revision > u32::MAX as i64);
    }
    #[test]
    fn nonaligned_hour_is_bounded_and_left_overlap_is_retained() {
        let r = DockerRecorder::new(host().id);
        let t = wall_ms().div_euclid(900_000) * 900_000;
        let mut s = fixture(t + 1000);
        for i in 0..=360 {
            at(&mut s, t + i * 10_000 + 1000, 10000);
            r.record(&s);
        }
        let page = r.history(&query(t + 1000, t + HOUR + 1000, 360)).unwrap();
        assert!(page.points.len() <= 360);
        assert!(page.points.first().unwrap().time_ms <= t + 1000);
        assert_eq!(
            page.points.get(1).unwrap().explicit_limit_bytes,
            Some(1024.)
        );
    }
    #[test]
    fn coarse_database_span_is_not_overwritten_by_partial_ring() {
        let (_dir, store, r) = setup();
        let t = wall_ms().div_euclid(900_000) * 900_000;
        let mut s = fixture(t + 1000);
        r.record(&s);
        for i in 1..=6 {
            at(&mut s, t + i * 10000, 10000);
            r.record(&s);
        }
        store.writer().flush().unwrap();
        r.status();
        {
            let mut state = r.state.lock_ok();
            let removed: Vec<_> = state
                .entries
                .keys()
                .filter(|(_, _, time)| *time < t + 50000)
                .cloned()
                .collect();
            for key in removed {
                let e = state.entries.remove(&key).unwrap();
                state.bytes -= e.bytes;
            }
        }
        at(&mut s, t + 61000, 1000);
        s.containers[0].metrics.cpu_pct = Some(300.);
        r.record(&s);
        let page = r.history(&query(t, t + 3_600_000, 60)).unwrap();
        let first = page.points.first().unwrap();
        assert!(first.measured_ms >= 59000, "{first:?}");
        assert!(first.metrics.cpu_pct.unwrap() < 110.);
    }
    #[test]
    fn same_nanosecond_ring_database_pagination_does_not_skip_or_repeat() {
        let (_dir, store, r) = setup();
        let t = wall_ms();
        let make = |id: &str| api::DockerEvent {
            id: id.into(),
            engine_id: "daemon".into(),
            container_id: Some(id.into()),
            time_ns: (t * 1_000_000 + 1).to_string(),
            time_ms: t,
            action: "oom".into(),
            attributes: BTreeMap::new(),
            name: None,
            exit_code: None,
        };
        for id in ["a", "b", "c"] {
            r.event(&make(id), 1);
        }
        store.writer().flush().unwrap();
        r.status();
        r.event(&make("d"), 1);
        let mut q = api::DockerEventsRequest {
            engine_id: "daemon".into(),
            container_id: None,
            from_ms: t,
            to_ms: t + 1,
            limit: 2,
            before_id: None,
        };
        let first = r.events(&q).unwrap();
        assert_eq!(first.events.len(), 2);
        q.before_id = first.next_id;
        let second = r.events(&q).unwrap();
        assert_eq!(second.events.len(), 2);
        let ids: BTreeSet<_> = first
            .events
            .iter()
            .chain(&second.events)
            .map(|e| &e.id)
            .collect();
        assert_eq!(ids.len(), 4);
    }
    #[test]
    fn reset_invalidates_reader_pins_and_discards_late_generation() {
        let (_dir, store, r) = setup();
        let t = wall_ms();
        r.record(&fixture(t));
        let pin = {
            let mut s = r.state.lock_ok();
            s.reader_bytes += 100;
            ReadPin {
                state: &r.state,
                bytes: 100,
                generation: s.generation,
                reset_epoch: s.reset_epoch,
                started: Instant::now(),
            }
        };
        r.reset(2).unwrap();
        assert!(!pin.valid());
        assert_eq!(r.state.lock_ok().reader_bytes, 100);
        drop(pin);
        assert_eq!(r.state.lock_ok().reader_bytes, 0);
        r.record(&fixture(t + 1000));
        assert!(r.state.lock_ok().entries.is_empty());
        store.writer().flush().unwrap();
    }
    #[test]
    fn churn_accounts_for_metadata_and_engine_maps_and_expires_them() {
        let r = DockerRecorder::new(host().id);
        let t = wall_ms();
        let mut s = fixture(t);
        for i in 0..200 {
            s.engine.as_mut().unwrap().daemon_id = Some(format!("engine{i}"));
            s.containers[0].engine_id = format!("engine{i}");
            r.record(&s);
        }
        let mut state = r.state.lock_ok();
        assert!(state.aux_bytes > state.facts.len() * 512);
        evict(&mut state, t + HOUR + 60000);
        assert!(state.entries.is_empty());
        assert!(state.facts.is_empty());
        assert!(state.runtime.is_empty());
        assert!(state.bytes + state.aux_bytes + state.reader_bytes <= RING_BYTES);
    }
    #[test]
    fn shutdown_commits_latest_open_bucket_after_pending_older_revision() {
        let (_dir, store, r) = setup();
        let t = wall_ms().div_euclid(10_000) * 10_000;
        let mut s = fixture(t + 1000);
        r.record(&s);
        // Metadata is still pending, and both samples belong to the open bucket.
        at(&mut s, t + 2000, 1000);
        r.record(&s);
        r.shutdown_flush().unwrap();
        let mut reader = store.reader().unwrap();
        let snap = reader.docker_snapshot(host().id, "daemon").unwrap();
        let page = snap
            .history(&db::DockerHistoryQuery {
                entity: "c".into(),
                start_ms: t,
                end_ms: t + 10000,
                tier: Tier::S10,
                max_points: 10,
            })
            .unwrap();
        assert_eq!(page.buckets.len(), 1);
        assert_eq!(
            page.buckets[0]
                .metrics
                .iter()
                .find(|m| m.key == "cpu_percent")
                .unwrap()
                .weight_ms,
            2000
        );
        let state = r.state.lock_ok();
        assert!(state.pending.is_empty());
        assert!(
            state
                .entries
                .values()
                .all(|e| e.pins == 0 && e.durable >= e.bucket.revision)
        );
    }
    #[test]
    fn review_clock_rollback_reseeds_unchanged_metadata_and_population() {
        let (_dir, store, r) = setup();
        let t = wall_ms().div_euclid(900000) * 900000;
        let mut s = fixture(t + 1000);
        r.record(&s);
        at(&mut s, t + 61000, 1000);
        s.containers[0].name = "future-name".into();
        s.containers[0].explicit_limit_bytes = Some(4096.);
        r.record(&s);
        store.writer().flush().unwrap();
        r.clock_step(t + 10000);
        at(&mut s, t + 11000, 1000);
        r.record(&s);
        at(&mut s, t + 21000, 10000);
        r.record(&s);
        r.shutdown_flush().unwrap();
        let mut reader = store.reader().unwrap();
        let snap = reader.docker_snapshot(host().id, "daemon").unwrap();
        let page = snap
            .history(&db::DockerHistoryQuery {
                entity: "c".into(),
                start_ms: t,
                end_ms: t + 120000,
                tier: Tier::S10,
                max_points: 100,
            })
            .unwrap();
        assert!(
            page.metadata
                .iter()
                .any(|m| m.name == "before" && m.effective_ms == t + 1000)
        );
        assert!(page.metadata.iter().any(|m| m.name == "future-name"
            && m.effective_ms == t + 11000
            && m.explicit_limit == Some(4096)));
        assert!(page.metadata.iter().all(|m| m.effective_ms < t + 61000));
        drop(snap);
        let fresh = DockerRecorder::new(host().id);
        fresh.attach(store.writer(), store.path().to_path_buf());
        let page = fresh.history(&query(t + 20000, t + 30000, 100)).unwrap();
        assert_eq!(page.points[0].explicit_limit_bytes, Some(4096.));
    }
    #[test]
    fn review_short_history_falls_back_after_age_or_partial_fine_reclamation() {
        for age in [true, false] {
            let (_dir, store, r) = setup();
            let now = wall_ms().div_euclid(900000) * 900000;
            let t = if age { now - 2 * 86400000 } else { now };
            let mut s = fixture(t + 10000);
            for i in 1..=6 {
                at(&mut s, t + i * 10000, 10000);
                r.record(&s);
            }
            r.shutdown_flush().unwrap();
            if age {
                store
                    .writer()
                    .prune(now, kelvo_store::Retention::default())
                    .unwrap();
            } else {
                let conn = rusqlite::Connection::open(store.path()).unwrap();
                conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
                conn.execute("DELETE FROM docker_10s WHERE bucket_ms=?1", [t + 20000])
                    .unwrap();
            }
            let fresh = DockerRecorder::new(host().id);
            fresh.attach(store.writer(), store.path().to_path_buf());
            let page = fresh.history(&query(t, t + 3600000, 360)).unwrap();
            assert_eq!(page.points.len(), 1, "age={age}");
            assert_eq!(page.points[0].duration_ms, 60000);
            assert_eq!(page.points[0].measured_ms, 60000);
            assert_eq!(page.points[0].metrics.cpu_pct, Some(100.));
        }
    }
    #[test]
    fn review_event_coverage_survives_recorder_restart() {
        let (_dir, store, r) = setup();
        let t = wall_ms();
        let mut s = fixture(t);
        s.coverage = vec![api::DockerCoverage {
            start_ms: t - 1000,
            end_ms: Some(t),
            container_id: None,
            reason: "replay_uncertain".into(),
        }];
        r.record(&s);
        store.writer().flush().unwrap();
        let fresh = DockerRecorder::new(host().id);
        fresh.attach(store.writer(), store.path().to_path_buf());
        let page = fresh
            .events(&api::DockerEventsRequest {
                engine_id: "daemon".into(),
                container_id: Some("c".into()),
                from_ms: t - 2000,
                to_ms: t + 1000,
                limit: 10,
                before_id: None,
            })
            .unwrap();
        assert_eq!(page.coverage, s.coverage);
        // A newer ring close supersedes the older durable gap identity.
        s.coverage[0].end_ms = Some(t + 500);
        at(&mut s, t + 1000, 1000);
        fresh.record(&s);
        let merged = fresh
            .events(&api::DockerEventsRequest {
                engine_id: "daemon".into(),
                container_id: Some("c".into()),
                from_ms: t - 2000,
                to_ms: t + 2000,
                limit: 10,
                before_id: None,
            })
            .unwrap();
        assert_eq!(merged.coverage, s.coverage);
    }
    #[test]
    fn review_failed_reset_resumes_detached_ring_history_and_events() {
        let (_dir, _store, r) = setup();
        let t = wall_ms();
        let result: Result<(), String> = r.with_suspended(|| {
            r.detach();
            r.reset(2)?;
            Err("injected store reopen failure".into())
        });
        assert!(result.is_err());
        let mut s = fixture(t);
        s.generation = 2;
        r.record(&s);
        r.event(
            &api::DockerEvent {
                id: "event".into(),
                engine_id: "daemon".into(),
                container_id: Some("c".into()),
                time_ns: (t * 1000000).to_string(),
                time_ms: t,
                action: "start".into(),
                name: None,
                exit_code: None,
                attributes: BTreeMap::new(),
            },
            2,
        );
        assert!(
            !r.history(&query(t - 10000, t + 10000, 100))
                .unwrap()
                .points
                .is_empty()
        );
        assert_eq!(
            r.events(&api::DockerEventsRequest {
                engine_id: "daemon".into(),
                container_id: None,
                from_ms: t - 1000,
                to_ms: t + 1000,
                limit: 10,
                before_id: None
            })
            .unwrap()
            .events
            .len(),
            1
        );
        assert_eq!(r.status().status, "live_only");
    }
    #[test]
    fn retained_catalog_merges_uncommitted_metadata_and_survives_restart_then_reset() {
        let (_dir, store, recorder) = setup();
        let mut snapshot = fixture(wall_ms());
        recorder.record(&snapshot);
        store.writer().flush().unwrap();
        snapshot.containers[0].name = "after".into();
        snapshot.containers[0].last_seen_ms += 1000;
        let mut second = snapshot.containers[0].clone();
        second.id = "second".into();
        second.name = "second".into();
        snapshot.containers.push(second);
        recorder.record(&snapshot);
        let request = api::DockerRetainedRequest {
            engine_id: Some("daemon".into()),
            search: String::new(),
            offset: 0,
            limit: 1,
        };
        let first = recorder.retained(&request).unwrap();
        assert_eq!(first.total, 2);
        assert_eq!(first.containers[0].name, "after");
        assert_eq!(first.next_offset, Some(1));
        assert!(
            first.containers[0].metrics.cpu_pct.is_none(),
            "retained metadata must not masquerade as current metrics"
        );
        assert_eq!(first.engines[0].runtime, "OrbStack");
        let page2 = recorder
            .retained(&api::DockerRetainedRequest {
                offset: 1,
                ..request.clone()
            })
            .unwrap();
        assert_eq!(page2.containers[0].id, "second");
        assert_eq!(page2.next_offset, None);
        assert_eq!(
            recorder
                .retained(&api::DockerRetainedRequest {
                    search: "before".into(),
                    ..request.clone()
                })
                .unwrap()
                .total,
            0,
            "renamed ring metadata suppresses stale durable search results"
        );
        recorder.shutdown_flush().unwrap();
        let restarted = DockerRecorder::new(host().id);
        restarted.attach(store.writer(), store.path().to_path_buf());
        let durable = restarted.retained(&request).unwrap();
        assert_eq!(durable.total, 2);
        assert_eq!(durable.containers[0].name, "after");
        restarted.reset(2).unwrap();
        let reset = restarted.retained(&request).unwrap();
        assert!(reset.containers.is_empty());
        assert!(reset.engines.is_empty());
    }
    #[test]
    fn live_only_retained_catalog_caps_all_engine_descriptors_even_when_filtered() {
        let recorder = DockerRecorder::new(host().id);
        for index in 0..256 {
            let mut snapshot = fixture(wall_ms());
            let id = format!("daemon-{index:03}");
            let engine = snapshot.engine.as_mut().unwrap();
            engine.id = id.clone();
            engine.daemon_id = Some(id.clone());
            snapshot.containers[0].engine_id = id;
            recorder.record(&snapshot);
        }
        let request = api::DockerRetainedRequest {
            engine_id: Some("daemon-000".into()),
            search: String::new(),
            offset: 0,
            limit: 1,
        };
        assert_eq!(recorder.retained(&request).unwrap().engines.len(), 256);
        let mut extra = fixture(wall_ms());
        extra.engine.as_mut().unwrap().id = "daemon-extra".into();
        extra.engine.as_mut().unwrap().daemon_id = Some("daemon-extra".into());
        extra.containers[0].engine_id = "daemon-extra".into();
        recorder.record(&extra);
        assert!(
            recorder
                .retained(&request)
                .unwrap_err()
                .contains("engine bound")
        );
        assert_eq!(recorder.state.lock_ok().reader_bytes, 0);
    }
    #[test]
    fn metadata_receipt_refresh_does_not_write_duplicate_semantic_facts() {
        let recorder = DockerRecorder::new(host().id);
        let mut snapshot = fixture(wall_ms());
        recorder.record(&snapshot);
        let count = recorder.state.lock_ok().metadata_history.len();
        snapshot.containers[0].metadata_observed_ms = Some(wall_ms() + 1000);
        recorder.record(&snapshot);
        assert_eq!(recorder.state.lock_ok().metadata_history.len(), count);
        assert_eq!(
            recorder.detail("daemon", "c").unwrap().metadata_observed_ms,
            snapshot.containers[0].metadata_observed_ms
        );
    }
}

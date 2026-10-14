//! Admit observations and lifecycle evidence under the recorder coordination lock.
use super::*;

impl DockerRecorder {
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
        self.restore_coverage(&mut s, attached.as_ref(), daemon, now);
        let mut batch = self.batch(&mut s, daemon, snapshot.generation, now);
        record_capacity(&mut s, &mut batch, engine, daemon, now);
        let population = record_population(&mut s, snapshot, daemon, now);
        record_containers(&mut s, &mut batch, snapshot, daemon);
        record_aggregate(&mut s, snapshot, daemon, population, now);
        record_coverage(&mut s, &mut batch, snapshot, daemon, now);
        let keys = pending_buckets(&s, &mut batch, daemon, now);
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
        if !s.retry.contains_key(&event.engine_id) {
            batch.cursor = Some(db::DockerCursor {
                time_ns,
                event_key: key,
            });
        }
        if let Some(id) = &event.container_id
            && let Some(fact) = s.facts.get(&(event.engine_id.clone(), id.clone()))
        {
            batch.entities.push(fact.clone());
        }
        batch.events.push(e);
        queue(&mut s, a.as_ref(), batch, Vec::new());
    }
}

impl DockerRecorder {
    fn restore_coverage(&self, s: &mut State, attached: Option<&Attached>, daemon: &str, now: i64) {
        // A new process has no published coverage from the previous process.
        // Restore its open lifecycle intervals before the first observation so
        // the ring can close them immediately, ahead of SQLite's commit cadence.
        if !s.restored_coverage.contains(daemon)
            && let Some(attached) = attached
        {
            let restored = (|| -> Result<_, kelvo_store::StoreError> {
                let mut reader = Reader::open(&attached.path)?;
                let snapshot = reader.docker_snapshot(self.host, daemon)?;
                snapshot.event_coverage(None, now.saturating_sub(db::MAX_RANGE_MS - 1), now + 1)
            })();
            match restored {
                Ok(gaps) => {
                    let coverage = s.coverage.entry(daemon.to_owned()).or_default();
                    for gap in gaps.into_iter().filter(|g| {
                        db::resumes_on_observation(&g.reason)
                            && g.entity.is_none()
                            && g.metric.is_none()
                            && g.end_ms.is_none()
                    }) {
                        if !coverage
                            .iter()
                            .any(|g| g.start_ms == gap.start_ms && g.reason == gap.reason)
                        {
                            coverage.push(api::DockerCoverage {
                                start_ms: gap.start_ms,
                                end_ms: None,
                                container_id: None,
                                reason: gap.reason,
                            });
                        }
                    }
                    s.restored_coverage.insert(daemon.to_owned());
                }
                Err(error) => s.error = Some(error.to_string()),
            }
        }
    }
}

fn record_capacity(
    s: &mut State,
    batch: &mut db::DockerBatch,
    engine: &api::DockerEngine,
    daemon: &str,
    now: i64,
) {
    let capacity = engine
        .capacity_bytes
        .filter(|v| v.is_finite() && *v >= 0.)
        .map(|v| v as i64);
    if s.facts
        .get(&(daemon.to_owned(), String::new()))
        .is_none_or(|e| e.capacity != capacity)
    {
        let fact = db::DockerEntity {
            external_id: String::new(),
            effective_ms: engine.capacity_observed_ms.unwrap_or(now).min(now),
            revision: next(s),
            name: engine.runtime.clone(),
            capacity,
            last_seen_ms: now,
            ..Default::default()
        };
        s.facts
            .insert((daemon.to_owned(), String::new()), fact.clone());
        s.metadata_history
            .push_back((daemon.to_owned(), fact.clone()));
        batch.entities.push(fact);
    }
}

fn record_population(s: &mut State, snapshot: &api::DockerSnapshot, daemon: &str, now: i64) -> i64 {
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
        let revision = next(s);
        s.populations.insert(daemon.to_owned(), (revision, members));
    }
    let (population, members) = s.populations.get(daemon).cloned().unwrap_or_default();
    let fact = db::DockerPopulation {
        revision: population,
        effective_ms: now,
        members,
    };
    s.population_facts
        .entry((daemon.to_owned(), population))
        .or_insert(fact);
    population
}

fn record_containers(
    s: &mut State,
    batch: &mut db::DockerBatch,
    snapshot: &api::DockerSnapshot,
    daemon: &str,
) {
    for c in &snapshot.containers {
        metadata(s, batch, daemon, c);
        if !c.monitored {
            continue;
        }
        let Some(time) = c.observed_ms else {
            continue;
        };
        let span = c.interval_ms.unwrap_or(0).min(60_000) as i64;
        if span == 0
            || s.observed
                .get(&(daemon.to_owned(), c.id.clone()))
                .is_some_and(|v| *v >= time)
        {
            continue;
        }
        s.observed.insert((daemon.to_owned(), c.id.clone()), time);
        observe(s, daemon, &c.id, time, span, &c.metrics, None);
    }
}

fn record_aggregate(
    s: &mut State,
    snapshot: &api::DockerSnapshot,
    daemon: &str,
    population: i64,
    now: i64,
) {
    if !s
        .observed
        .get(&(daemon.to_owned(), String::new()))
        .is_some_and(|v| *v >= now)
    {
        s.observed.insert((daemon.to_owned(), String::new()), now);
        let metrics = api::DockerMetrics {
            cpu_pct: snapshot.aggregate.cpu_cores.map(|v| v * 100.),
            working_set_bytes: snapshot.aggregate.working_set_bytes,
            ..Default::default()
        };
        observe(
            s,
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
}

fn record_coverage(
    s: &mut State,
    batch: &mut db::DockerBatch,
    snapshot: &api::DockerSnapshot,
    daemon: &str,
    now: i64,
) {
    // configure() clears the published generation's coverage. The recorder
    // still owns the old engine interruption until that daemon resumes.
    let mut resumed = Vec::new();
    if snapshot.status == api::DockerStatus::Connected
        && let Some(gaps) = s.coverage.get_mut(daemon)
    {
        for gap in gaps.iter_mut().filter(|g| {
            db::resumes_on_observation(&g.reason) && g.end_ms.is_none() && g.start_ms < now
        }) {
            gap.end_ms = Some(now);
            resumed.push(gap.clone());
        }
    }
    for gap in resumed {
        batch.coverage.push(db::DockerCoverage {
            entity: gap.container_id,
            metric: None,
            start_ms: gap.start_ms,
            end_ms: gap.end_ms,
            reason: gap.reason,
            revision: next(s),
            sampled_ms: 0,
            expected_ms: now - gap.start_ms,
        });
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
                revision: next(s),
                sampled_ms: 0,
                // A transition can open a gap after the last metric snapshot.
                // Its still-open duration is zero until a newer observation.
                expected_ms: c.end_ms.unwrap_or(now).saturating_sub(c.start_ms).max(0),
            });
        }
    }
    let gaps = s.coverage.entry(daemon.to_owned()).or_default();
    for c in &snapshot.coverage {
        if let Some(old) = gaps.iter_mut().find(|old| {
            old.start_ms == c.start_ms
                && old.container_id == c.container_id
                && old.reason == c.reason
        }) {
            if old.end_ms.is_none() || c.end_ms.is_some() {
                *old = c.clone();
            }
        } else {
            gaps.push(c.clone());
        }
    }
    bound_coverage(gaps);
}

fn pending_buckets(s: &State, batch: &mut db::DockerBatch, daemon: &str, now: i64) -> Vec<Key> {
    let cut = now.div_euclid(10_000) * 10_000;
    let keys: Vec<_> = s
        .entries
        .iter()
        .filter(|((e, _, t), v)| {
            e == daemon && *t < cut && v.pins == 0 && v.bucket.revision > v.durable
        })
        .take(1024)
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
        .filter_map(|revision| {
            s.population_facts
                .get(&(daemon.to_owned(), revision))
                .cloned()
        })
        .collect();
    let needed: BTreeSet<_> = batch.buckets.iter().map(|b| b.entity.clone()).collect();
    for id in needed {
        if !batch.entities.iter().any(|e| e.external_id == id)
            && let Some(fact) = s.facts.get(&(daemon.to_owned(), id))
        {
            batch.entities.push(fact.clone());
        }
    }
    keys
}

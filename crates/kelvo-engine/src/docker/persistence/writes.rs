//! Bound outgoing writes, release acknowledgement pins and retain retry evidence.
use super::*;

impl DockerRecorder {
    /// Settle older revisions, then flush open buckets without waiting under the snapshot lock.
    pub fn shutdown_flush(&self) -> Result<(), DockerServiceError> {
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
            .flush_docker(deadline.saturating_duration_since(Instant::now()))?;
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
                    .or_else(|| s.retry.keys().next().cloned())
                else {
                    return Ok(());
                };
                if Instant::now() >= deadline {
                    return Err(DockerServiceError::Unavailable(
                        "Docker shutdown flush timed out",
                    ));
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
                if keys.is_empty() && !s.retry.contains_key(&engine) {
                    return Err(DockerServiceError::Unavailable(
                        "Docker shutdown still has unacknowledged writes",
                    ));
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
                batch.entities.extend(
                    entities
                        .into_iter()
                        .filter_map(|id| s.facts.get(&(engine.clone(), id)).cloned()),
                );
                batch.start_ms = batch
                    .buckets
                    .iter()
                    .map(|b| b.bucket_ms)
                    .min()
                    .unwrap_or(batch.end_ms);
                queue(&mut s, Some(&a), batch, keys);
            }
            a.writer
                .flush_docker(deadline.saturating_duration_since(Instant::now()))?;
            let _guard = coord.lock_ok();
            let mut s = self.state.lock_ok();
            drain(&mut s);
            if let Some(error) = &s.error {
                return Err(DockerServiceError::WriteFailed(error.clone()));
            }
        }
    }
    pub(super) fn batch(
        &self,
        s: &mut State,
        daemon: &str,
        generation: u32,
        time: i64,
    ) -> db::DockerBatch {
        let mut retry = s.retry.remove(daemon).unwrap_or_default();
        // Leave one event slot and 128 coverage slots for this collection cycle.
        // A bounded retry queue can still be larger than one store batch.
        let remaining_events = if retry.events.len() > 999 {
            retry.events.split_off(999)
        } else {
            Vec::new()
        };
        let remaining_coverage = if retry.coverage.len() > 896 {
            retry.coverage.split_off(896)
        } else {
            Vec::new()
        };
        if !remaining_events.is_empty() || !remaining_coverage.is_empty() {
            s.retry.insert(
                daemon.into(),
                Retry {
                    events: remaining_events,
                    coverage: remaining_coverage,
                    entities: Vec::new(),
                    cursor: retry.cursor.take(),
                },
            );
        }
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
            entities: retry.entities,
            buckets: Vec::new(),
            events: retry.events,
            coverage: retry.coverage,
            cursor: retry.cursor,
            populations: Vec::new(),
        }
    }
}
/// Submit a bounded prefix; excluded buckets stay dirty and retry evidence stays owned.
fn bound_batch(s: &mut State, batch: &mut db::DockerBatch, keys: &mut Vec<Key>) {
    let mut deferred = Retry::default();
    let mut split = false;
    while batch.logical_bytes().is_err() {
        let mut reduced = false;
        if batch.entities.len() > 1 {
            deferred
                .entities
                .extend(batch.entities.split_off(batch.entities.len().div_ceil(2)));
            reduced = true;
        }
        if batch.events.len() > 1 {
            deferred
                .events
                .extend(batch.events.split_off(batch.events.len().div_ceil(2)));
            reduced = true;
        }
        if batch.coverage.len() > 1 {
            deferred
                .coverage
                .extend(batch.coverage.split_off(batch.coverage.len().div_ceil(2)));
            reduced = true;
        }
        if batch.buckets.len() > 1 {
            batch.buckets.truncate(batch.buckets.len().div_ceil(2));
            let needed: BTreeSet<_> = batch
                .buckets
                .iter()
                .flat_map(|bucket| bucket.populations.iter().map(|p| p.revision))
                .collect();
            batch.populations.retain(|p| needed.contains(&p.revision));
            reduced = true;
        }
        if !reduced {
            // A malformed individual row still reaches normal admission diagnostics;
            // never spin here or discard it as if it had been written successfully.
            break;
        }
        split = true;
    }
    if split {
        let kept: BTreeSet<_> = batch
            .buckets
            .iter()
            .map(|b| (b.entity.as_str(), b.bucket_ms))
            .collect();
        keys.retain(|(_, id, time)| kept.contains(&(id.as_str(), *time)));
        // A replay cursor cannot pass deferred events or uncertainty coverage.
        deferred.cursor = batch.cursor.take();
        requeue(s, &batch.daemon_id, deferred);
    }
}
pub(super) fn queue(
    s: &mut State,
    a: Option<&Attached>,
    mut batch: db::DockerBatch,
    mut keys: Vec<Key>,
) {
    let Some(a) = a else {
        return;
    };
    bound_batch(s, &mut batch, &mut keys);
    if batch.buckets.is_empty()
        && batch.entities.is_empty()
        && batch.events.is_empty()
        && batch.coverage.is_empty()
        && batch.cursor.is_none()
    {
        return;
    }
    let engine = batch.daemon_id.clone();
    let start = batch.start_ms;
    let end = batch.end_ms;
    let retry = Retry {
        events: batch.events.clone(),
        coverage: batch.coverage.clone(),
        entities: batch.entities.clone(),
        cursor: batch.cursor.clone(),
    };
    match a.writer.write_docker(batch) {
        Ok(rx) => {
            for key in &keys {
                if let Some(e) = s.entries.get_mut(key) {
                    e.pins += 1;
                }
            }
            s.pending.push(Receipt {
                rx,
                retry,
                keys,
                engine,
                start,
                end,
            });
        }
        Err(e) => {
            s.error = Some(e.to_string());
            requeue(s, &engine, retry);
        }
    }
}
pub(super) fn drain(s: &mut State) {
    drain_at(s, wall_ms());
}
pub(super) fn drain_at(s: &mut State, now: i64) {
    let mut keep = Vec::new();
    for p in std::mem::take(&mut s.pending) {
        match p.rx.try_recv() {
            Ok(ack) => {
                if matches!(
                    ack.outcome,
                    db::DockerOutcome::Failed | db::DockerOutcome::Rejected
                ) {
                    s.error = Some(
                        ack.reason
                            .unwrap_or_else(|| "Docker history write failed".into()),
                    );
                    if should_warn(s, &p.engine, now) {
                        tracing::warn!(engine = %p.engine, reason = ?s.error, "Docker history write will retry");
                    }
                    requeue(s, &p.engine, p.retry);
                } else if ack.outcome == db::DockerOutcome::Persisted && s.retry.is_empty() {
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
                requeue(s, &p.engine, p.retry);
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
pub(super) fn should_warn(s: &mut State, engine: &str, now: i64) -> bool {
    if s.last_warning
        .get(engine)
        .is_some_and(|last| now.saturating_sub(*last) < 60_000)
    {
        return false;
    }
    s.last_warning.insert(engine.into(), now);
    true
}
fn merge_retry_coverage(rows: &mut Vec<db::DockerCoverage>, mut row: db::DockerCoverage) {
    let mut index = 0;
    while index < rows.len() {
        let old = &rows[index];
        if old.entity == row.entity
            && old.metric == row.metric
            && old.reason == row.reason
            && old.end_ms.is_some_and(|end| end >= row.start_ms)
            && row.end_ms.is_some_and(|end| end >= old.start_ms)
        {
            let old = rows.remove(index);
            row.start_ms = row.start_ms.min(old.start_ms);
            row.end_ms = row.end_ms.max(old.end_ms);
            row.revision = row.revision.max(old.revision);
            row.expected_ms = row
                .end_ms
                .unwrap_or(row.start_ms)
                .saturating_sub(row.start_ms);
        } else {
            index += 1;
        }
    }
    rows.push(row);
}
pub(super) fn requeue(s: &mut State, engine: &str, mut content: Retry) {
    let revision = next(s);
    let retry = s.retry.entry(engine.into()).or_default();
    retry.events.append(&mut content.events);
    retry.events.sort_by_key(|e| e.time_ns);
    retry.events.dedup_by(|a, b| a == b);
    for coverage in content.coverage {
        merge_retry_coverage(&mut retry.coverage, coverage);
    }
    for entity in content.entities {
        if !retry
            .entities
            .iter()
            .any(|e| e.external_id == entity.external_id && e.revision == entity.revision)
        {
            retry.entities.push(entity);
        }
    }
    if content.cursor.as_ref().is_some_and(|c| {
        retry
            .cursor
            .as_ref()
            .is_none_or(|old| c.time_ns > old.time_ns)
    }) {
        retry.cursor = content.cursor;
    }
    let mut excess = s
        .retry
        .values()
        .map(|retry| retry.events.len())
        .sum::<usize>()
        .saturating_sub(4096);
    let mut lost: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    while excess > 0 {
        let Some(engine) = s
            .retry
            .iter()
            .filter_map(|(engine, retry)| retry.events.first().map(|event| (engine, event.time_ns)))
            .min_by_key(|(_, time)| *time)
            .map(|(engine, _)| engine.clone())
        else {
            break;
        };
        if let Some(retry) = s.retry.get_mut(&engine) {
            let event = retry.events.remove(0);
            let start = event.time_ns / 1_000_000;
            lost.entry(engine)
                .and_modify(|span| {
                    span.0 = span.0.min(start);
                    span.1 = span.1.max(start + 1);
                })
                .or_insert((start, start + 1));
        }
        excess -= 1;
    }
    for (engine, (start, end)) in lost {
        if let Some(retry) = s.retry.get_mut(&engine) {
            merge_retry_coverage(
                &mut retry.coverage,
                db::DockerCoverage {
                    entity: None,
                    metric: None,
                    start_ms: start,
                    end_ms: Some(end),
                    reason: "event_replay_uncertain".into(),
                    revision,
                    sampled_ms: 0,
                    expected_ms: end - start,
                },
            );
        }
    }
}

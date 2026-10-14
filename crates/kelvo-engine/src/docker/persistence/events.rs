//! Merge durable and live events with stable pagination and coverage.
use super::*;

impl DockerRecorder {
    /// Durable replay cursor; Docker's bounded replay still requires uncertainty coverage.
    /// Invoke from a blocking worker, never a sampler.
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
    pub fn events(
        &self,
        q: &api::DockerEventsRequest,
    ) -> Result<api::DockerEventsPage, DockerServiceError> {
        if q.to_ms <= q.from_ms || q.to_ms.saturating_sub(q.from_ms) > db::MAX_RANGE_MS {
            return Err(DockerServiceError::InvalidRequest("Invalid events range"));
        }
        let (a, coord) = self.context();
        let mut reader = a.as_ref().map(|a| Reader::open(&a.path)).transpose()?;
        let _g = coord.lock_ok();
        let snapshot = reader
            .as_mut()
            .map(|r| r.docker_snapshot(self.host, &q.engine_id))
            .transpose()?;
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
            return Err(DockerServiceError::ResourceLimit(
                "Events snapshot byte limit reached",
            ));
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
        let coverage = event_coverage(q, snapshot.as_ref(), coverage)?;
        let (events, next_id) = event_rows(q, snapshot.as_ref(), ring, after)?;
        if !pin.valid() {
            return Err(DockerServiceError::SnapshotExpired(
                "Events query reset or expired",
            ));
        }
        let page = api::DockerEventsPage {
            events,
            next_id,
            coverage,
        };
        if serde_json::to_vec(&page)?.len() > db::MAX_QUERY_BYTES {
            return Err(DockerServiceError::ResourceLimit(
                "Events response byte limit exceeded",
            ));
        }
        Ok(page)
    }
}

fn event_coverage(
    q: &api::DockerEventsRequest,
    snapshot: Option<&db::DockerReadSnapshot<'_>>,
    coverage: Vec<api::DockerCoverage>,
) -> Result<Vec<api::DockerCoverage>, DockerServiceError> {
    let mut coverage = coverage;
    if let Some(snapshot) = &snapshot {
        for c in snapshot.event_coverage(q.container_id.as_deref(), q.from_ms, q.to_ms)? {
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
        return Err(DockerServiceError::ResourceLimit(
            "Events coverage count exceeded",
        ));
    }
    Ok(coverage)
}

fn event_rows(
    q: &api::DockerEventsRequest,
    snapshot: Option<&db::DockerReadSnapshot<'_>>,
    ring: Vec<api::DockerEvent>,
    after: Option<(i64, String)>,
) -> Result<(Vec<api::DockerEvent>, Option<String>), DockerServiceError> {
    let rows = if let Some(snapshot) = &snapshot {
        snapshot.events(&db::DockerEventQuery {
            entity: q.container_id.clone(),
            start_ms: q.from_ms,
            end_ms: q.to_ms,
            after: after.clone(),
            limit: q.limit.min(1000) as usize,
        })?
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
    Ok((events, next_id))
}

//! Account for ring, reader and retry ownership; evict only unpinned evidence.
use super::*;

pub(super) fn bucket_bytes(b: &db::DockerBucket) -> usize {
    256 + b.entity.len()
        + b.metrics.iter().map(|m| 80 + m.key.len()).sum::<usize>()
        + b.populations.len() * 32
}
pub(super) fn evict_entry(s: &mut State, key: &Key) {
    if let Some(e) = s.entries.remove(key) {
        s.bytes = s.bytes.saturating_sub(e.bytes);
        if e.bucket.revision > e.durable {
            let revision = next(s);
            let coverage = db::DockerCoverage {
                // Empty ID is the engine aggregate, not engine-wide coverage.
                entity: Some(key.1.clone()),
                metric: None,
                start_ms: key.2,
                end_ms: Some(key.2 + 10_000),
                reason: "write_failed".into(),
                revision,
                sampled_ms: 0,
                expected_ms: 10_000,
            };
            let mut retry = Retry::default();
            if let Some(fact) = s.facts.get(&(key.0.clone(), key.1.clone())) {
                retry.entities.push(fact.clone());
            }
            retry.coverage.push(coverage);
            requeue(s, &key.0, retry);
        }
    }
}
pub(super) fn gap(s: &mut State, engine: &str, start: i64, end: i64, reason: &str) {
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
pub(super) fn bound_coverage(gaps: &mut Vec<api::DockerCoverage>) {
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
fn retry_bytes(retry: &Retry) -> usize {
    retry
        .events
        .iter()
        .map(|e| {
            e.action.len()
                + e.entity.as_ref().map_or(0, String::len)
                + e.attributes
                    .iter()
                    .map(|(k, v)| k.len() + v.len() + 64)
                    .sum::<usize>()
                + 256
        })
        .sum::<usize>()
        + retry
            .coverage
            .iter()
            .map(|c| c.entity.as_ref().map_or(0, String::len) + c.reason.len() + 256)
            .sum::<usize>()
        + retry
            .entities
            .iter()
            .map(|e| e.extras.len() + e.name.len() + e.image.len() + 512)
            .sum::<usize>()
        + retry.cursor.as_ref().map_or(0, |c| c.event_key.len() + 64)
}
pub(super) fn recount_aux(s: &mut State) {
    let mut read_payload = 0;
    s.shared_read_buckets.retain(|address, read| {
        let in_ring = s
            .entries
            .get(&read.key)
            .is_some_and(|entry| Arc::as_ptr(&entry.bucket) as usize == *address);
        if read.bucket.strong_count() <= usize::from(in_ring) {
            return false;
        }
        // The map and its identity strings are owned even while payload is shared.
        read_payload += 192 + read.key.0.len() + read.key.1.len();
        if !in_ring {
            read_payload += read.bytes;
        }
        true
    });
    // Conservative owned-memory allowance: strings/serialized extras + map/allocator
    // overhead, including duplicate latest facts and fingerprints.
    s.aux_bytes = read_payload
        + s.facts
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
                retry_bytes(&p.retry)
                    + p.keys
                        .iter()
                        .map(|(a, b, _)| a.len() + b.len() + 128)
                        .sum::<usize>()
                    + 256
            })
            .sum::<usize>()
        + s.retry.values().map(retry_bytes).sum::<usize>()
        + s.last_warning.len() * 384
        + s.observed.len() * 384
        + s.runtime.len() * 512
        + s.restored_coverage.len() * 320
        + s.coverage.values().map(|g| g.len() * 512).sum::<usize>();
}
pub(super) fn evict(s: &mut State, now: i64) {
    let keys: Vec<_> = s
        .entries
        .iter()
        .filter(|((_, _, t), e)| t.saturating_add(10_000) <= now - HOUR && e.pins == 0)
        .map(|(k, _)| k.clone())
        .collect();
    for key in keys {
        evict_entry(s, &key);
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
    s.last_warning
        .retain(|e, _| engines.contains(e) || s.retry.contains_key(e));
    s.coverage.retain(|e, _| engines.contains(e));
    s.restored_coverage.retain(|e| engines.contains(e));
    recount_aux(s);
    while s.bytes + s.aux_bytes + s.reader_bytes > RING_BYTES {
        if let Some(key) = s
            .entries
            .iter()
            .filter(|(_, e)| e.pins == 0)
            .min_by_key(|((_, _, t), _)| *t)
            .map(|(key, _)| key.clone())
        {
            evict_entry(s, &key);
            recount_aux(s);
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

//! Normalize container metadata and weighted observations into immutable ring buckets.
use super::*;

pub(super) fn metadata(
    s: &mut State,
    b: &mut db::DockerBatch,
    engine: &str,
    c: &api::DockerContainer,
) {
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
pub(super) fn observe(
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
        merge_metrics(&mut b, m, duration, population);
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
        admit_bucket(s, key, b, at, end);
        at = end;
    }
}

fn merge_metrics(
    b: &mut db::DockerBucket,
    m: &api::DockerMetrics,
    duration: u64,
    population: Option<(i64, bool, bool)>,
) {
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
}

fn admit_bucket(s: &mut State, key: Key, b: db::DockerBucket, at: i64, end: i64) {
    let bytes = bucket_bytes(&b);
    let old_bytes = s.entries.get(&key).map_or(0, |e| e.bytes);
    let replacing_read_payload = s.entries.get(&key).is_some_and(|e| {
        s.shared_read_buckets
            .contains_key(&(Arc::as_ptr(&e.bucket) as usize))
    });
    if s.bytes
        .saturating_sub(if replacing_read_payload { 0 } else { old_bytes })
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
        if replacing_read_payload {
            recount_aux(s);
        }
    } else {
        gap(s, &key.0, at, end, "ring_capacity");
    }
}

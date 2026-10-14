//! Choose retained tiers and merge immutable history evidence for presentation.
use super::projection::{display_points, fold_ring};
use super::*;

impl DockerRecorder {
    pub fn history(
        &self,
        q: &api::DockerHistoryRequest,
    ) -> Result<api::DockerHistoryPage, DockerServiceError> {
        if q.to_ms <= q.from_ms
            || q.to_ms.saturating_sub(q.from_ms) > db::MAX_RANGE_MS
            || q.max_points == 0
            || q.max_points > 10_000
        {
            return Err(DockerServiceError::InvalidRequest(
                "History request exceeds range or point bounds",
            ));
        }
        // Capture enough predecessor ring data for a retained coarser tier too.
        let from = q.from_ms.div_euclid(900_000) * 900_000;
        let (a, coord) = self.context();
        let mut reader = a.as_ref().map(|a| Reader::open(&a.path)).transpose()?;
        let guard = coord.lock_ok();
        let snapshot = reader
            .as_mut()
            .map(|r| r.docker_snapshot(self.host, &q.engine_id))
            .transpose()?;
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
            return Err(DockerServiceError::ResourceLimit(
                "History snapshot byte limit reached",
            ));
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
        let result = self
            .history_page(q, snapshot.as_ref(), &ring, metadata, coverage, generation)
            .and_then(|page| {
                if serde_json::to_vec(&page)?.len() > db::MAX_QUERY_BYTES {
                    return Err(DockerServiceError::ResourceLimit(
                        "History response byte limit exceeded",
                    ));
                }
                Ok(page)
            });
        drop(snapshot);
        let mut s = self.state.lock_ok();
        s.reader_bytes = s.reader_bytes.saturating_sub(held);
        if s.generation != generation || s.reset_epoch != reset_epoch {
            return Err(DockerServiceError::SnapshotExpired(
                "History was reset or engine changed",
            ));
        }
        result
    }
    pub(super) fn history_page(
        &self,
        q: &api::DockerHistoryRequest,
        snapshot: Option<&db::DockerReadSnapshot<'_>>,
        ring: &[Arc<db::DockerBucket>],
        metadata: Vec<db::DockerEntity>,
        coverage: Vec<api::DockerCoverage>,
        generation: u32,
    ) -> Result<api::DockerHistoryPage, DockerServiceError> {
        let started = Instant::now();
        let (tier, from, persisted) = select_history(q, snapshot)?;
        let ring: Vec<_> = ring
            .iter()
            .filter(|b| b.bucket_ms >= from)
            .cloned()
            .collect();
        let buckets = merge_buckets(q, snapshot, &ring, tier, from, persisted.buckets)?;
        let mut points = display_points(q, tier, buckets)?;
        let metas = history_metadata(q, &mut points, persisted.metadata, metadata);
        let gaps = history_coverage(q, persisted.coverage, coverage);
        if started.elapsed() > Duration::from_secs(2) {
            return Err(DockerServiceError::SnapshotExpired(
                "History snapshot expired",
            ));
        }
        let mut persistence = self.status();
        let durable_start = snapshot
            .as_ref()
            .map(|snap| snap.retained_range())
            .transpose()?
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
        Ok(page)
    }
}

fn select_history(
    q: &api::DockerHistoryRequest,
    snapshot: Option<&db::DockerReadSnapshot<'_>>,
) -> Result<(Tier, i64, db::DockerHistory), DockerServiceError> {
    let range = q.to_ms - q.from_ms;
    let desired = range / i64::from(q.max_points);
    let mut tier = if desired <= 10_000 && range <= 86_400_000 {
        Tier::S10
    } else if desired <= 60_000 && range <= 7 * 86_400_000 {
        Tier::M1
    } else {
        Tier::M15
    };
    let entity = q.container_id.as_deref().unwrap_or("");
    let mut from = q.from_ms.div_euclid(tier.bucket_ms().unwrap_or(10_000))
        * tier.bucket_ms().unwrap_or(10_000);
    let mut persisted = if let Some(snapshot) = &snapshot {
        snapshot.history(&db::DockerHistoryQuery {
            entity: entity.into(),
            start_ms: from,
            end_ms: q.to_ms,
            tier,
            max_points: 10_000,
        })?
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
            let older = snapshot.history(&db::DockerHistoryQuery {
                entity: entity.into(),
                start_ms: start,
                end_ms: q.to_ms,
                tier: candidate,
                max_points: 10_000,
            })?;
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
                    })?
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
    Ok((tier, from, persisted))
}

fn merge_buckets(
    q: &api::DockerHistoryRequest,
    snapshot: Option<&db::DockerReadSnapshot<'_>>,
    ring: &[Arc<db::DockerBucket>],
    tier: Tier,
    from: i64,
    persisted: Vec<db::DockerBucket>,
) -> Result<BTreeMap<i64, db::DockerBucket>, DockerServiceError> {
    let entity = q.container_id.as_deref().unwrap_or("");
    let width = tier.bucket_ms().unwrap_or(10_000);
    let mut buckets: BTreeMap<i64, db::DockerBucket> =
        persisted.into_iter().map(|b| (b.bucket_ms, b)).collect();
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
                })?
                .buckets
            {
                fine.insert(b.bucket_ms, Arc::new(b));
            }
        }
    }
    for b in ring {
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
    Ok(buckets)
}

fn history_metadata(
    q: &api::DockerHistoryRequest,
    points: &mut [api::DockerHistoryPoint],
    persisted: Vec<db::DockerEntity>,
    metadata: Vec<db::DockerEntity>,
) -> Vec<api::DockerContainer> {
    let mut meta_rows: BTreeMap<(i64, i64), db::DockerEntity> = persisted
        .into_iter()
        .chain(metadata)
        .map(|e| ((e.effective_ms, e.revision), e))
        .collect();
    let predecessor = meta_rows
        .keys()
        .rfind(|(time, _)| *time < q.from_ms)
        .copied();
    meta_rows.retain(|key, _| key.0 < q.to_ms && (key.0 >= q.from_ms || Some(*key) == predecessor));
    for point in points {
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
    metas
}

fn history_coverage(
    q: &api::DockerHistoryRequest,
    persisted: Vec<db::DockerCoverage>,
    coverage: Vec<api::DockerCoverage>,
) -> Vec<api::DockerCoverage> {
    let mut gaps: Vec<_> = persisted
        .into_iter()
        .map(|c| api::DockerCoverage {
            start_ms: c.start_ms,
            end_ms: c.end_ms,
            container_id: c.entity,
            reason: c.reason,
        })
        .collect();
    for c in coverage {
        if let Some(old) = gaps.iter_mut().find(|g| {
            g.start_ms == c.start_ms && g.container_id == c.container_id && g.reason == c.reason
        }) {
            old.end_ms = old.end_ms.max(c.end_ms);
        } else {
            gaps.push(c);
        }
    }
    gaps.retain(|c| c.start_ms < q.to_ms && c.end_ms.is_none_or(|end| end > q.from_ms));
    gaps
}

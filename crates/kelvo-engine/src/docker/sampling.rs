//! Bounded inventory refresh, monitored population and fair stats polling.
use super::*;
impl DockerHandle {
    pub(super) async fn cycle(
        &self,
        adapter: &mut DockerAdapter,
        inventory: &mut Inventory,
        samples: &mut Samples,
        refresh: bool,
        generation: u32,
    ) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        if (refresh || inventory.refreshed_at.elapsed() >= Duration::from_secs(30))
            && !self
                .refresh_inventory(
                    adapter,
                    inventory,
                    &mut samples.capacity_at,
                    deadline,
                    generation,
                )
                .await
        {
            return false;
        }
        inventory.select_population(&mut samples.baseline);
        collect_stats(
            adapter,
            &mut inventory.rows,
            &mut samples.baseline,
            &mut samples.cursor,
            refresh,
            deadline,
        )
        .await
    }
}

/// Inventory facts and their bounded monitoring selection share a refresh clock.
pub(super) struct Inventory {
    pub(super) rows: Vec<DockerContainer>,
    pub(super) refreshed_at: Instant,
    pub(super) complete: bool,
    pub(super) population: BTreeSet<String>,
    pub(super) revision: u32,
}
/// Counter baselines and round-robin position belong to one connected session.
pub(super) struct Samples {
    pub(super) baseline: BTreeMap<String, (DockerCounters, Instant)>,
    pub(super) cursor: usize,
    pub(super) capacity_at: Instant,
}
impl Inventory {
    pub(super) fn new(revision: u32) -> Self {
        Self {
            rows: Vec::new(),
            refreshed_at: Instant::now() - Duration::from_secs(60),
            complete: true,
            population: BTreeSet::new(),
            revision,
        }
    }
    fn select_population(&mut self, baseline: &mut BTreeMap<String, (DockerCounters, Instant)>) {
        let Self {
            rows,
            population,
            revision: population_revision,
            ..
        } = self;
        // The daemon inventory is authoritative. Recompute the bounded population
        // so newly running containers displace stopped containers on every refresh.
        let selected: BTreeSet<_> = rows
            .iter()
            .filter(|c| c.lifecycle == DockerLifecycle::Running)
            .chain(
                rows.iter()
                    .filter(|c| c.lifecycle != DockerLifecycle::Running),
            )
            .take(128)
            .map(|c| c.id.clone())
            .collect();
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
    }
}
impl Samples {
    pub(super) fn new() -> Self {
        Self {
            baseline: BTreeMap::new(),
            cursor: 0,
            capacity_at: Instant::now(),
        }
    }
}
impl DockerHandle {
    async fn refresh_inventory(
        &self,
        adapter: &mut DockerAdapter,
        inventory: &mut Inventory,
        capacity_at: &mut Instant,
        deadline: Instant,
        generation: u32,
    ) -> bool {
        let Inventory {
            rows,
            refreshed_at: inventory_at,
            complete: inventory_complete,
            ..
        } = inventory;
        if capacity_at.elapsed() >= Duration::from_secs(30)
            && matches!(
                tokio::time::timeout_at(deadline, adapter.refresh_capacity()).await,
                Ok(Ok(()))
            )
        {
            *capacity_at = Instant::now();
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
        true
    }
}
async fn collect_stats(
    adapter: &DockerAdapter,
    rows: &mut [DockerContainer],
    baseline: &mut BTreeMap<String, (DockerCounters, Instant)>,
    cursor: &mut usize,
    refresh: bool,
    deadline: Instant,
) -> bool {
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
                    .map(|(_, t)| mono.duration_since(*t).as_millis().min(u32::MAX as u128) as u32)
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

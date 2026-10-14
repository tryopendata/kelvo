//! Source-derived aggregate presentation and snapshot publication.
use super::*;
pub(super) fn unavailable_runtime(runtime: &str) -> DockerRuntimeFootprint {
    DockerRuntimeFootprint{scope:if runtime=="OrbStack"{"shared"}else{"unavailable"}.into(),explanation:if runtime=="OrbStack"{"OrbStack shares its runtime with other workloads. Docker-only process attribution has not been verified."}else{"Runtime process attribution is unavailable; container metrics do not measure the Mac runtime's footprint."}.into(),cpu_pct:None,footprint_bytes:None,observed_ms:None,processes:Vec::new()}
}
pub(super) fn aggregate(
    rows: &[DockerContainer],
    capacity: Option<f64>,
    inventory_complete: bool,
    now: i64,
    interval: u32,
) -> DockerAggregate {
    aggregate_rows(rows.iter(), capacity, inventory_complete, now, interval)
}
pub(super) fn project_aggregates(
    rows: &[DockerContainer],
    capacity: Option<f64>,
    inventory_complete: bool,
    now: i64,
    interval: u32,
) -> Vec<DockerProjectAggregate> {
    let mut projects: BTreeMap<Option<String>, Vec<&DockerContainer>> = BTreeMap::new();
    for row in rows {
        projects.entry(row.project.clone()).or_default().push(row);
    }
    projects
        .into_iter()
        .map(|(project, rows)| DockerProjectAggregate {
            project,
            aggregate: aggregate_rows(
                rows.iter().copied(),
                capacity,
                inventory_complete,
                now,
                interval,
            ),
        })
        .collect()
}
pub(super) fn aggregate_rows<'a>(
    rows: impl Iterator<Item = &'a DockerContainer> + Clone,
    capacity: Option<f64>,
    inventory_complete: bool,
    now: i64,
    interval: u32,
) -> DockerAggregate {
    let running = rows
        .clone()
        .filter(|c| c.lifecycle == DockerLifecycle::Running)
        .count() as u32;
    let expected = rows
        .clone()
        .filter(|c| c.monitored && c.lifecycle == DockerLifecycle::Running)
        .count() as u32;
    let fresh = |c: &&DockerContainer| {
        c.monitored
            && c.lifecycle == DockerLifecycle::Running
            && c.observed_ms
                .is_some_and(|t| now.saturating_sub(t) <= interval.max(5000) as i64 * 2)
    };
    let cpu: Vec<_> = rows
        .clone()
        .filter(fresh)
        .filter_map(|c| c.metrics.cpu_pct)
        .collect();
    let memory: Vec<_> = rows
        .clone()
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
        total: rows.clone().count() as u32,
        monitored: rows.clone().filter(|c| c.monitored).count() as u32,
        partial: expected < running
            || cpu.len() < expected as usize
            || memory.len() < expected as usize
            || !inventory_complete,
        inventory_complete,
    }
}

impl DockerHandle {
    pub(super) fn publish_cycle(
        &self,
        adapter: &DockerAdapter,
        control: &Control,
        inventory: &mut Inventory,
        restarts: &mut BTreeMap<String, VecDeque<i64>>,
        last_observation: &mut Option<Instant>,
        ok: bool,
    ) {
        let interval = control
            .min_interval_ms
            .max(if control.visible > 0 { 1000 } else { 10000 });
        let observed = now_ms();
        for row in &mut inventory.rows {
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
        snapshot.generation = control.generation;
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
        *last_observation = Some(observed_mono);
        snapshot.population_revision = inventory.revision;
        // Receipt time differs per request; preserve it and publish source intervals.
        snapshot.containers = inventory.rows.clone();
        snapshot.aggregate = aggregate(
            &inventory.rows,
            adapter.engine.capacity_bytes,
            inventory.complete,
            observed,
            interval,
        );
        snapshot.project_aggregates = project_aggregates(
            &inventory.rows,
            adapter.engine.capacity_bytes,
            inventory.complete,
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
    }
}

//! All observed identities before display floors. Intervals are split while recording;
//! retained queries never prorate a stored bucket into invented precision.
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use kelvo_collect::{ProcessSample, SampleBuf, Tick};
use kelvo_schema::durable_usage::{UsageIncompleteCause, UsageNetworkUncertainty};
use kelvo_store::usage::{
    USAGE_BUCKET_MS, UsageAppFacts, UsageBucket, UsageFacts, UsageProcessFacts,
    UsageSourceCoverage, cause_mask,
};

mod ring;
pub(crate) use ring::UsageRecent;

#[derive(Default)]
struct Spans {
    first: Option<(i64, i64)>,
    extra: Vec<(i64, i64)>,
}
impl Spans {
    fn add(&mut self, mut from: i64, mut to: i64) -> u64 {
        let before = self.first.map_or(0, |(a, b)| b - a)
            + self.extra.iter().map(|(a, b)| b - a).sum::<i64>();
        if let Some((a, b)) = self.first.take() {
            if from <= b && to >= a {
                from = from.min(a);
                to = to.max(b);
            } else {
                self.extra.push((a, b));
            }
        }
        // Normally one continuous interval, with no heap allocation. Disjoint
        // fragments keep their buffer for unusual pause/re-enable sequences.
        loop {
            let mut merged = false;
            self.extra.retain(|&(a, b)| {
                if from <= b && to >= a {
                    from = from.min(a);
                    to = to.max(b);
                    merged = true;
                    false
                } else {
                    true
                }
            });
            if !merged {
                break;
            }
        }
        self.first = Some((from, to));
        let after = to - from + self.extra.iter().map(|(a, b)| b - a).sum::<i64>();
        u64::try_from(after.saturating_sub(before)).unwrap_or(0)
    }
}
#[derive(Default)]
struct Coverage {
    spans: [Spans; 6],
}
impl Coverage {
    fn add(&mut self, resource: usize, from: i64, to: i64) -> u64 {
        self.spans
            .get_mut(resource)
            .map_or(0, |spans| spans.add(from, to))
    }
}

struct Open {
    bucket: UsageBucket,
    source: Coverage,
    apps: HashMap<Arc<str>, (usize, Coverage)>,
    processes: HashMap<(i32, i64), (usize, Coverage)>,
}
impl Open {
    fn new(from_ms: i64, capacity: usize) -> Self {
        Self {
            bucket: UsageBucket {
                from_ms,
                to_ms: from_ms + USAGE_BUCKET_MS,
                source: Default::default(),
                apps: Vec::with_capacity(capacity),
                processes: Vec::with_capacity(capacity),
                uncertain_network: Vec::new(),
            },
            source: Coverage::default(),
            apps: HashMap::with_capacity(capacity),
            processes: HashMap::with_capacity(capacity),
        }
    }
    fn app(&mut self, name: &Arc<str>) -> (&mut UsageFacts, &mut Coverage) {
        let (index, coverage) = self.apps.entry(name.clone()).or_insert_with(|| {
            let index = self.bucket.apps.len();
            self.bucket.apps.push(UsageAppFacts {
                name: name.clone(),
                facts: UsageFacts::default(),
            });
            (index, Coverage::default())
        });
        // The index is inserted together with this row and rows are never removed.
        let row = self
            .bucket
            .apps
            .get_mut(*index)
            .expect("app index inserted with row");
        (&mut row.facts, coverage)
    }
    fn process(&mut self, row: &ProcessSample, app: &Arc<str>) -> (&mut UsageFacts, &mut Coverage) {
        let (index, coverage) = self
            .processes
            .entry((row.pid, row.start_time_us))
            .or_insert_with(|| {
                let index = self.bucket.processes.len();
                self.bucket.processes.push(UsageProcessFacts {
                    app_name: app.clone(),
                    pid: row.pid,
                    start_time_us: row.start_time_us,
                    name: row.name.clone(),
                    facts: UsageFacts::default(),
                });
                (index, Coverage::default())
            });
        let row = self
            .bucket
            .processes
            .get_mut(*index)
            .expect("process index inserted with row");
        (&mut row.facts, coverage)
    }
}

/// Open buckets allow a slow source to complete earlier spans. Closed snapshots enter
/// the recent ring before the writer queue, so a durable/recent query cannot go backward.
#[derive(Default)]
pub(crate) struct UsageRecorder {
    open: BTreeMap<i64, Open>,
    minimum_ms: i64,
    concurrent: HashMap<(i64, Arc<str>), u64>,
    capacity: usize,
    spare: Vec<Open>,
}
impl UsageRecorder {
    pub fn restart(&mut self, minimum_ms: i64) {
        self.minimum_ms = minimum_ms;
    }
    pub fn clear(&mut self, minimum_ms: i64) {
        self.open.clear();
        self.restart(minimum_ms);
    }
    pub fn drop_after(&mut self, at: i64) {
        self.open.retain(|_, b| b.bucket.to_ms <= at);
        self.minimum_ms = at;
    }
    fn ranges(&self, from: i64, to: i64) -> impl Iterator<Item = (i64, i64, i64)> + use<> {
        let from = from.max(self.minimum_ms);
        let first = from.div_euclid(USAGE_BUCKET_MS) * USAGE_BUCKET_MS;
        let count = if to > from {
            (to - 1 - first) / USAGE_BUCKET_MS + 1
        } else {
            0
        };
        (0..count).map(move |i| {
            let start = first + i * USAGE_BUCKET_MS;
            (start, from.max(start), to.min(start + USAGE_BUCKET_MS))
        })
    }
    fn slot(&mut self, start: i64) -> &mut Open {
        let capacity = self.capacity;
        let spare = &mut self.spare;
        self.open.entry(start).or_insert_with(|| {
            if let Some(mut slot) = spare.pop() {
                slot.bucket.from_ms = start;
                slot.bucket.to_ms = start + USAGE_BUCKET_MS;
                slot.bucket.source = Default::default();
                slot.bucket.apps = Vec::with_capacity(capacity);
                slot.bucket.processes = Vec::with_capacity(capacity);
                slot.source = Coverage::default();
                slot.apps.clear();
                slot.processes.clear();
                slot
            } else {
                Open::new(start, capacity)
            }
        })
    }

    pub fn record(
        &mut self,
        tick: Tick,
        rows: &[ProcessSample],
        buf: &SampleBuf,
        enabled: [bool; 5],
        supported: [bool; 5],
    ) {
        self.capacity = self.capacity.max(rows.len());
        self.sources(tick.wall_ms, buf, enabled, supported);
        self.processes(tick.wall_ms, rows, buf, enabled, supported);
        self.network(tick, buf, enabled[4], supported[4]);
        for slot in self.open.values_mut() {
            for (source, supported) in slot.bucket.source.iter_mut().zip(supported) {
                source.supported |= supported;
            }
        }
    }
    fn sources(&mut self, ts: i64, buf: &SampleBuf, enabled: [bool; 5], supported: [bool; 5]) {
        for (resource, observation) in buf.usage_observations().iter().enumerate() {
            if !enabled.get(resource).copied().unwrap_or(false) {
                continue;
            }
            let Some(observation) = observation else {
                continue;
            };
            let span = i64::try_from(observation.span_ms).unwrap_or(i64::MAX);
            let ranges = self.ranges(ts.saturating_sub(span.max(1)), ts);
            for (start, from, to) in ranges {
                let slot = self.slot(start);
                let measured = if span > 0 {
                    slot.source.add(resource, from, to)
                } else {
                    0
                };
                if let Some(source) = slot.bucket.source.get_mut(resource) {
                    source.supported = supported.get(resource).copied().unwrap_or(false);
                    source.measured_ms += measured;
                    source.causes |= observation
                        .causes
                        .iter()
                        .flatten()
                        .fold(0, |m, c| m | cause_mask(*c));
                }
            }
        }
    }
    fn processes(
        &mut self,
        ts: i64,
        rows: &[ProcessSample],
        buf: &SampleBuf,
        enabled: [bool; 5],
        supported: [bool; 5],
    ) {
        let mut concurrent = std::mem::take(&mut self.concurrent);
        concurrent.clear();
        for row in rows {
            let app = row.app.clone().unwrap_or_else(|| row.name.clone());
            let span = (f64::from(row.interval_s) * 1000.0).round() as i64;
            if span <= 0 {
                continue;
            }
            for (start, from, to) in self.ranges(ts.saturating_sub(span), ts) {
                let slot = self.slot(start);
                let source = slot.bucket.source;
                let (facts, coverage) = slot.process(row, &app);
                add_process(facts, coverage, row, (from, to), enabled, supported, source);
                let (facts, coverage) = slot.app(&app);
                add_process(facts, coverage, row, (from, to), enabled, supported, source);
                if enabled[2] {
                    *concurrent.entry((start, app.clone())).or_default() += row.mem_bytes;
                }
            }
            if enabled[1]
                && let Some(span) = buf
                    .process_gpu_span_ms()
                    .and_then(|v| i64::try_from(v).ok())
            {
                for (start, from, to) in self.ranges(ts.saturating_sub(span), ts) {
                    let slot = self.slot(start);
                    let source = slot.bucket.source.get(1).copied().unwrap_or_default();
                    let pct = f64::from(row.gpu_pct.unwrap_or(0.0));
                    let (facts, coverage) = slot.process(row, &app);
                    add_gpu(facts, coverage, pct, from, to, source);
                    let (facts, coverage) = slot.app(&app);
                    add_gpu(facts, coverage, pct, from, to, source);
                }
            }
        }
        for ((start, app), &memory) in &concurrent {
            let (facts, _) = self.slot(*start).app(app);
            facts.mem_peak_bytes = facts.mem_peak_bytes.max(memory);
        }
        self.concurrent = concurrent;
        // GPU evidence for a pid missing from process rows cannot acquire an invented
        // process instance or app identity. Keep the source explicitly incomplete.
        if buf
            .process_gpu()
            .iter()
            .any(|g| !rows.iter().any(|p| p.pid == g.pid))
        {
            for slot in self.open.values_mut().filter(|s| {
                s.bucket.from_ms < ts
                    && s.bucket.to_ms
                        >= ts.saturating_sub(buf.process_gpu_span_ms().unwrap_or(0) as i64)
            }) {
                if let Some(source) = slot.bucket.source.get_mut(1) {
                    source.causes |= cause_mask(UsageIncompleteCause::UnresolvedIdentity);
                }
            }
        }
    }
    fn network(&mut self, tick: Tick, buf: &SampleBuf, enabled: bool, supported: bool) {
        if !enabled {
            return;
        }
        let Some(interval) = buf.process_net_interval() else {
            return;
        };
        let span = i64::try_from(interval.now_ns.saturating_sub(interval.prev_ns) / 1_000_000)
            .unwrap_or(i64::MAX);
        if span <= 0 {
            return;
        }
        for row in buf.process_net() {
            for (start, from, to) in self.ranges(tick.wall_ms.saturating_sub(span), tick.wall_ms) {
                let slot = self.slot(start);
                let source = slot.bucket.source.get(4).copied().unwrap_or_default();
                let Some(app) = &row.identity else {
                    if let Some(source) = slot.bucket.source.get_mut(4) {
                        source.causes |= cause_mask(UsageIncompleteCause::UnresolvedIdentity);
                    }
                    continue;
                };
                let (facts, coverage) = slot.app(app);
                let origin = tick.wall_ms.saturating_sub(span);
                let portion = |bytes: u64| {
                    let before = u128::from(bytes) * u128::try_from(from - origin).unwrap_or(0)
                        / span as u128;
                    let after =
                        u128::from(bytes) * u128::try_from(to - origin).unwrap_or(0) / span as u128;
                    u64::try_from(after.saturating_sub(before)).unwrap_or(u64::MAX)
                };
                facts.network_rx_bytes =
                    facts.network_rx_bytes.saturating_add(portion(row.rx_bytes));
                facts.network_tx_bytes =
                    facts.network_tx_bytes.saturating_add(portion(row.tx_bytes));
                add_coverage(
                    facts,
                    coverage,
                    4,
                    from,
                    to,
                    UsageSourceCoverage {
                        supported,
                        ..source
                    },
                );
            }
            if row.late_rx_bytes > 0 || row.late_tx_bytes > 0 {
                let since = buf.net_session_from_ns().unwrap_or(interval.prev_ns);
                let ago = i64::try_from(tick.continuous_ns.saturating_sub(since) / 1_000_000)
                    .unwrap_or(i64::MAX);
                let start =
                    tick.wall_ms.saturating_sub(1).div_euclid(USAGE_BUCKET_MS) * USAGE_BUCKET_MS;
                self.slot(start)
                    .bucket
                    .uncertain_network
                    .push(UsageNetworkUncertainty {
                        app_name: row.identity.as_ref().map(ToString::to_string),
                        possible_from_ms: tick.wall_ms.saturating_sub(ago),
                        possible_to_ms: tick.wall_ms,
                        rx_bytes: row.late_rx_bytes,
                        tx_bytes: row.late_tx_bytes,
                        records: 1,
                    });
            }
        }
    }
    pub fn buckets(&self) -> impl Iterator<Item = &UsageBucket> {
        self.open.values().map(|s| &s.bucket)
    }
    pub fn snapshots(&self) -> Vec<UsageBucket> {
        self.open.values().map(|s| s.bucket.clone()).collect()
    }
    pub fn close_before(&mut self, before: i64) -> Vec<UsageBucket> {
        let spare = &mut self.spare;
        self.open
            .extract_if(.., |_, slot| slot.bucket.to_ms <= before)
            .map(|(_, mut slot)| {
                let bucket = std::mem::replace(
                    &mut slot.bucket,
                    UsageBucket {
                        from_ms: 0,
                        to_ms: 0,
                        source: Default::default(),
                        apps: Vec::new(),
                        processes: Vec::new(),
                        uncertain_network: Vec::new(),
                    },
                );
                spare.push(slot);
                bucket
            })
            .collect()
    }
}

fn add_coverage(
    facts: &mut UsageFacts,
    coverage: &mut Coverage,
    resource: usize,
    from: i64,
    to: i64,
    source: UsageSourceCoverage,
) {
    let measured = coverage.add(resource, from, to);
    if let Some(c) = facts.coverage.get_mut(resource) {
        c.supported |= source.supported;
        c.measured_ms += measured;
        c.causes |= source.causes;
    }
}
fn add_gpu(
    facts: &mut UsageFacts,
    coverage: &mut Coverage,
    pct: f64,
    from: i64,
    to: i64,
    source: UsageSourceCoverage,
) {
    if pct.is_finite() && pct >= 0.0 {
        facts.gpu_seconds += pct / 100.0 * (to - from) as f64 / 1000.0;
        add_coverage(facts, coverage, 1, from, to, source);
    }
}
fn add_process(
    facts: &mut UsageFacts,
    coverage: &mut Coverage,
    row: &ProcessSample,
    bounds: (i64, i64),
    enabled: [bool; 5],
    supported: [bool; 5],
    source: [UsageSourceCoverage; 5],
) {
    let (from, to) = bounds;
    let ms = (to - from) as f64;
    for resource in [0, 2, 3] {
        if enabled[resource] && supported[resource] {
            add_coverage(facts, coverage, resource, from, to, source[resource]);
        }
    }
    if enabled[0] && row.cpu_pct.is_finite() {
        facts.cpu_seconds += f64::from(row.cpu_pct.max(0.0)) / 100.0 * ms / 1000.0;
    }
    if enabled[3] && supported[3] && row.energy_j.is_finite() && row.interval_s > 0.0 {
        facts.energy_j +=
            f64::from(row.energy_j.max(0.0)) / f64::from(row.interval_s) * ms / 1000.0;
    }
    if enabled[2] {
        facts.mem_byte_ms += row.mem_bytes as f64 * ms;
        facts.mem_peak_bytes = facts.mem_peak_bytes.max(row.mem_bytes);
        facts.present_ms += coverage.add(5, from, to);
    }
}

#[cfg(test)]
mod tests;

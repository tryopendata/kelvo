//! The last hour of per-process usage (CPU, GPU, disk, memory, energy), for "what used
//! this resource over the chart window or a brushed range" (D-093, D-099).
//!
//! Every process batch is charged pro rata: a row's rates times its interval
//! ([`ProcessSample::interval_s`]) give what the process used over `[ts - interval, ts)`,
//! and that amount is split across the 10 s buckets the interval overlaps. A sample taken
//! every 30 s in the background spreads over three buckets rather than landing whole in
//! one, so a 10 s selection reads a sane average. Each bucket also keeps the time the
//! samples covered (`covered_ms`, and `gpu_covered_ms` for batches that measured GPU):
//! averages divide by that, not by the range's width, so sleep (the collector restarts
//! from a fresh baseline after a wake) and stretches with no sample count as unmeasured
//! rather than as zero use.
//!
//! Two levels are kept per bucket:
//! - Apps, summed from every row of each batch with no per-process floor, so an app of
//!   thirty small helpers reads right. Memory is a level: an app's footprint is the sum
//!   of its processes in one batch, and a bucket keeps the largest such sum (peak) and
//!   byte-seconds over the time the app was present (average while running). Apps below
//!   [`APP_FLOOR`] in a batch are left out of that bucket.
//! - Processes, the rows an app expands to, kept only above [`PROC_FLOOR`]: idle daemons
//!   would multiply the ring's size for nothing a table could show. Children therefore
//!   need not add up to their app.
//!
//! Bucket totals sum every row before any floor, so a remainder against a host total is
//! right.
//!
//! In memory only, kept for [`USAGE_KEEP_MS`]: the longest chart window is an hour
//! (D-091), so nothing older is ever asked for. It starts empty when the app starts and
//! says so through [`UsageByApp::since_ms`].

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use kelvo_collect::ProcessSample;

/// Width of a usage bucket.
pub const USAGE_BUCKET_MS: i64 = 10_000;

/// How far back the ring reaches: the longest chart window plus one bucket, so an
/// hour-long window still finds its first bucket.
pub const USAGE_KEEP_MS: i64 = 3_600_000 + USAGE_BUCKET_MS;

/// The least a process or app must use over a batch's interval to be kept. Rates, not
/// amounts, so a floor means the same at every cadence. A row clears it on any one.
#[derive(Clone, Copy, Debug)]
struct Floor {
    /// Percent of one core.
    cpu_pct: f64,
    /// Read plus write, bytes per second.
    disk_bps: f64,
    watts: f64,
    mem_bytes: u64,
}

impl Floor {
    fn clears(&self, rate: &Use, mem_bytes: u64) -> bool {
        rate.cpu_s * 100.0 >= self.cpu_pct
            || rate.read_b + rate.write_b >= self.disk_bps
            || rate.gpu_s > 0.0
            || rate.energy_j >= self.watts
            || mem_bytes >= self.mem_bytes
    }
}

/// An app's floor: memory is summed over its processes, so a many-helper app clears it.
const APP_FLOOR: Floor = Floor {
    cpu_pct: 0.1,
    disk_bps: 1_024.0,
    watts: 1e-5,
    mem_bytes: 8 << 20,
};

/// A process's floor, for an app's expanded rows.
const PROC_FLOOR: Floor = Floor {
    cpu_pct: 0.1,
    disk_bps: 1_024.0,
    watts: 1e-5,
    mem_bytes: 32 << 20,
};

/// What to sort apps by in [`UsageRing::by_app`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsageKey {
    Cpu,
    Gpu,
    /// Peak footprint.
    Memory,
    /// Read plus written.
    Disk,
    Energy,
}

/// Amounts used over a span (or, in a floor check, per second of it).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Use {
    cpu_s: f64,
    gpu_s: f64,
    read_b: f64,
    write_b: f64,
    energy_j: f64,
}

impl Use {
    fn of(r: &ProcessSample) -> Self {
        let secs = f64::from(r.interval_s.max(0.0));
        Self {
            cpu_s: f64::from(r.cpu_pct) / 100.0 * secs,
            gpu_s: f64::from(r.gpu_pct.unwrap_or(0.0)) / 100.0 * secs,
            read_b: f64::from(r.disk_read_bps) * secs,
            write_b: f64::from(r.disk_write_bps) * secs,
            energy_j: f64::from(r.energy_j),
        }
    }

    fn add(&mut self, o: &Self) {
        self.cpu_s += o.cpu_s;
        self.gpu_s += o.gpu_s;
        self.read_b += o.read_b;
        self.write_b += o.write_b;
        self.energy_j += o.energy_j;
    }

    fn scaled(&self, f: f64) -> Self {
        Self {
            cpu_s: self.cpu_s * f,
            gpu_s: self.gpu_s * f,
            read_b: self.read_b * f,
            write_b: self.write_b * f,
            energy_j: self.energy_j * f,
        }
    }
}

/// [`Use`] as a bucket stores it: `f32` keeps the hour small.
#[derive(Clone, Copy, Debug, Default)]
struct Use32 {
    cpu_s: f32,
    gpu_s: f32,
    read_b: f32,
    write_b: f32,
    energy_j: f32,
}

impl Use32 {
    fn add(&mut self, u: &Use) {
        self.cpu_s += u.cpu_s as f32;
        self.gpu_s += u.gpu_s as f32;
        self.read_b += u.read_b as f32;
        self.write_b += u.write_b as f32;
        self.energy_j += u.energy_j as f32;
    }

    fn wide(&self) -> Use {
        Use {
            cpu_s: f64::from(self.cpu_s),
            gpu_s: f64::from(self.gpu_s),
            read_b: f64::from(self.read_b),
            write_b: f64::from(self.write_b),
            energy_j: f64::from(self.energy_j),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct AppSlot {
    used: Use32,
    /// Footprint times the time present, byte-seconds.
    mem_byte_s: f32,
    /// Time the app was present in this bucket, ms.
    present_ms: f32,
    /// Largest footprint of one batch, KiB.
    mem_peak_kib: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct ProcSlot {
    used: Use32,
    mem_peak_kib: u32,
}

#[derive(Debug, Default)]
struct Bucket {
    start: i64,
    covered_ms: f32,
    gpu_covered_ms: f32,
    /// Every row, before any floor.
    total: Use32,
    /// By id, ascending.
    apps: Vec<(u32, AppSlot)>,
    procs: Vec<(u32, ProcSlot)>,
}

impl Bucket {
    fn reset(&mut self, start: i64) {
        self.start = start;
        self.covered_ms = 0.0;
        self.gpu_covered_ms = 0.0;
        self.total = Use32::default();
        self.apps.clear();
        self.procs.clear();
    }
}

/// The slot for `id` in a list sorted by id, inserted when missing.
fn slot<T: Default>(list: &mut Vec<(u32, T)>, id: u32) -> &mut T {
    let i = match list.binary_search_by_key(&id, |(k, _)| *k) {
        Ok(i) => i,
        Err(i) => {
            list.insert(i, (id, T::default()));
            i
        }
    };
    &mut list
        .get_mut(i)
        .expect("the index was just found or inserted")
        .1
}

/// One process as the ring knows it.
#[derive(Clone, Debug)]
struct Proc {
    pid: i32,
    start_time_us: i64,
    name: Arc<str>,
    app: u32,
    app_main: bool,
    /// Timestamp of the newest batch that listed it.
    seen_ms: i64,
    /// Start of the newest bucket that holds it.
    last_bucket: i64,
}

#[derive(Clone, Debug)]
struct App {
    name: Arc<str>,
    last_bucket: i64,
}

/// One process's use over a range.
#[derive(Clone, Debug, PartialEq)]
pub struct UsageProc {
    pub pid: i32,
    pub start_time_us: i64,
    pub name: Arc<str>,
    /// Average percent of one core over the range's covered time.
    pub cpu_avg_pct: f64,
    /// Average percent of the GPU over the time GPU was measured; `None` when it never was.
    pub gpu_avg_pct: Option<f64>,
    pub mem_peak_b: u64,
    pub read_b: f64,
    pub write_b: f64,
    pub energy_j: f64,
    pub avg_w: f64,
    /// Listed by the newest batch: still running as of then.
    pub running: bool,
    /// Its app bundle's main executable.
    pub app_main: bool,
}

/// One app's use over a range.
#[derive(Clone, Debug, PartialEq)]
pub struct UsageApp {
    /// The app identity (D-089), or the process name when none was resolved.
    pub name: Arc<str>,
    pub cpu_avg_pct: f64,
    pub gpu_avg_pct: Option<f64>,
    /// Largest footprint of its processes summed at one sample.
    pub mem_peak_b: u64,
    /// Average footprint while it was running.
    pub mem_avg_b: u64,
    pub read_b: f64,
    pub write_b: f64,
    pub energy_j: f64,
    pub avg_w: f64,
    /// Kept processes, ordered like the apps.
    pub processes: Vec<UsageProc>,
}

/// Every process's use over a range, before any floor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageTotal {
    pub cpu_avg_pct: f64,
    pub gpu_avg_pct: Option<f64>,
    pub read_b: f64,
    pub write_b: f64,
    pub energy_j: f64,
    pub avg_w: f64,
}

/// Per-app use over a range ([`UsageRing::by_app`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageByApp {
    /// The range the sums cover: the request widened to whole buckets.
    pub from_ms: i64,
    pub to_ms: i64,
    /// When the ring started counting, `None` before any batch. A range starting earlier
    /// was only measured from here.
    pub since_ms: Option<i64>,
    /// Buckets ending at or before this are final: the next sample's interval starts
    /// at the newest one, so it charges only later time. `None` before any batch.
    pub complete_to_ms: Option<i64>,
    /// Time inside the range a process sample covered, ms. Averages divide by this; 0
    /// means nothing was measured, not zero use.
    pub covered_ms: i64,
    /// The part of `covered_ms` whose samples measured GPU.
    pub gpu_covered_ms: i64,
    pub total: UsageTotal,
    /// The largest `limit` by the requested key, largest first.
    pub apps: Vec<UsageApp>,
}

#[derive(Default)]
pub struct UsageRing {
    /// Contiguous 10 s buckets, oldest first.
    buckets: VecDeque<Bucket>,
    procs: HashMap<u32, Proc>,
    ids: HashMap<(i32, i64), u32>,
    next_id: u32,
    apps: HashMap<u32, App>,
    app_ids: HashMap<Arc<str>, u32>,
    next_app: u32,
    /// Pruned buckets, for reuse.
    spare: Vec<Bucket>,
    /// One batch's per-app sums and kept processes, reused so a batch allocates nothing
    /// once the ring is warm.
    scratch_apps: HashMap<u32, AppBatch>,
    scratch_kept_apps: Vec<(u32, AppBatch)>,
    scratch_kept: Vec<(u32, Use, u64)>,
    since_ms: Option<i64>,
    latest_ms: Option<i64>,
}

fn bucket_of(ts_ms: i64) -> i64 {
    ts_ms - ts_ms.rem_euclid(USAGE_BUCKET_MS)
}

/// Per-app sums of one batch.
#[derive(Clone, Copy, Default)]
struct AppBatch {
    used: Use,
    mem_bytes: u64,
}

impl UsageRing {
    /// Adds one process batch taken at `ts_ms`.
    pub fn push(&mut self, ts_ms: i64, rows: &[ProcessSample]) {
        if rows.is_empty() || self.latest_ms.is_some_and(|l| ts_ms < l) {
            // A baseline batch (after a wake) has no rows. An older batch only arrives
            // after the clock stepped back; the engine drops what it overlaps first, so
            // this is a batch from before that.
            return;
        }
        let span_ms = rows
            .iter()
            .map(|r| (f64::from(r.interval_s.max(0.0)) * 1_000.0).round() as i64)
            .max()
            .unwrap_or(0)
            .min(USAGE_KEEP_MS);
        let start = ts_ms - span_ms;
        self.prune(ts_ms - USAGE_KEEP_MS);
        self.extend_to(
            bucket_of(start.max(ts_ms - USAGE_KEEP_MS)),
            bucket_of(ts_ms),
        );
        let front = self.buckets.front().map_or(ts_ms, |b| b.start);
        self.since_ms.get_or_insert(start.max(front));
        self.latest_ms = Some(ts_ms);
        let gpu = rows.iter().any(|r| r.gpu_pct.is_some());

        // Sum the batch per app and pick the processes kept, before touching buckets.
        let mut total = Use::default();
        let mut apps = std::mem::take(&mut self.scratch_apps);
        let mut kept = std::mem::take(&mut self.scratch_kept);
        let mut kept_apps = std::mem::take(&mut self.scratch_kept_apps);
        apps.clear();
        kept.clear();
        kept_apps.clear();
        let secs = span_ms as f64 / 1_000.0;
        let rate = |u: &Use| u.scaled(if secs > 0.0 { 1.0 / secs } else { 0.0 });
        let last = bucket_of((ts_ms - 1).max(start));
        for r in rows {
            let used = Use::of(r);
            total.add(&used);
            let app_name = r.app.clone().unwrap_or_else(|| Arc::clone(&r.name));
            let app = self.app_id(app_name, last);
            let a = apps.entry(app).or_default();
            a.used.add(&used);
            a.mem_bytes = a.mem_bytes.saturating_add(r.mem_bytes);
            let key = (r.pid, r.start_time_us);
            let id = self.ids.get(&key).copied();
            if !PROC_FLOOR.clears(&rate(&used), r.mem_bytes) {
                // Not worth a slot, but a process the ring holds is still running.
                if let Some(p) = id.and_then(|id| self.procs.get_mut(&id)) {
                    p.seen_ms = ts_ms;
                }
                continue;
            }
            let id = id.unwrap_or_else(|| {
                let id = self.next_id;
                self.next_id = self.next_id.wrapping_add(1);
                self.ids.insert(key, id);
                id
            });
            self.procs.insert(
                id,
                Proc {
                    pid: r.pid,
                    start_time_us: r.start_time_us,
                    name: Arc::clone(&r.name),
                    app,
                    app_main: r.app_main,
                    seen_ms: ts_ms,
                    last_bucket: last,
                },
            );
            kept.push((id, used, r.mem_bytes));
        }
        kept_apps.extend(
            apps.iter()
                .filter(|(_, a)| APP_FLOOR.clears(&rate(&a.used), a.mem_bytes))
                .map(|(id, a)| (*id, *a)),
        );

        // Charge each bucket its share of the batch's interval.
        if span_ms > 0 {
            self.charge(start, ts_ms, gpu, &total, &kept_apps, &kept);
        }
        self.scratch_apps = apps;
        self.scratch_kept = kept;
        self.scratch_kept_apps = kept_apps;
    }

    /// Adds a batch over `[start, ts_ms)` to the buckets it overlaps, each in proportion
    /// to the time it covers.
    fn charge(
        &mut self,
        start: i64,
        ts_ms: i64,
        gpu: bool,
        total: &Use,
        apps: &[(u32, AppBatch)],
        kept: &[(u32, Use, u64)],
    ) {
        let span_ms = ts_ms - start;
        for b in self.buckets.iter_mut() {
            let lo = b.start.max(start);
            let hi = (b.start + USAGE_BUCKET_MS).min(ts_ms);
            if hi <= lo {
                continue;
            }
            let ms = (hi - lo) as f64;
            let f = ms / span_ms as f64;
            b.covered_ms += ms as f32;
            if gpu {
                b.gpu_covered_ms += ms as f32;
            }
            b.total.add(&total.scaled(f));
            for (id, a) in apps {
                let s = slot(&mut b.apps, *id);
                s.used.add(&a.used.scaled(f));
                s.mem_byte_s += (a.mem_bytes as f64 * ms / 1_000.0) as f32;
                s.present_ms += ms as f32;
                s.mem_peak_kib = s.mem_peak_kib.max(kib(a.mem_bytes));
            }
            for (id, used, mem) in kept {
                let s = slot(&mut b.procs, *id);
                s.used.add(&used.scaled(f));
                s.mem_peak_kib = s.mem_peak_kib.max(kib(*mem));
            }
        }
    }

    /// The id of app `name`, which now reaches bucket `last`.
    fn app_id(&mut self, name: Arc<str>, last: i64) -> u32 {
        if let Some(&id) = self.app_ids.get(&name) {
            if let Some(a) = self.apps.get_mut(&id) {
                a.last_bucket = a.last_bucket.max(last);
            }
            return id;
        }
        let id = self.next_app;
        self.next_app = self.next_app.wrapping_add(1);
        self.app_ids.insert(Arc::clone(&name), id);
        self.apps.insert(
            id,
            App {
                name,
                last_bucket: last,
            },
        );
        id
    }

    /// Appends empty buckets so the ring runs contiguously through `last`, starting at
    /// `first` when it is empty.
    fn extend_to(&mut self, first: i64, last: i64) {
        let mut next = self
            .buckets
            .back()
            .map_or(first, |b| b.start + USAGE_BUCKET_MS);
        // Sized like the newest bucket, reusing a pruned one's storage, so a bucket costs
        // at most one allocation per list and none once the hour is full.
        let (apps, procs) = self
            .buckets
            .back()
            .map_or((0, 0), |b| (b.apps.len(), b.procs.len()));
        while next <= last {
            let mut b = self.spare.pop().unwrap_or_default();
            b.reset(next);
            b.apps.reserve(apps);
            b.procs.reserve(procs);
            self.buckets.push_back(b);
            next += USAGE_BUCKET_MS;
        }
    }

    /// Drops buckets starting before `cutoff_ms` and the processes and apps only they
    /// named.
    fn prune(&mut self, cutoff_ms: i64) {
        let mut dropped = false;
        while self.buckets.front().is_some_and(|b| b.start < cutoff_ms) {
            if let Some(b) = self.buckets.pop_front() {
                self.spare.push(b);
            }
            dropped = true;
        }
        if !dropped {
            return;
        }
        let oldest = self.buckets.front().map_or(i64::MAX, |b| b.start);
        self.forget_before(oldest);
        // Nothing before the cutoff is held any more.
        if let Some(s) = self.since_ms.as_mut() {
            *s = (*s).max(cutoff_ms);
        }
    }

    /// Forgets processes and apps whose newest bucket starts before `oldest`.
    fn forget_before(&mut self, oldest: i64) {
        let ids = &mut self.ids;
        self.procs.retain(|_, p| {
            let keep = p.last_bucket >= oldest;
            if !keep {
                ids.remove(&(p.pid, p.start_time_us));
            }
            keep
        });
        let names = &mut self.app_ids;
        self.apps.retain(|_, a| {
            let keep = a.last_bucket >= oldest;
            if !keep {
                names.remove(&a.name);
            }
            keep
        });
    }

    /// Forgets buckets ending after `ts_ms`: the wall clock stepped back.
    pub fn drop_after(&mut self, ts_ms: i64) {
        while self
            .buckets
            .back()
            .is_some_and(|b| b.start + USAGE_BUCKET_MS > ts_ms)
        {
            if let Some(b) = self.buckets.pop_back() {
                self.spare.push(b);
            }
        }
        // A process can hold use on both sides of the step: its newest bucket is now
        // the newest one left that names it.
        let mut last: HashMap<u32, i64> = HashMap::new();
        let mut last_app: HashMap<u32, i64> = HashMap::new();
        for b in &self.buckets {
            for &(id, _) in &b.procs {
                last.insert(id, b.start);
            }
            for &(id, _) in &b.apps {
                last_app.insert(id, b.start);
            }
        }
        // The newest batch kept is no later than the end of the newest bucket left.
        let latest = self
            .latest_ms
            .zip(self.buckets.back())
            .map(|(l, b)| l.min(b.start + USAGE_BUCKET_MS - 1));
        let ids = &mut self.ids;
        self.procs.retain(|id, p| match last.get(id) {
            Some(&b) => {
                p.last_bucket = b;
                // Seen by the newest batch before the step: still running as of it.
                if let Some(l) = latest {
                    p.seen_ms = p.seen_ms.min(l);
                }
                true
            }
            None => {
                ids.remove(&(p.pid, p.start_time_us));
                false
            }
        });
        let names = &mut self.app_ids;
        let procs = &self.procs;
        self.apps.retain(|id, a| {
            let b = last_app.get(id).copied().or_else(|| {
                procs
                    .values()
                    .filter(|p| p.app == *id)
                    .map(|p| p.last_bucket)
                    .max()
            });
            match b {
                Some(b) => {
                    a.last_bucket = b;
                    true
                }
                None => {
                    names.remove(&a.name);
                    false
                }
            }
        });
        if self.buckets.is_empty() {
            self.since_ms = None;
            self.latest_ms = None;
        } else {
            self.latest_ms = latest;
        }
    }

    /// Use per app over `[from_ms, to_ms)`, widened to whole buckets: the largest
    /// `limit` by `by`. A process with no resolved app forms its own group under its
    /// name.
    pub fn by_app(&self, from_ms: i64, to_ms: i64, by: UsageKey, limit: usize) -> UsageByApp {
        let from = bucket_of(from_ms);
        let to = if to_ms.rem_euclid(USAGE_BUCKET_MS) == 0 {
            to_ms
        } else {
            bucket_of(to_ms) + USAGE_BUCKET_MS
        };
        #[derive(Default)]
        struct AppSum {
            used: Use,
            mem_byte_s: f64,
            present_ms: f64,
            mem_peak_kib: u32,
        }
        let mut covered_ms = 0.0_f64;
        let mut gpu_covered_ms = 0.0_f64;
        let mut total = Use::default();
        let mut app_sums: HashMap<u32, AppSum> = HashMap::new();
        let mut proc_sums: HashMap<u32, (Use, u32)> = HashMap::new();
        for b in &self.buckets {
            if b.start < from || b.start >= to {
                continue;
            }
            covered_ms += f64::from(b.covered_ms);
            gpu_covered_ms += f64::from(b.gpu_covered_ms);
            total.add(&b.total.wide());
            for (id, s) in &b.apps {
                let a = app_sums.entry(*id).or_default();
                a.used.add(&s.used.wide());
                a.mem_byte_s += f64::from(s.mem_byte_s);
                a.present_ms += f64::from(s.present_ms);
                a.mem_peak_kib = a.mem_peak_kib.max(s.mem_peak_kib);
            }
            for (id, s) in &b.procs {
                let p = proc_sums.entry(*id).or_default();
                p.0.add(&s.used.wide());
                p.1 = p.1.max(s.mem_peak_kib);
            }
        }
        let secs = covered_ms / 1_000.0;
        let gpu_secs = gpu_covered_ms / 1_000.0;
        let per_s = |v: f64| if secs > 0.0 { v / secs } else { 0.0 };
        let cpu_pct = |u: &Use| per_s(u.cpu_s) * 100.0;
        let gpu_pct = |u: &Use| (gpu_secs > 0.0).then(|| u.gpu_s / gpu_secs * 100.0);
        let latest = self.latest_ms;

        let mut apps: HashMap<u32, UsageApp> = HashMap::new();
        let no_gpu = gpu_pct(&Use::default());
        for (id, s) in &app_sums {
            let Some(a) = app_entry(&mut apps, &self.apps, *id, no_gpu) else {
                continue;
            };
            a.cpu_avg_pct = cpu_pct(&s.used);
            a.gpu_avg_pct = gpu_pct(&s.used);
            a.mem_peak_b = u64::from(s.mem_peak_kib) * 1_024;
            a.mem_avg_b = if s.present_ms > 0.0 {
                (s.mem_byte_s / (s.present_ms / 1_000.0)).round() as u64
            } else {
                0
            };
            a.read_b = s.used.read_b;
            a.write_b = s.used.write_b;
            a.energy_j = s.used.energy_j;
            a.avg_w = per_s(s.used.energy_j);
        }
        for (id, (used, peak)) in &proc_sums {
            let Some(p) = self.procs.get(id) else {
                continue;
            };
            let Some(a) = app_entry(&mut apps, &self.apps, p.app, no_gpu) else {
                continue;
            };
            a.processes.push(UsageProc {
                pid: p.pid,
                start_time_us: p.start_time_us,
                name: Arc::clone(&p.name),
                cpu_avg_pct: cpu_pct(used),
                gpu_avg_pct: gpu_pct(used),
                mem_peak_b: u64::from(*peak) * 1_024,
                read_b: used.read_b,
                write_b: used.write_b,
                energy_j: used.energy_j,
                avg_w: per_s(used.energy_j),
                running: Some(p.seen_ms) == latest,
                app_main: p.app_main,
            });
        }
        let key = |cpu: f64, gpu: Option<f64>, mem: u64, read: f64, write: f64, j: f64| match by {
            UsageKey::Cpu => cpu,
            UsageKey::Gpu => gpu.unwrap_or(0.0),
            UsageKey::Memory => mem as f64,
            UsageKey::Disk => read + write,
            UsageKey::Energy => j,
        };
        let app_key = |a: &UsageApp| {
            key(
                a.cpu_avg_pct,
                a.gpu_avg_pct,
                a.mem_peak_b,
                a.read_b,
                a.write_b,
                a.energy_j,
            )
        };
        let proc_key = |p: &UsageProc| {
            key(
                p.cpu_avg_pct,
                p.gpu_avg_pct,
                p.mem_peak_b,
                p.read_b,
                p.write_b,
                p.energy_j,
            )
        };
        let mut apps: Vec<UsageApp> = apps
            .into_values()
            .map(|mut a| {
                a.processes
                    .sort_by(|x, y| proc_key(y).total_cmp(&proc_key(x)).then(x.pid.cmp(&y.pid)));
                a
            })
            .collect();
        apps.sort_by(|x, y| app_key(y).total_cmp(&app_key(x)).then(x.name.cmp(&y.name)));
        apps.truncate(limit);
        UsageByApp {
            from_ms: from,
            to_ms: to,
            since_ms: self.since_ms,
            complete_to_ms: latest.map(|l| l - l.rem_euclid(USAGE_BUCKET_MS)),
            covered_ms: covered_ms.round() as i64,
            gpu_covered_ms: gpu_covered_ms.round() as i64,
            total: UsageTotal {
                cpu_avg_pct: cpu_pct(&total),
                gpu_avg_pct: gpu_pct(&total),
                read_b: total.read_b,
                write_b: total.write_b,
                energy_j: total.energy_j,
                avg_w: per_s(total.energy_j),
            },
            apps,
        }
    }

    /// Bytes the buckets hold, for the size budget (D-099).
    #[cfg(test)]
    fn bucket_bytes(&self) -> usize {
        self.buckets
            .iter()
            .map(|b| {
                std::mem::size_of::<Bucket>()
                    + b.apps.capacity() * std::mem::size_of::<(u32, AppSlot)>()
                    + b.procs.capacity() * std::mem::size_of::<(u32, ProcSlot)>()
            })
            .sum()
    }
}

/// App `id`'s row in `apps`, started empty from its name in `names`.
fn app_entry<'a>(
    apps: &'a mut HashMap<u32, UsageApp>,
    names: &HashMap<u32, App>,
    id: u32,
    gpu_avg_pct: Option<f64>,
) -> Option<&'a mut UsageApp> {
    let name = Arc::clone(&names.get(&id)?.name);
    Some(apps.entry(id).or_insert_with(|| UsageApp {
        name,
        cpu_avg_pct: 0.0,
        gpu_avg_pct,
        mem_peak_b: 0,
        mem_avg_b: 0,
        read_b: 0.0,
        write_b: 0.0,
        energy_j: 0.0,
        avg_w: 0.0,
        processes: Vec::new(),
    }))
}

fn kib(bytes: u64) -> u32 {
    u32::try_from(bytes / 1_024).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_700_000_000_000;
    const MIB: u64 = 1 << 20;

    fn proc(pid: i32, name: &str, app: Option<&str>, main: bool, joules: f32) -> ProcessSample {
        ProcessSample {
            pid,
            start_time_us: i64::from(pid) * 1_000,
            name: name.into(),
            cpu_pct: 0.0,
            mem_bytes: 0,
            compressed_bytes: None,
            threads: 1,
            idle_wakeups_per_s: 0.0,
            energy: joules / 10.0,
            energy_j: joules,
            interval_s: 10.0,
            app: app.map(Into::into),
            app_main: main,
            disk_read_bps: 0.0,
            disk_write_bps: 0.0,
            net_rx_bps: None,
            net_tx_bps: None,
            gpu_pct: None,
            ports: None,
            user: "me".into(),
        }
    }

    /// A process at `cpu` percent of a core sampled every `secs`.
    fn busy(pid: i32, app: &str, cpu: f32, secs: f32) -> ProcessSample {
        let mut p = proc(pid, app, Some(app), false, 0.0);
        p.cpu_pct = cpu;
        p.interval_s = secs;
        p.energy = 0.0;
        p
    }

    fn names(r: &UsageByApp) -> Vec<&str> {
        r.apps.iter().map(|a| a.name.as_ref()).collect()
    }

    fn energy(ring: &UsageRing, from: i64, to: i64) -> UsageByApp {
        ring.by_app(from, to, UsageKey::Energy, usize::MAX)
    }

    #[test]
    fn groups_processes_by_app_and_sums_samples() {
        let mut ring = UsageRing::default();
        for i in 1..=6 {
            ring.push(
                T0 + i * 10_000,
                &[
                    proc(1, "Google Chrome", Some("Google Chrome"), true, 10.0),
                    proc(
                        2,
                        "Google Chrome Helper (Renderer)",
                        Some("Google Chrome"),
                        false,
                        20.0,
                    ),
                    proc(3, "node", Some("node"), false, 5.0),
                    proc(4, "mdworker", None, false, 0.00001),
                ],
            );
        }
        let r = energy(&ring, T0, T0 + 70_000);
        assert_eq!(r.from_ms, T0);
        // The idle process is below every floor and does not appear.
        assert_eq!(names(&r), ["Google Chrome", "node"]);
        let chrome = &r.apps[0];
        assert!((chrome.energy_j - 180.0).abs() < 1e-3);
        assert_eq!(chrome.processes[0].pid, 2);
        assert!(chrome.processes.iter().all(|p| p.running));
        assert!(chrome.processes[1].app_main);
        // Totals count every row, the idle one too.
        assert!((r.total.energy_j - 210.0).abs() < 1e-3);
        // Six 10 s intervals, each covering the bucket before its batch.
        assert_eq!(r.covered_ms, 60_000);
        assert_eq!(r.since_ms, Some(T0));
        // The next batch charges only time after the newest one.
        assert_eq!(r.complete_to_ms, Some(T0 + 60_000));
        assert!((chrome.avg_w - 3.0).abs() < 1e-6);
    }

    #[test]
    fn a_sample_spreads_over_the_buckets_its_interval_covers() {
        // Every 30 s, as in the background (D-094): 50% of a core throughout.
        let mut ring = UsageRing::default();
        for i in 1..=4 {
            ring.push(T0 + i * 30_000, &[busy(1, "A", 50.0, 30.0)]);
        }
        // Any single 10 s bucket reads the true rate, not 150% in one and 0 beside it.
        for b in 0..12 {
            let r = ring.by_app(T0 + b * 10_000, T0 + (b + 1) * 10_000, UsageKey::Cpu, 5);
            assert_eq!(r.covered_ms, 10_000, "bucket {b}");
            assert!((r.apps[0].cpu_avg_pct - 50.0).abs() < 1e-3, "bucket {b}");
        }
        // A sample whose interval straddles a bucket edge splits by time.
        let mut ring = UsageRing::default();
        ring.push(T0 + 15_000, &[busy(1, "A", 100.0, 10.0)]);
        let first = ring.by_app(T0, T0 + 10_000, UsageKey::Cpu, 5);
        assert_eq!(first.covered_ms, 5_000);
        assert!((first.total.cpu_avg_pct - 100.0).abs() < 1e-3);
        let both = ring.by_app(T0, T0 + 20_000, UsageKey::Cpu, 5);
        assert_eq!(both.covered_ms, 10_000);
    }

    #[test]
    fn time_no_sample_covered_reads_as_unmeasured() {
        let mut ring = UsageRing::default();
        ring.push(T0 + 10_000, &[busy(1, "A", 80.0, 10.0)]);
        // Asleep for five minutes: the wake's baseline batch has no rows, and the next
        // sample covers only the tick after it.
        ring.push(T0 + 310_000, &[]);
        ring.push(T0 + 320_000, &[busy(1, "A", 80.0, 10.0)]);
        let asleep = ring.by_app(T0 + 60_000, T0 + 300_000, UsageKey::Cpu, 5);
        assert_eq!(asleep.covered_ms, 0);
        let whole = ring.by_app(T0, T0 + 330_000, UsageKey::Cpu, 5);
        assert_eq!(whole.covered_ms, 20_000);
        assert!(
            (whole.apps[0].cpu_avg_pct - 80.0).abs() < 1e-3,
            "sleep does not dilute the average"
        );
    }

    #[test]
    fn app_memory_sums_its_processes_at_each_sample() {
        let mut ring = UsageRing::default();
        // Thirty 20 MiB helpers: each under the process floor, together 600 MiB.
        let helpers = |first: i32| -> Vec<ProcessSample> {
            (first..first + 30)
                .map(|pid| {
                    let mut p = busy(pid, "Google Chrome", 0.0, 10.0);
                    p.mem_bytes = 20 * MIB;
                    p
                })
                .collect()
        };
        ring.push(T0 + 10_000, &helpers(100));
        // The tabs close and thirty others open: never more than thirty at once.
        ring.push(T0 + 20_000, &helpers(200));
        let r = ring.by_app(T0, T0 + 20_000, UsageKey::Memory, 5);
        let chrome = &r.apps[0];
        assert_eq!(
            chrome.mem_peak_b,
            600 * MIB,
            "closed tabs' peaks do not add up"
        );
        assert_eq!(chrome.mem_avg_b, 600 * MIB);
        assert!(chrome.processes.is_empty(), "children keep their own floor");
    }

    #[test]
    fn memory_average_is_over_the_time_the_app_ran() {
        let mut ring = UsageRing::default();
        let mut big = busy(1, "Big", 0.0, 10.0);
        big.mem_bytes = 1_024 * MIB;
        ring.push(T0 + 10_000, &[big, busy(2, "Other", 1.0, 10.0)]);
        for i in 2..=6 {
            ring.push(T0 + i * 10_000, &[busy(2, "Other", 1.0, 10.0)]);
        }
        let r = ring.by_app(T0, T0 + 60_000, UsageKey::Memory, 5);
        assert_eq!(r.apps[0].name.as_ref(), "Big");
        assert_eq!(r.apps[0].mem_avg_b, 1_024 * MIB);
        assert!(!r.apps[0].processes[0].running);
    }

    #[test]
    fn gpu_averages_over_the_time_gpu_was_measured() {
        let mut ring = UsageRing::default();
        let mut g = busy(1, "Game", 10.0, 10.0);
        g.gpu_pct = Some(40.0);
        ring.push(T0 + 10_000, &[g]);
        // GPU not sampled for the next two batches.
        ring.push(T0 + 20_000, &[busy(1, "Game", 10.0, 10.0)]);
        ring.push(T0 + 30_000, &[busy(1, "Game", 10.0, 10.0)]);
        let r = ring.by_app(T0, T0 + 30_000, UsageKey::Gpu, 5);
        assert_eq!(r.covered_ms, 30_000);
        assert_eq!(r.gpu_covered_ms, 10_000);
        assert_eq!(r.apps[0].gpu_avg_pct.map(|v| v.round()), Some(40.0));
        let none = ring.by_app(T0 + 10_000, T0 + 30_000, UsageKey::Gpu, 5);
        assert_eq!(none.apps[0].gpu_avg_pct, None, "never measured, not 0%");
    }

    #[test]
    fn sorts_by_the_key_and_keeps_the_limit_but_totals_everything() {
        let mut ring = UsageRing::default();
        let mut disk = busy(1, "Disky", 1.0, 10.0);
        disk.disk_write_bps = 1e6;
        ring.push(
            T0 + 10_000,
            &[
                busy(2, "Busy", 90.0, 10.0),
                busy(3, "Mid", 20.0, 10.0),
                disk,
            ],
        );
        let cpu = ring.by_app(T0, T0 + 10_000, UsageKey::Cpu, 2);
        assert_eq!(names(&cpu), ["Busy", "Mid"]);
        assert!((cpu.total.cpu_avg_pct - 111.0).abs() < 1e-3);
        let disk = ring.by_app(T0, T0 + 10_000, UsageKey::Disk, 1);
        assert_eq!(names(&disk), ["Disky"]);
        assert!((disk.apps[0].write_b - 1e7).abs() < 1.0);
    }

    #[test]
    fn buckets_older_than_an_hour_go_with_their_processes() {
        let mut ring = UsageRing::default();
        ring.push(T0 + 10_000, &[busy(1, "Old", 5.0, 10.0)]);
        ring.push(T0 + USAGE_KEEP_MS + 30_000, &[busy(2, "New", 5.0, 10.0)]);
        assert!(!ring.ids.contains_key(&(1, 1_000)));
        assert_eq!(ring.procs.len(), 1);
        assert_eq!(ring.apps.len(), 1);
        let r = ring.by_app(T0, T0 + USAGE_KEEP_MS + 30_000, UsageKey::Cpu, 5);
        assert_eq!(names(&r), ["New"]);
    }

    #[test]
    fn a_clock_step_back_forgets_what_it_overlaps() {
        let mut ring = UsageRing::default();
        ring.push(T0 + 10_000, &[busy(1, "A", 5.0, 10.0)]);
        ring.push(T0 + 40_000, &[busy(2, "B", 5.0, 10.0)]);
        ring.drop_after(T0 + 20_000);
        let r = ring.by_app(T0 - 60_000, T0 + 60_000, UsageKey::Cpu, 5);
        assert_eq!(names(&r), ["A"]);
        // The batch after the step lands normally.
        ring.push(T0 + 30_000, &[busy(3, "C", 5.0, 10.0)]);
        assert_eq!(ring.by_app(T0, T0 + 30_000, UsageKey::Cpu, 5).apps.len(), 2);
    }

    #[test]
    fn a_clock_step_back_keeps_a_process_that_spans_it() {
        let mut ring = UsageRing::default();
        ring.push(T0 + 10_000, &[proc(1, "a", Some("A"), true, 1.0)]);
        ring.push(T0 + 40_000, &[proc(1, "a", Some("A"), true, 2.0)]);
        ring.drop_after(T0 + 20_000);
        let r = energy(&ring, T0 - 60_000, T0 + 60_000);
        assert!(
            (r.total.energy_j - 1.0).abs() < 1e-6,
            "the bucket before the step still counts"
        );
        assert_eq!(r.apps.len(), 1);
        assert!(r.apps[0].processes[0].running, "it was in the newest batch");
        // The same process after the step adds to the same entry.
        ring.push(T0 + 30_000, &[proc(1, "a", Some("A"), true, 1.0)]);
        let r = energy(&ring, T0 - 60_000, T0 + 60_000);
        assert_eq!(r.apps[0].processes.len(), 1);
        assert!((r.total.energy_j - 2.0).abs() < 1e-6);
    }

    #[test]
    fn the_floor_is_on_rates_whatever_the_cadence() {
        let mut ring = UsageRing::default();
        // 50 µW: kept at 1 s as at 10 s.
        let mut p = proc(1, "a", Some("A"), true, 50e-6);
        p.interval_s = 1.0;
        ring.push(T0 + 1_000, &[p.clone()]);
        let mut idle = proc(2, "b", Some("B"), true, 5e-6);
        idle.interval_s = 1.0;
        ring.push(T0 + 2_000, &[p, idle]);
        let r = energy(&ring, T0, T0 + 10_000);
        assert_eq!(names(&r), ["A"]);
    }

    #[test]
    fn nothing_measured_reads_empty() {
        let ring = UsageRing::default();
        let r = energy(&ring, T0, T0 + 60_000);
        assert_eq!(r.since_ms, None);
        assert_eq!(r.covered_ms, 0);
        assert!(r.apps.is_empty());
    }

    #[test]
    fn a_full_hour_stays_small() {
        // 400 processes in 120 apps, 60 of them over the memory floor, every second.
        let mut ring = UsageRing::default();
        let rows: Vec<ProcessSample> = (0..400)
            .map(|pid| {
                let app = format!("app{}", pid % 120);
                let mut p = busy(pid, &app, if pid % 10 == 0 { 2.0 } else { 0.0 }, 1.0);
                p.mem_bytes = if pid < 60 { 40 * MIB } else { 4 * MIB };
                p
            })
            .collect();
        for s in 1..=3_600 {
            ring.push(T0 + s * 1_000, &rows);
        }
        let bytes = ring.bucket_bytes();
        eprintln!(
            "[usage] full hour: {} buckets, {bytes} bytes",
            ring.buckets.len()
        );
        assert!(bytes < 3 << 20, "{bytes} bytes");
    }
}

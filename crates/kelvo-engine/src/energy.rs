//! The last hour of per-process energy, for "what used the battery over this window"
//! (D-093).
//!
//! Every process batch adds each row's [`ProcessSample::energy_j`] (joules over the
//! interval since the previous sample) to the 10 s bucket the batch's timestamp falls in.
//! A range read sums the buckets that start inside it, so a sample's whole interval is
//! charged to the bucket it ended in: the edges of a range are off by at most one sample
//! interval (10 s tray-only, 30 s in Performance mode, a tick while a process view
//! shows). Summing per sample,
//! never rate times window, keeps the totals right whatever the cadence was.
//!
//! In memory only, kept for [`ENERGY_KEEP_MS`]: the longest chart window is an hour
//! (D-091), so nothing older is ever asked for. It starts empty when the app starts and
//! says so through [`EnergyByApp::since_ms`].
//!
//! Each bucket holds `(id, joules)` pairs; process details live once in a table keyed by
//! id and are dropped when the last bucket naming them leaves the hour. Rows below
//! [`MIN_WATTS`] are not kept: idle daemons report microwatts, and keeping them would
//! multiply the ring's size for nothing a table could show. The floor is on power, not
//! energy, so it means the same at every cadence.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use kelvo_collect::ProcessSample;

/// Width of an energy bucket.
pub const ENERGY_BUCKET_MS: i64 = 10_000;

/// How far back the ring reaches: the longest chart window plus one bucket, so an
/// hour-long window still finds its first bucket.
pub const ENERGY_KEEP_MS: i64 = 3_600_000 + ENERGY_BUCKET_MS;

/// Smallest sample kept: a process averaging under 10 µW over its interval.
const MIN_WATTS: f32 = 1e-5;

/// One process as the ring knows it.
#[derive(Clone, Debug)]
struct Proc {
    pid: i32,
    start_time_us: i64,
    name: Arc<str>,
    app: Option<Arc<str>>,
    app_main: bool,
    /// Timestamp of the newest batch that listed it.
    seen_ms: i64,
    /// Start of the newest bucket that holds its energy.
    last_bucket: i64,
}

/// One process's energy over a range.
#[derive(Clone, Debug, PartialEq)]
pub struct EnergyProc {
    pub pid: i32,
    pub start_time_us: i64,
    pub name: Arc<str>,
    pub joules: f64,
    /// Average power over the range's measured span, watts.
    pub avg_w: f64,
    /// Listed by the newest batch: still running as of then.
    pub running: bool,
    /// Its app bundle's main executable.
    pub app_main: bool,
}

/// One app's energy over a range: its processes', summed.
#[derive(Clone, Debug, PartialEq)]
pub struct EnergyApp {
    /// The app identity (D-089), or the process name when none was resolved.
    pub name: Arc<str>,
    pub joules: f64,
    pub avg_w: f64,
    /// Largest first.
    pub processes: Vec<EnergyProc>,
}

/// Per-app energy over a range ([`EnergyRing::by_app`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnergyByApp {
    /// The range the sums cover: the request widened to whole buckets.
    pub from_ms: i64,
    pub to_ms: i64,
    /// When the ring started counting (the first batch it saw), `None` before any.
    /// After `from_ms`, the range was only measured from here.
    pub since_ms: Option<i64>,
    /// How much of the range was measured: from the later of `from_ms` and `since_ms`
    /// to the earlier of `to_ms` and the newest batch. The averages divide by this.
    pub measured_ms: i64,
    /// Every app's joules, summed.
    pub total_j: f64,
    /// Largest first.
    pub apps: Vec<EnergyApp>,
}

#[derive(Default)]
pub struct EnergyRing {
    /// `(bucket start, (proc id, joules))`, oldest first.
    buckets: VecDeque<(i64, Vec<(u32, f32)>)>,
    procs: HashMap<u32, Proc>,
    ids: HashMap<(i32, i64), u32>,
    next_id: u32,
    /// Where each id sits in the newest bucket, so a batch merges in one lookup per row.
    slot_index: HashMap<u32, usize>,
    /// A pruned bucket's storage, for the next bucket.
    spare: Vec<(u32, f32)>,
    since_ms: Option<i64>,
    latest_ms: Option<i64>,
}

fn bucket_of(ts_ms: i64) -> i64 {
    ts_ms - ts_ms.rem_euclid(ENERGY_BUCKET_MS)
}

impl EnergyRing {
    /// Adds one process batch taken at `ts_ms`.
    pub fn push(&mut self, ts_ms: i64, rows: &[ProcessSample]) {
        if self.latest_ms.is_some_and(|l| ts_ms < l) {
            // An older batch only arrives after the clock stepped back; the engine drops
            // what it overlaps first, so this is a batch from before that.
            return;
        }
        self.since_ms.get_or_insert(ts_ms);
        self.latest_ms = Some(ts_ms);
        self.prune(ts_ms - ENERGY_KEEP_MS);
        let bucket = bucket_of(ts_ms);
        if self.buckets.back().is_none_or(|(b, _)| *b != bucket) {
            // Sized like the last bucket, reusing a pruned one's allocation, so a bucket
            // costs at most one allocation and none once the hour is full.
            let want = self.buckets.back().map_or(0, |(_, s)| s.len());
            let mut slot = std::mem::take(&mut self.spare);
            slot.reserve(want);
            self.buckets.push_back((bucket, slot));
            self.slot_index.clear();
        }
        let Self {
            buckets,
            procs,
            ids,
            next_id,
            slot_index,
            ..
        } = self;
        let Some((_, slot)) = buckets.back_mut() else {
            return;
        };
        for r in rows {
            let key = (r.pid, r.start_time_us);
            let id = ids.get(&key).copied();
            if r.energy < MIN_WATTS {
                // Not worth a slot, but a process the ring holds is still running.
                if let Some(p) = id.and_then(|id| procs.get_mut(&id)) {
                    p.seen_ms = ts_ms;
                }
                continue;
            }
            let id = id.unwrap_or_else(|| {
                let id = *next_id;
                *next_id = next_id.wrapping_add(1);
                ids.insert(key, id);
                procs.insert(
                    id,
                    Proc {
                        pid: r.pid,
                        start_time_us: r.start_time_us,
                        name: Arc::clone(&r.name),
                        app: r.app.clone(),
                        app_main: r.app_main,
                        seen_ms: ts_ms,
                        last_bucket: bucket,
                    },
                );
                id
            });
            if let Some(p) = procs.get_mut(&id) {
                p.seen_ms = ts_ms;
                p.last_bucket = bucket;
            }
            // One batch per bucket tray-only; while a view shows processes, ten. Merge
            // so a process holds one pair per bucket.
            match slot_index.get(&id) {
                Some(&i) => slot[i].1 += r.energy_j,
                None => {
                    slot_index.insert(id, slot.len());
                    slot.push((id, r.energy_j));
                }
            }
        }
    }

    /// Drops buckets starting before `cutoff_ms` and the processes only they named.
    fn prune(&mut self, cutoff_ms: i64) {
        let mut dropped = false;
        while self.buckets.front().is_some_and(|(b, _)| *b < cutoff_ms) {
            if let Some((_, mut slot)) = self.buckets.pop_front() {
                slot.clear();
                self.spare = slot;
            }
            dropped = true;
        }
        if !dropped {
            return;
        }
        let oldest = self.buckets.front().map_or(i64::MAX, |(b, _)| *b);
        let ids = &mut self.ids;
        self.procs.retain(|_, p| {
            let keep = p.last_bucket >= oldest;
            if !keep {
                ids.remove(&(p.pid, p.start_time_us));
            }
            keep
        });
        // Nothing before the cutoff is held any more.
        if let Some(s) = self.since_ms.as_mut() {
            *s = (*s).max(cutoff_ms);
        }
    }

    /// Forgets buckets ending after `ts_ms`: the wall clock stepped back.
    pub fn drop_after(&mut self, ts_ms: i64) {
        while self
            .buckets
            .back()
            .is_some_and(|(b, _)| b + ENERGY_BUCKET_MS > ts_ms)
        {
            self.buckets.pop_back();
            self.slot_index.clear();
        }
        // A process can hold energy on both sides of the step: its newest bucket is
        // now the newest one left that names it.
        let mut last: HashMap<u32, i64> = HashMap::new();
        for (b, slot) in &self.buckets {
            for &(id, _) in slot {
                last.insert(id, *b);
            }
        }
        // The newest batch kept is no later than the end of the newest bucket left.
        let latest = self
            .latest_ms
            .zip(self.buckets.back())
            .map(|(l, (b, _))| l.min(b + ENERGY_BUCKET_MS - 1));
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
        if self.buckets.is_empty() {
            self.since_ms = None;
            self.latest_ms = None;
        } else {
            self.latest_ms = latest;
        }
    }

    /// Energy per app over `[from_ms, to_ms)`, widened to whole buckets. A process with
    /// no resolved app forms its own group under its name.
    pub fn by_app(&self, from_ms: i64, to_ms: i64) -> EnergyByApp {
        let from = bucket_of(from_ms);
        let to = if to_ms.rem_euclid(ENERGY_BUCKET_MS) == 0 {
            to_ms
        } else {
            bucket_of(to_ms) + ENERGY_BUCKET_MS
        };
        let mut per_proc: HashMap<u32, f64> = HashMap::new();
        for (b, slot) in &self.buckets {
            if *b < from || *b >= to {
                continue;
            }
            for &(id, j) in slot {
                *per_proc.entry(id).or_default() += f64::from(j);
            }
        }
        let measured_ms = match (self.since_ms, self.latest_ms) {
            (Some(s), Some(l)) => (l.min(to) - s.max(from)).max(0),
            _ => 0,
        };
        let secs = measured_ms as f64 / 1_000.0;
        let avg = |j: f64| if secs > 0.0 { j / secs } else { 0.0 };
        let latest = self.latest_ms;

        let mut apps: HashMap<Arc<str>, EnergyApp> = HashMap::new();
        let mut total_j = 0.0;
        for (id, joules) in per_proc {
            let Some(p) = self.procs.get(&id) else {
                continue;
            };
            total_j += joules;
            let name = p.app.clone().unwrap_or_else(|| Arc::clone(&p.name));
            let app = apps.entry(Arc::clone(&name)).or_insert_with(|| EnergyApp {
                name,
                joules: 0.0,
                avg_w: 0.0,
                processes: Vec::new(),
            });
            app.joules += joules;
            app.processes.push(EnergyProc {
                pid: p.pid,
                start_time_us: p.start_time_us,
                name: Arc::clone(&p.name),
                joules,
                avg_w: avg(joules),
                running: Some(p.seen_ms) == latest,
                app_main: p.app_main,
            });
        }
        let mut apps: Vec<EnergyApp> = apps
            .into_values()
            .map(|mut a| {
                a.avg_w = avg(a.joules);
                a.processes
                    .sort_by(|x, y| y.joules.total_cmp(&x.joules).then(x.pid.cmp(&y.pid)));
                a
            })
            .collect();
        apps.sort_by(|x, y| y.joules.total_cmp(&x.joules).then(x.name.cmp(&y.name)));
        EnergyByApp {
            from_ms: from,
            to_ms: to,
            since_ms: self.since_ms,
            measured_ms,
            total_j,
            apps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            app: app.map(Into::into),
            app_main: main,
            disk_read_bps: 0.0,
            disk_write_bps: 0.0,
            net_rx_bps: None,
            net_tx_bps: None,
            gpu_pct: None,
            user: "me".into(),
        }
    }

    const T0: i64 = 1_700_000_000_000;

    #[test]
    fn groups_processes_by_app_and_sums_samples() {
        let mut ring = EnergyRing::default();
        for i in 0..6 {
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
        let r = ring.by_app(T0, T0 + 60_000);
        assert_eq!(r.from_ms, T0);
        assert_eq!(r.to_ms, T0 + 60_000);
        let names: Vec<&str> = r.apps.iter().map(|a| a.name.as_ref()).collect();
        // The idle process is below the floor and does not appear.
        assert_eq!(names, ["Google Chrome", "node"]);
        let chrome = &r.apps[0];
        assert!((chrome.joules - 180.0).abs() < 1e-6);
        assert_eq!(chrome.processes[0].pid, 2);
        assert!(chrome.processes.iter().all(|p| p.running));
        assert!(chrome.processes[1].app_main);
        assert!((r.total_j - 210.0).abs() < 1e-6);
        // Measured from the first batch to the newest: 50 s.
        assert_eq!(r.measured_ms, 50_000);
        assert!((chrome.avg_w - 180.0 / 50.0).abs() < 1e-9);
    }

    #[test]
    fn a_range_sums_only_its_buckets_and_flags_exited_processes() {
        let mut ring = EnergyRing::default();
        ring.push(T0, &[proc(1, "a", Some("A"), false, 1.0)]);
        ring.push(T0 + 10_000, &[proc(1, "a", Some("A"), false, 2.0)]);
        ring.push(T0 + 20_000, &[proc(2, "b", Some("B"), false, 4.0)]);
        let r = ring.by_app(T0 + 10_000, T0 + 30_000);
        assert_eq!(r.apps.len(), 2);
        assert!((r.apps[0].joules - 4.0).abs() < 1e-6);
        let a = &r.apps[1];
        assert!((a.joules - 2.0).abs() < 1e-6);
        // Process 1 was not in the newest batch: it exited.
        assert!(!a.processes[0].running);
        assert!(r.apps[0].processes[0].running);
    }

    #[test]
    fn buckets_older_than_an_hour_go_with_their_processes() {
        let mut ring = EnergyRing::default();
        ring.push(T0, &[proc(1, "old", Some("Old"), false, 1.0)]);
        ring.push(
            T0 + ENERGY_KEEP_MS + 20_000,
            &[proc(2, "new", Some("New"), false, 1.0)],
        );
        assert!(!ring.ids.contains_key(&(1, 1_000)));
        assert_eq!(ring.procs.len(), 1);
        let r = ring.by_app(T0, T0 + ENERGY_KEEP_MS + 30_000);
        let names: Vec<&str> = r.apps.iter().map(|a| a.name.as_ref()).collect();
        assert_eq!(names, ["New"]);
    }

    #[test]
    fn a_clock_step_back_forgets_what_it_overlaps() {
        let mut ring = EnergyRing::default();
        ring.push(T0, &[proc(1, "a", Some("A"), false, 1.0)]);
        ring.push(T0 + 30_000, &[proc(2, "b", Some("B"), false, 1.0)]);
        ring.drop_after(T0 + 15_000);
        let r = ring.by_app(T0 - 60_000, T0 + 60_000);
        let names: Vec<&str> = r.apps.iter().map(|a| a.name.as_ref()).collect();
        assert_eq!(names, ["A"]);
        // The batch after the step lands normally.
        ring.push(T0 + 12_000, &[proc(3, "c", Some("C"), false, 1.0)]);
        assert_eq!(ring.by_app(T0, T0 + 20_000).apps.len(), 2);
    }

    #[test]
    fn a_clock_step_back_keeps_a_process_that_spans_it() {
        let mut ring = EnergyRing::default();
        ring.push(T0, &[proc(1, "a", Some("A"), true, 1.0)]);
        ring.push(T0 + 30_000, &[proc(1, "a", Some("A"), true, 2.0)]);
        ring.drop_after(T0 + 15_000);
        let r = ring.by_app(T0 - 60_000, T0 + 60_000);
        assert_eq!(r.total_j, 1.0, "the bucket before the step still counts");
        assert_eq!(r.apps.len(), 1);
        assert!(r.apps[0].processes[0].running, "it was in the newest batch");
        // The same process after the step adds to the same entry.
        ring.push(T0 + 16_000, &[proc(1, "a", Some("A"), true, 1.0)]);
        let r = ring.by_app(T0 - 60_000, T0 + 60_000);
        assert_eq!(r.apps[0].processes.len(), 1);
        assert_eq!(r.total_j, 2.0);
    }

    #[test]
    fn the_floor_is_on_power_whatever_the_cadence() {
        let mut ring = EnergyRing::default();
        // 50 µW: kept at 1 s (50 µJ) as at 10 s.
        let mut p = proc(1, "a", Some("A"), true, 50e-6);
        p.energy = 50e-6;
        ring.push(T0, &[p.clone()]);
        let mut idle = proc(2, "b", Some("B"), true, 5e-6);
        idle.energy = 5e-6;
        ring.push(T0 + 1_000, &[p, idle]);
        let names: Vec<_> = ring
            .by_app(T0, T0 + 10_000)
            .apps
            .into_iter()
            .map(|a| a.name)
            .collect();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].as_ref(), "A");
    }

    #[test]
    fn nothing_measured_reads_empty() {
        let ring = EnergyRing::default();
        let r = ring.by_app(T0, T0 + 60_000);
        assert_eq!(r.since_ms, None);
        assert_eq!(r.measured_ms, 0);
        assert!(r.apps.is_empty());
    }
}

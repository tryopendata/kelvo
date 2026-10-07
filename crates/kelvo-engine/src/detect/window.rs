//! The last minute of process batches, cut to what attribution needs: each batch's top
//! few processes by CPU and by energy. Fed by whatever batches the processes collector
//! produces (every 10 s with no window open, every tick with the process table open), so
//! attribution never asks for process sampling of its own (D-083).

use std::collections::VecDeque;
use std::sync::Arc;

use kelvo_collect::ProcessSample;

/// Processes kept per batch and measure.
const TOP: usize = 5;
/// Batches kept: a minute at the fastest interval (0.5 s) with room to spare. Fixed so
/// pushing never reallocates.
const MAX_BATCHES: usize = 256;
/// The weight of a batch is the time since the one before, capped at the idle cadence
/// of the processes collector: 10 s, or 30 s in Performance mode (D-088, set with
/// [`ProcessWindow::set_idle_ms`]). A batch after a long hole counts as one idle period,
/// not the whole hole.
const DEFAULT_MAX_WEIGHT_MS: i64 = 10_000;

type Top = [Option<(Arc<str>, f32)>; TOP];

struct Batch {
    ts_ms: i64,
    weight_ms: i64,
    cpu: Top,
    energy: Top,
}

/// Which measure to rank by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measure {
    Cpu,
    Energy,
}

pub struct ProcessWindow {
    batches: VecDeque<Batch>,
    span_ms: i64,
    max_weight_ms: i64,
}

impl ProcessWindow {
    /// Keeps batches for `span_ms` before the newest.
    pub fn new(span_ms: i64) -> Self {
        Self {
            batches: VecDeque::with_capacity(MAX_BATCHES),
            span_ms,
            max_weight_ms: DEFAULT_MAX_WEIGHT_MS,
        }
    }

    /// The processes collector's idle period, which caps a batch's weight.
    pub fn set_idle_ms(&mut self, idle_ms: u32) {
        self.max_weight_ms = i64::from(idle_ms);
    }

    /// Adds a batch taken at `ts_ms`. Allocation-free: names are shared with the
    /// collector's cache and the ring is preallocated.
    pub fn push(&mut self, ts_ms: i64, rows: &[ProcessSample]) {
        let horizon = ts_ms.saturating_sub(self.span_ms);
        while self
            .batches
            .front()
            .is_some_and(|b| b.ts_ms < horizon || b.ts_ms > ts_ms)
            || self.batches.len() >= MAX_BATCHES
        {
            self.batches.pop_front();
        }
        let weight_ms = self.batches.back().map_or(self.max_weight_ms, |b| {
            (ts_ms - b.ts_ms).clamp(1, self.max_weight_ms)
        });
        let mut batch = Batch {
            ts_ms,
            weight_ms,
            cpu: std::array::from_fn(|_| None),
            energy: std::array::from_fn(|_| None),
        };
        for p in rows {
            insert(&mut batch.cpu, &p.name, p.cpu_pct);
            insert(&mut batch.energy, &p.name, p.energy);
        }
        self.batches.push_back(batch);
    }

    pub fn clear(&mut self) {
        self.batches.clear();
    }

    /// The names with the highest mean `measure` over batches at or after `from_ms`,
    /// highest first: at most `n`, each with a mean of at least `min_mean`. A process
    /// outside a batch's top few counts as zero in that batch. With no batch since
    /// `from_ms` (batches 30 s apart in Performance mode, D-088) the newest batch stands
    /// in, if it is no older than one idle period before `from_ms`. Allocates; called
    /// only when an event fires.
    pub fn top(&self, measure: Measure, from_ms: i64, n: usize, min_mean: f32) -> Vec<String> {
        let mut total_ms = 0i64;
        let mut sums: Vec<(&str, f64)> = Vec::new();
        let since = self.batches.iter().filter(|b| b.ts_ms >= from_ms).count();
        let fallback = self
            .batches
            .back()
            .is_some_and(|b| b.ts_ms >= from_ms - self.max_weight_ms);
        let take = match since {
            0 if fallback => 1,
            0 => 0,
            n => n,
        };
        for b in self.batches.iter().rev().take(take) {
            total_ms += b.weight_ms;
            let top = match measure {
                Measure::Cpu => &b.cpu,
                Measure::Energy => &b.energy,
            };
            for (name, v) in top.iter().flatten() {
                let add = f64::from(*v) * b.weight_ms as f64;
                match sums.iter_mut().find(|(n, _)| *n == &**name) {
                    Some((_, s)) => *s += add,
                    None => sums.push((name, add)),
                }
            }
        }
        if total_ms == 0 {
            return Vec::new();
        }
        let total = total_ms as f64;
        sums.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        sums.into_iter()
            .filter(|(_, s)| s / total >= f64::from(min_mean))
            .take(n)
            .map(|(name, _)| name.to_owned())
            .collect()
    }
}

/// Puts `(name, v)` into `top`, kept in descending order, if it ranks. Zero, negative
/// and NaN values never rank.
fn insert(top: &mut Top, name: &Arc<str>, v: f32) {
    if v.is_nan() || v <= 0.0 {
        return;
    }
    let Some(pos) = top
        .iter()
        .position(|e| e.as_ref().is_none_or(|(_, x)| v > *x))
    else {
        return;
    };
    if let Some(tail) = top.get_mut(pos..) {
        tail.rotate_right(1);
        if let Some(first) = tail.first_mut() {
            *first = Some((Arc::clone(name), v));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn proc(name: &str, pid: i32, cpu: f32, energy: f32) -> ProcessSample {
        ProcessSample {
            pid,
            start_time_us: 1,
            name: name.into(),
            cpu_pct: cpu,
            mem_bytes: 0,
            compressed_bytes: None,
            threads: 1,
            idle_wakeups_per_s: 0.0,
            energy,
            energy_j: 0.0,
            interval_s: 1.0,
            app: None,
            app_main: false,
            disk_read_bps: 0.0,
            disk_write_bps: 0.0,
            net_rx_bps: None,
            net_tx_bps: None,
            gpu_pct: None,
            ports: None,
            user: "u".into(),
        }
    }

    #[test]
    fn ranks_by_mean_over_the_window() {
        let mut w = ProcessWindow::new(60_000);
        // 0 s: Xcode busy; 10 s and 20 s: kernel_task busier on average.
        w.push(
            0,
            &[
                proc("Xcode", 1, 300.0, 10.0),
                proc("kernel_task", 0, 50.0, 1.0),
            ],
        );
        w.push(
            10_000,
            &[
                proc("kernel_task", 0, 400.0, 1.0),
                proc("Xcode", 1, 100.0, 50.0),
            ],
        );
        w.push(20_000, &[proc("kernel_task", 0, 400.0, 1.0)]);
        assert_eq!(w.top(Measure::Cpu, 0, 2, 20.0), ["kernel_task", "Xcode"]);
        assert_eq!(w.top(Measure::Energy, 0, 1, 0.0), ["Xcode"]);
        // From 15 s only kernel_task ran.
        assert_eq!(w.top(Measure::Cpu, 15_000, 2, 20.0), ["kernel_task"]);
        // A minimum mean drops the light ones.
        assert_eq!(w.top(Measure::Cpu, 0, 2, 200.0), ["kernel_task"]);
    }

    #[test]
    fn keeps_only_the_span_and_the_top_few() {
        let mut w = ProcessWindow::new(60_000);
        let rows: Vec<ProcessSample> = (0..20)
            .map(|i| proc(&format!("p{i}"), i, i as f32, 0.0))
            .collect();
        w.push(0, &rows);
        assert_eq!(
            w.top(Measure::Cpu, 0, 10, 0.0),
            ["p19", "p18", "p17", "p16", "p15"]
        );
        w.push(61_000, &[proc("late", 1, 5.0, 0.0)]);
        assert_eq!(
            w.top(Measure::Cpu, 0, 10, 0.0),
            ["late"],
            "the first batch expired"
        );
        w.clear();
        assert!(w.top(Measure::Cpu, 0, 10, 0.0).is_empty());
    }
    /// Batches 30 s apart (Performance mode, D-088): a span with no batch of its own is
    /// attributed to the newest batch rather than to nothing.
    #[test]
    fn a_span_without_a_batch_falls_back_to_the_newest() {
        let mut w = ProcessWindow::new(60_000);
        w.set_idle_ms(30_000);
        w.push(0, &[proc("old", 1, 10.0, 1.0)]);
        w.push(30_000, &[proc("Xcode", 2, 250.0, 9.0)]);
        assert_eq!(w.top(Measure::Energy, 40_000, 1, 0.0), ["Xcode"]);
        assert_eq!(w.top(Measure::Cpu, 25_000, 2, 0.0), ["Xcode"]);
        // A batch older than one idle period before the span stands for nothing.
        assert!(w.top(Measure::Cpu, 70_000, 1, 0.0).is_empty());
    }
}

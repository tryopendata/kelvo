//! The four v1.2 detectors. Each keeps a few numbers of state per tick; none allocates
//! on a tick that fires nothing.

use std::collections::VecDeque;
use std::sync::Arc;

use kelvo_collect::ProcessSample;
use kelvo_schema::{
    DetectorThresholds, Event, EventDetail, PowerComponent, SeriesKey, ThermalState,
};

use super::window::Measure;
use super::{Detector, TickInput, find, value};

// ---- fans_ramped -------------------------------------------------------------------

/// Readings kept for the trailing minimum: a minute of `fan.rpm` at its fastest (every
/// 2 s, or every tick at 0.5 s) with room to spare.
const FAN_READINGS: usize = 256;

/// The fastest fan rose by `fan_rise_rpm` over its lowest reading in the last
/// `fan_window_ms`. Attributed to the top processes by CPU over the minute before.
///
/// After a ramp the detector re-arms when the fans have come back down (within half a
/// rise of where that ramp started) and the refractory period is over, or, for fans
/// that settle high and stay there, once `fan_rearm_after_ms` has passed since the
/// ramp; either way it measures the next ramp from that moment, so from where the fans
/// are then. So one long ramp, or fans that stay up, is one event per
/// `fan_rearm_after_ms` at most, while back-to-back builds that let the fans settle in
/// between are one each; and a ramp is never reported late against a minimum from
/// before the re-arm. Readings under `fan_floor_rpm` (a stopped fan) are skipped: a fan
/// starting up is not a ramp.
pub struct FansRamped {
    th: DetectorThresholds,
    fans: Vec<usize>,
    /// Monotonic deque: increasing rpm from front to back, so the front is the window's
    /// minimum.
    mins: VecDeque<(i64, f32)>,
    quiet_until: i64,
    /// Set after a ramp: the rpm the fans must fall to before the next one counts.
    rearm_at: Option<f32>,
    /// Set after a ramp: when the detector re-arms wherever the fans are.
    rearm_by: i64,
}

impl FansRamped {
    pub fn new(th: DetectorThresholds) -> Self {
        Self {
            th,
            fans: Vec::new(),
            mins: VecDeque::with_capacity(FAN_READINGS),
            quiet_until: i64::MIN,
            rearm_at: None,
            rearm_by: i64::MIN,
        }
    }
}

impl Detector for FansRamped {
    fn bind(&mut self, series: &[SeriesKey]) {
        self.fans.clear();
        self.fans.extend(
            series
                .iter()
                .enumerate()
                .filter(|(_, k)| k.metric.as_str() == "fan.rpm")
                .map(|(i, _)| i),
        );
        self.reset();
    }

    fn on_tick(&mut self, input: &TickInput<'_>, out: &mut Vec<Event>) {
        let ts = input.ts_ms;
        let Some(rpm) = self
            .fans
            .iter()
            .filter_map(|&i| value(input.values, i))
            .reduce(f32::max)
        else {
            return;
        };
        if rpm < self.th.fan_floor_rpm {
            return;
        }
        if let Some(level) = self.rearm_at {
            let settled = ts >= self.quiet_until && rpm <= level;
            if !settled && ts < self.rearm_by {
                return;
            }
            // Nothing was kept while disarmed: the next ramp is measured from here.
            self.rearm_at = None;
        }
        while self.mins.back().is_some_and(|&(_, v)| v >= rpm) || self.mins.len() >= FAN_READINGS {
            self.mins.pop_back();
        }
        self.mins.push_back((ts, rpm));
        let horizon = ts - self.th.fan_window_ms;
        while self.mins.front().is_some_and(|&(t, _)| t < horizon) {
            self.mins.pop_front();
        }
        let Some(&(from_ts, from_rpm)) = self.mins.front() else {
            return;
        };
        if rpm - from_rpm < self.th.fan_rise_rpm {
            return;
        }
        out.push(Event {
            ts_ms: ts,
            start_ms: from_ts,
            processes: input.recent.top(
                Measure::Cpu,
                ts - self.th.attribution_window_ms,
                self.th.attribution_top,
                self.th.attribution_min_cpu_pct,
            ),
            detail: EventDetail::FansRamped {
                from_rpm,
                to_rpm: rpm,
            },
        });
        self.quiet_until = ts + self.th.fan_refractory_ms;
        self.rearm_at = Some(from_rpm + self.th.fan_rise_rpm / 2.0);
        self.rearm_by = ts + self.th.fan_rearm_after_ms;
        self.mins.clear();
    }

    fn reset(&mut self) {
        self.mins.clear();
        self.quiet_until = i64::MIN;
        self.rearm_at = None;
        self.rearm_by = i64::MIN;
    }
}

// ---- thermal_state -----------------------------------------------------------------

/// `thermal.state` moved to another level and held it for `thermal_hold_ms`. The first
/// reading after a reset sets the level without an event.
pub struct ThermalChange {
    hold_ms: i64,
    idx: Option<usize>,
    level: Option<ThermalState>,
    pending: Option<(ThermalState, i64)>,
}

impl ThermalChange {
    pub fn new(th: DetectorThresholds) -> Self {
        Self {
            hold_ms: th.thermal_hold_ms,
            idx: None,
            level: None,
            pending: None,
        }
    }
}

impl Detector for ThermalChange {
    fn bind(&mut self, series: &[SeriesKey]) {
        self.idx = find(series, "thermal.state");
        self.reset();
    }

    fn on_tick(&mut self, input: &TickInput<'_>, out: &mut Vec<Event>) {
        let Some(state) = self
            .idx
            .and_then(|i| value(input.values, i))
            .and_then(ThermalState::from_value)
        else {
            return;
        };
        let Some(level) = self.level else {
            self.level = Some(state);
            return;
        };
        if state == level {
            self.pending = None;
            return;
        }
        let since = match self.pending {
            Some((s, since)) if s == state => since,
            _ => {
                self.pending = Some((state, input.ts_ms));
                input.ts_ms
            }
        };
        if input.ts_ms - since >= self.hold_ms {
            out.push(Event {
                ts_ms: input.ts_ms,
                start_ms: since,
                processes: Vec::new(),
                detail: EventDetail::ThermalState {
                    from: Some(level),
                    to: state,
                },
            });
            self.level = Some(state);
            self.pending = None;
        }
    }

    fn reset(&mut self) {
        self.level = None;
        self.pending = None;
    }
}

// ---- sustained runs (sustained_process and the hot-process alert) ------------------

/// Runs kept without reallocating; more processes than this above the threshold at once
/// just grow the vector.
const RUNS: usize = 16;

/// One process's run above the threshold.
pub struct Run {
    pid: i32,
    start_time_us: i64,
    pub name: Arc<str>,
    pub since: i64,
    last_ts: i64,
    cpu_ms: f64,
    weight_ms: f64,
    fired: bool,
    seen: bool,
}

impl Run {
    /// Mean CPU over the run, weighted by time between batches.
    pub fn mean_cpu(&self) -> f32 {
        if self.weight_ms > 0.0 {
            (self.cpu_ms / self.weight_ms) as f32
        } else {
            0.0
        }
    }
}

/// Tracks processes at or above `above_pct` from process batches. A run starts at the
/// first batch at or above it and is reported once it has lasted `for_ms`; before that a
/// batch below `above_pct` ends it, after that only one below `release_pct` does, so a
/// process hovering at the threshold is reported once, not every few minutes. A
/// process missing from a batch (exited) ends its run.
pub struct Runs {
    above_pct: f32,
    release_pct: f32,
    for_ms: i64,
    runs: Vec<Run>,
}

impl Runs {
    pub fn new(above_pct: f32, release_pct: f32, for_ms: i64) -> Self {
        Self {
            above_pct,
            release_pct,
            for_ms,
            runs: Vec::with_capacity(RUNS),
        }
    }

    /// Updates the runs from a batch at `ts_ms` and calls `fire` for each run that just
    /// reached `for_ms`.
    pub fn on_batch(&mut self, ts_ms: i64, rows: &[ProcessSample], mut fire: impl FnMut(&Run)) {
        for r in &mut self.runs {
            r.seen = false;
        }
        for p in rows {
            if p.cpu_pct.is_nan() || p.cpu_pct < self.release_pct.min(self.above_pct) {
                continue;
            }
            match self
                .runs
                .iter_mut()
                .find(|r| r.pid == p.pid && r.start_time_us == p.start_time_us)
            {
                Some(r) => {
                    let keep = if r.fired {
                        self.release_pct
                    } else {
                        self.above_pct
                    };
                    if p.cpu_pct >= keep {
                        let w = (ts_ms - r.last_ts).max(0) as f64;
                        r.cpu_ms += f64::from(p.cpu_pct) * w;
                        r.weight_ms += w;
                        r.last_ts = ts_ms;
                        r.seen = true;
                    }
                }
                None if p.cpu_pct >= self.above_pct => self.runs.push(Run {
                    pid: p.pid,
                    start_time_us: p.start_time_us,
                    name: Arc::clone(&p.name),
                    since: ts_ms,
                    last_ts: ts_ms,
                    cpu_ms: 0.0,
                    weight_ms: 0.0,
                    fired: false,
                    seen: true,
                }),
                None => {}
            }
        }
        self.runs.retain(|r| r.seen);
        for r in &mut self.runs {
            if !r.fired && ts_ms - r.since >= self.for_ms {
                r.fired = true;
                fire(r);
            }
        }
    }

    pub fn clear(&mut self) {
        self.runs.clear();
    }

    /// Names of the runs already reported.
    pub fn fired_names(&self) -> impl Iterator<Item = &Arc<str>> {
        self.runs.iter().filter(|r| r.fired).map(|r| &r.name)
    }

    /// Marks the current runs that `spent` accepts as already reported: they fire no
    /// more, and end only below `release_pct`.
    pub fn spend(&mut self, spent: impl Fn(&str) -> bool) {
        for r in &mut self.runs {
            if spent(&r.name) {
                r.fired = true;
            }
        }
    }
}

/// Names remembered for the per-name refractory period without reallocating.
const NAMES: usize = 16;

/// One process at or above `sustained_cpu_pct` for `sustained_ms`, at most one event per
/// process name per `sustained_name_refractory_ms`: a cargo build runs a dozen `rustc`
/// processes over 100% for minutes each, which is one pill, not a dozen.
pub struct SustainedProcess {
    runs: Runs,
    name_quiet_ms: i64,
    /// Names reported recently and when.
    reported: Vec<(Arc<str>, i64)>,
}

impl SustainedProcess {
    pub fn new(th: DetectorThresholds) -> Self {
        Self {
            runs: Runs::new(
                th.sustained_cpu_pct,
                th.sustained_release_pct,
                th.sustained_ms,
            ),
            name_quiet_ms: th.sustained_name_refractory_ms,
            reported: Vec::with_capacity(NAMES),
        }
    }
}

impl Detector for SustainedProcess {
    fn bind(&mut self, _series: &[SeriesKey]) {}

    fn on_tick(&mut self, input: &TickInput<'_>, out: &mut Vec<Event>) {
        let Some(rows) = input.batch else { return };
        let ts = input.ts_ms;
        let quiet = self.name_quiet_ms;
        // A clock that went back clears the memory instead of holding names off.
        self.reported.retain(|&(_, at)| at <= ts && ts - at < quiet);
        let reported = &mut self.reported;
        self.runs.on_batch(ts, rows, |r| {
            if reported.iter().any(|(n, _)| *n == r.name) {
                return;
            }
            reported.push((Arc::clone(&r.name), ts));
            out.push(Event {
                ts_ms: ts,
                start_ms: r.since,
                processes: vec![r.name.to_string()],
                detail: EventDetail::SustainedProcess {
                    process: r.name.to_string(),
                    cpu_pct: r.mean_cpu(),
                    secs: u32::try_from((ts - r.since) / 1000).unwrap_or(u32::MAX),
                },
            });
        });
    }

    fn reset(&mut self) {
        self.runs.clear();
        self.reported.clear();
    }
}

// ---- power_spike -------------------------------------------------------------------

/// A component's power at least `power_ratio` times its baseline and its minimum rise
/// above it, for `power_hold_ms`. The baseline is an exponential moving average, frozen
/// while power is above it by the threshold so a long spike does not become the new
/// normal. Attributed to the top process by energy over the spike.
///
/// Readings can be sparse (`power.package` is mostly gaps on macOS 27, D-043): the
/// baseline is warm only after `power_warmup_readings` readings as well as
/// `power_warmup_ms`, and a hole of more than `power_max_gap_periods` sampling periods
/// of the series (its collector's current period, not the base tick) between
/// readings ends any spike in progress and starts the warm-up again, so two readings
/// minutes apart never make a 10 s hold and one stale reading is never the baseline.
pub struct PowerSpike {
    th: DetectorThresholds,
    component: PowerComponent,
    idx: Option<usize>,
    baseline: Option<f32>,
    warm_from: i64,
    /// Readings the baseline was built from.
    readings: u32,
    last_ts: i64,
    /// The series' sampling period at the last reading.
    last_period: u32,
    above_since: Option<i64>,
    peak: f32,
    /// Reported and not yet back down.
    active: bool,
    quiet_until: i64,
}

impl PowerSpike {
    pub fn new(th: DetectorThresholds, component: PowerComponent) -> Self {
        Self {
            th,
            component,
            idx: None,
            baseline: None,
            warm_from: 0,
            readings: 0,
            last_ts: 0,
            last_period: 0,
            above_since: None,
            peak: 0.0,
            active: false,
            quiet_until: i64::MIN,
        }
    }
}

impl Detector for PowerSpike {
    fn bind(&mut self, series: &[SeriesKey]) {
        self.idx = find(series, self.component.metric());
        self.reset();
    }

    fn on_tick(&mut self, input: &TickInput<'_>, out: &mut Vec<Event>) {
        let ts = input.ts_ms;
        let Some(idx) = self.idx else { return };
        let Some(w) = value(input.values, idx) else {
            return;
        };
        // The spacing to expect is the series' own sampling period, which follows what
        // is visible (every 10 s tray-only, every tick with a window open). Across a
        // change, the longer of the period now and at the last reading: a window opening
        // must not read the 10 s since the last idle reading as a hole.
        let period = input.periods.get(idx).copied().unwrap_or(0);
        let expected = period.max(self.last_period);
        self.last_period = period;
        let max_gap = i64::from(expected) * i64::from(self.th.power_max_gap_periods);
        let gap = expected > 0 && ts - self.last_ts > max_gap;
        let base = match self.baseline {
            // Whatever happened in a hole between readings was not seen, and a baseline
            // from before it (one idle reading minutes ago, D-043) is not one to compare
            // with: warm up again from here, which also ends any hold in progress. A
            // reported spike keeps its baseline to tell when it ends.
            Some(base) if !gap || self.active => base,
            _ => {
                self.above_since = None;
                self.peak = 0.0;
                self.baseline = Some(w);
                self.warm_from = ts;
                self.readings = 1;
                self.last_ts = ts;
                return;
            }
        };
        let rise = self.th.power_min_rise_w(self.component);
        if self.active {
            if w < base + rise / 2.0 {
                self.active = false;
                self.above_since = None;
                self.peak = 0.0;
                self.quiet_until = ts + self.th.power_refractory_ms;
            } else {
                self.last_ts = ts;
                return;
            }
        }
        let spiking = w >= base * self.th.power_ratio && w - base >= rise;
        if spiking {
            let since = *self.above_since.get_or_insert(ts);
            self.peak = self.peak.max(w);
            let warm = ts - self.warm_from >= self.th.power_warmup_ms
                && self.readings >= self.th.power_warmup_readings;
            if warm && ts >= self.quiet_until && ts - since >= self.th.power_hold_ms {
                out.push(Event {
                    ts_ms: ts,
                    start_ms: since,
                    processes: input.recent.top(
                        Measure::Energy,
                        since - self.th.power_hold_ms,
                        1,
                        f32::MIN_POSITIVE,
                    ),
                    detail: EventDetail::PowerSpike {
                        component: self.component,
                        watts: self.peak,
                        baseline_watts: base,
                    },
                });
                self.active = true;
            }
            if warm {
                self.last_ts = ts;
                return;
            }
        } else {
            self.above_since = None;
            self.peak = 0.0;
        }
        let dt = (ts - self.last_ts).max(0) as f32;
        let alpha = 1.0 - (-dt / self.th.power_baseline_tau_ms as f32).exp();
        self.baseline = Some(base + alpha * (w - base));
        self.readings = self.readings.saturating_add(1);
        self.last_ts = ts;
    }

    fn reset(&mut self) {
        self.baseline = None;
        self.last_period = 0;
        self.above_since = None;
        self.peak = 0.0;
        self.active = false;
        self.quiet_until = i64::MIN;
    }
}

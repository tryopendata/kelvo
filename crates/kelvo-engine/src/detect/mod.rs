//! Event detectors and alert rules (v1.2, architecture.md "Event detectors"). They read
//! the same per-tick values the rollups do, plus process batches when the processes
//! collector produced one, and emit [`Event`]s that the engine stores and publishes.
//!
//! Cost: a detector's tick is a few comparisons over values already in hand. Nothing
//! allocates on a tick that fires nothing (the per-tick allocation gate covers this), and
//! attribution reads the batches the processes collector produces anyway, never asking
//! for more (D-083).

mod alerts;
mod detectors;
mod window;

use kelvo_collect::ProcessSample;
use kelvo_schema::{
    AlertSettings, DetectorThresholds, Event, EventDetail, PowerComponent, SeriesKey,
};

pub use alerts::AlertEval;
pub use detectors::{FansRamped, PowerSpike, SustainedProcess, ThermalChange};
pub use window::{Measure, ProcessWindow};

/// What a detector sees on one tick.
pub struct TickInput<'a> {
    pub ts_ms: i64,
    /// Each series' sampling period on this tick, in layout order (the larger of its
    /// catalog period, its collector's current period and the base tick), for detectors
    /// that must tell a missed reading from the normal spacing. A collector's period
    /// changes with what is visible: IOReport reads every 10 s tray-only, every tick with
    /// a window open.
    pub periods: &'a [u32],
    /// Values measured on this tick in layout order, `NaN` where a series was not sampled
    /// (`LiveFrame::values`). Never held values: a detector sees each reading once.
    pub values: &'a [f32],
    /// This tick's process rows, when the processes collector ran.
    pub batch: Option<&'a [ProcessSample]>,
    /// The last minute of process batches, for attribution.
    pub recent: &'a ProcessWindow,
}

pub trait Detector: Send {
    /// The layout changed: find the series this detector reads. Resets its state.
    fn bind(&mut self, series: &[SeriesKey]);
    /// Reads one tick; pushes any events to `out`.
    fn on_tick(&mut self, input: &TickInput<'_>, out: &mut Vec<Event>);
    /// Forgets what it was tracking (after sleep, a pause or a clock step), so a
    /// comparison never spans a hole.
    fn reset(&mut self);
}

/// The index of the unlabelled series `metric`.
fn find(series: &[SeriesKey], metric: &str) -> Option<usize> {
    series
        .iter()
        .position(|k| k.metric.as_str() == metric && k.labels.is_empty())
}

/// The value at `i` when it was measured this tick.
fn value(values: &[f32], i: usize) -> Option<f32> {
    values.get(i).copied().filter(|v| !v.is_nan())
}

/// How far back an alert rule's last firing still holds it off: the longest cooldown
/// of the built-in rules. The app shell reads stored alerts this far back at launch
/// ([`Detectors::seed_alert_history`]).
pub fn alert_history_ms() -> i64 {
    AlertSettings::default()
        .rules()
        .iter()
        .map(|r| i64::from(r.cooldown_secs) * 1000)
        .max()
        .unwrap_or(0)
}

/// Every detector plus the alert rules, fed by the engine once per persisted tick.
/// Every built-in rule has an evaluator, on or off, so a rule switched off and on again
/// keeps its cooldown.
pub struct Detectors {
    detectors: Vec<Box<dyn Detector>>,
    alerts: Vec<AlertEval>,
    window: ProcessWindow,
    series: Vec<SeriesKey>,
}

impl Detectors {
    pub fn new(th: DetectorThresholds, alerts: AlertSettings) -> Self {
        let mut d = Self {
            detectors: vec![
                Box::new(FansRamped::new(th)),
                Box::new(ThermalChange::new(th)),
                Box::new(SustainedProcess::new(th)),
                Box::new(PowerSpike::new(th, PowerComponent::Package)),
                Box::new(PowerSpike::new(th, PowerComponent::Ane)),
            ],
            alerts: Vec::new(),
            window: ProcessWindow::new(th.attribution_window_ms),
            series: Vec::new(),
        };
        d.set_alerts(alerts);
        d
    }

    /// Binds every detector to a new layout.
    pub fn bind(&mut self, series: &[SeriesKey]) {
        self.series.clear();
        self.series.extend_from_slice(series);
        for d in &mut self.detectors {
            d.bind(series);
        }
        for a in &mut self.alerts {
            a.bind(series);
        }
        self.window.clear();
    }

    /// The processes collector's idle period (10 s, or 30 s in Performance mode), which
    /// caps how much time one batch stands for in attribution.
    pub fn set_process_idle_ms(&mut self, idle_ms: u32) {
        self.window.set_idle_ms(idle_ms);
    }

    /// Switches the built-in alert rules. A rule that stays on keeps its progress; one
    /// switched off forgets its progress (it sees nothing while off) but keeps its
    /// cooldown, so switching it back on cannot re-fire it at once.
    pub fn set_alerts(&mut self, settings: AlertSettings) {
        for rule in settings.rules() {
            match self.alerts.iter_mut().find(|a| a.rule().id == rule.id) {
                Some(eval) => eval.set_enabled(rule.enabled),
                None => {
                    let mut eval = AlertEval::new(rule);
                    eval.bind(&self.series);
                    self.alerts.push(eval);
                }
            }
        }
    }

    /// Restores each rule's cooldown from stored alert events (the newest per rule), so
    /// a restart does not re-fire a rule that fired minutes ago. Other events and rules
    /// this build does not know are ignored.
    pub fn seed_alert_history(&mut self, events: &[Event]) {
        for e in events {
            let EventDetail::Alert { rule_id, .. } = &e.detail else {
                continue;
            };
            if let Some(eval) = self.alerts.iter_mut().find(|a| a.rule().id == *rule_id) {
                eval.fired_at(e.ts_ms, &e.processes);
            }
        }
    }

    /// Runs one tick. Events go to `out`; two of one kind on one tick get distinct
    /// timestamps, a millisecond apart, since the store keys events on host, ts and kind.
    pub fn on_tick(
        &mut self,
        ts_ms: i64,
        values: &[f32],
        periods: &[u32],
        batch: Option<&[ProcessSample]>,
        out: &mut Vec<Event>,
    ) {
        if let Some(rows) = batch {
            self.window.push(ts_ms, rows);
        }
        let first = out.len();
        let input = TickInput {
            ts_ms,
            periods,
            values,
            batch,
            recent: &self.window,
        };
        for d in &mut self.detectors {
            d.on_tick(&input, out);
        }
        for a in self.alerts.iter_mut().filter(|a| a.rule().enabled) {
            a.on_tick(&input, out);
        }
        for i in first + 1..out.len() {
            let (done, rest) = out.split_at_mut(i);
            let Some(e) = rest.first_mut() else { continue };
            let kind = e.detail.kind();
            if let Some(last) = done
                .iter()
                .skip(first)
                .filter(|p| p.detail.kind() == kind)
                .map(|p| p.ts_ms)
                .max()
                && last >= e.ts_ms
            {
                e.ts_ms = last + 1;
            }
        }
    }

    /// Forgets every detector's progress and the process window. Alert cooldowns stay.
    pub fn reset(&mut self) {
        for d in &mut self.detectors {
            d.reset();
        }
        for a in &mut self.alerts {
            a.reset();
        }
        self.window.clear();
    }
}

#[cfg(test)]
mod tests;

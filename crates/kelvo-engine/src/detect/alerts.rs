//! Alert rules evaluated on the same per-tick input as the detectors (architecture.md
//! infra 7). A rule fires on the edge: once when its condition has held for `for_secs`,
//! again only after the condition cleared and held again, and never within
//! `cooldown_secs` of its last firing. A firing is an `alert` event; the app shell turns
//! it into a notification.
//!
//! When the rule forgets its progress (switched off, a sleep or clock step, a relayout,
//! a restart seeded from stored alerts) while inside a reported episode, it cannot tell
//! whether the condition cleared meanwhile. Within the cooldown, that episode still met
//! when it next looks counts as the one already reported: it fires again only after
//! clearing and holding again, never just because the cooldown ran out. For the
//! process rule the episode is the processes named in that alert; any other process is
//! new. Progress forgotten outside a reported episode is simply started over.
//!
//! The process rule fires once for every process whose run completed on that batch:
//! one alert naming them all (hottest first, `cause` the hottest), since a second alert
//! on the same tick would only be held off by the cooldown and lost.

use std::sync::Arc;

use kelvo_schema::{AlertCause, AlertRule, Condition, Event, EventDetail, SeriesKey, ThermalState};

use super::detectors::Runs;
use super::{Detector, TickInput, find, value};

enum State {
    Process(Runs),
    Thermal {
        at_least: ThermalState,
        idx: Option<usize>,
        since: Option<i64>,
        fired: bool,
    },
    /// A condition this build does not evaluate (series thresholds have no built-in rule
    /// yet); the rule never fires.
    Unsupported,
}

pub struct AlertEval {
    rule: AlertRule,
    state: State,
    last_fired: Option<i64>,
    /// Progress was forgotten inside a reported episode: the next reading decides
    /// whether the condition still met is that episode (it is, within the cooldown).
    resumed: Resumed,
}

/// What was being reported when progress was forgotten.
enum Resumed {
    No,
    /// The thermal episode, or a process alert stored without names.
    Episode,
    /// These processes' runs (the process rule).
    Processes(Vec<Arc<str>>),
}

impl AlertEval {
    pub fn new(rule: AlertRule) -> Self {
        let for_ms = i64::from(rule.for_secs) * 1000;
        let state = match &rule.when {
            // Ends a reported run only when the process drops to half the threshold, like
            // `sustained_process`, so one long hot process is one alert.
            Condition::ProcessCpuAbove { percent_of_core } => {
                State::Process(Runs::new(*percent_of_core, percent_of_core / 2.0, for_ms))
            }
            Condition::ThermalStateAtLeast(level) => State::Thermal {
                at_least: *level,
                idx: None,
                since: None,
                fired: false,
            },
            Condition::Threshold { .. } => State::Unsupported,
        };
        Self {
            rule,
            state,
            last_fired: None,
            resumed: Resumed::No,
        }
    }

    pub fn rule(&self) -> &AlertRule {
        &self.rule
    }

    /// Switches the rule. Going off forgets its progress, since it sees no input while
    /// off; the cooldown stays.
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.rule.enabled && !enabled {
            self.reset();
        }
        self.rule.enabled = enabled;
    }

    /// Records a firing at `ts` from history (an alert stored before a restart, naming
    /// `processes`), so the cooldown runs from it and, within it, that episode still met
    /// is not reported again. Keeps the newer of it and what is already known. A process
    /// alert without names (none is stored that way) holds off every process hot on the
    /// first batch, since it cannot tell which.
    pub fn fired_at(&mut self, ts: i64, processes: &[String]) {
        if self.last_fired.is_some_and(|last| last > ts) {
            return;
        }
        self.last_fired = Some(ts);
        self.resumed = match (&self.state, processes) {
            (State::Process(_), [_, ..]) => {
                Resumed::Processes(processes.iter().map(|p| Arc::from(p.as_str())).collect())
            }
            _ => Resumed::Episode,
        };
    }

    /// Past the cooldown at `ts`. A clock that stepped back before the last firing does
    /// not hold the rule off.
    fn cooled(&self, ts: i64) -> bool {
        let cooldown = i64::from(self.rule.cooldown_secs) * 1000;
        self.last_fired
            .is_none_or(|last| ts < last || ts - last >= cooldown)
    }

    fn alert(&self, ts: i64, since: i64, processes: Vec<String>, cause: AlertCause) -> Event {
        Event {
            ts_ms: ts,
            start_ms: since,
            processes,
            detail: EventDetail::Alert {
                rule_id: self.rule.id,
                rule_name: self.rule.name.clone(),
                cause,
            },
        }
    }
}

impl Detector for AlertEval {
    fn bind(&mut self, series: &[SeriesKey]) {
        if let State::Thermal { idx, .. } = &mut self.state {
            *idx = find(series, "thermal.state");
        }
        self.reset();
    }

    fn on_tick(&mut self, input: &TickInput<'_>, out: &mut Vec<Event>) {
        let ts = input.ts_ms;
        let for_ms = i64::from(self.rule.for_secs) * 1000;
        let cooled = self.cooled(ts);
        let mut fired = None;
        match &mut self.state {
            State::Process(runs) => {
                let Some(rows) = input.batch else { return };
                // A run that reaches `for_ms` during the cooldown is spent: it does not
                // fire when the cooldown ends. Every run completing on this batch joins
                // one alert. Allocates only when one fires.
                let mut hot: Vec<(Arc<str>, f32, i64)> = Vec::new();
                runs.on_batch(ts, rows, |r| {
                    if cooled {
                        hot.push((Arc::clone(&r.name), r.mean_cpu(), r.since));
                    }
                });
                // The first batch after progress was forgotten inside a reported
                // episode: within the cooldown, the processes of that episode still hot
                // are the ones already reported. Any other process is new.
                match std::mem::replace(&mut self.resumed, Resumed::No) {
                    _ if cooled => {}
                    Resumed::No => {}
                    Resumed::Episode => runs.spend(|_| true),
                    Resumed::Processes(names) => {
                        runs.spend(|n| names.iter().any(|x| &**x == n));
                    }
                }
                hot.sort_by(|a, b| b.1.total_cmp(&a.1));
                if let Some((name, cpu_pct, _)) = hot.first() {
                    let since = hot.iter().map(|h| h.2).min().unwrap_or(ts);
                    let mut names: Vec<String> = Vec::with_capacity(hot.len());
                    for (n, ..) in &hot {
                        if !names.iter().any(|x| x.as_str() == &**n) {
                            names.push(n.to_string());
                        }
                    }
                    fired = Some((
                        since,
                        names,
                        AlertCause::ProcessCpu {
                            process: name.to_string(),
                            cpu_pct: *cpu_pct,
                        },
                    ));
                }
            }
            State::Thermal {
                at_least,
                idx,
                since,
                fired: done,
            } => {
                let Some(state) = idx
                    .and_then(|i| value(input.values, i))
                    .and_then(ThermalState::from_value)
                else {
                    return;
                };
                let resumed = !matches!(
                    std::mem::replace(&mut self.resumed, Resumed::No),
                    Resumed::No
                );
                if state < *at_least {
                    *since = None;
                    *done = false;
                    return;
                }
                if resumed && !cooled {
                    // Still met after forgetting, within the cooldown: the episode
                    // already reported.
                    *done = true;
                }
                let start = *since.get_or_insert(ts);
                if !*done && cooled && ts - start >= for_ms {
                    *done = true;
                    fired = Some((start, Vec::new(), AlertCause::ThermalState { state }));
                }
            }
            State::Unsupported => {}
        }
        if let Some((since, processes, cause)) = fired {
            out.push(self.alert(ts, since, processes, cause));
            self.last_fired = Some(ts);
        }
    }

    /// Forgets the condition's progress (after sleep, a clock step, a relayout or being
    /// switched off). The cooldown survives, and an episode already reported and still
    /// met within it is not a new one: sleeping does not make a rule fire again sooner.
    /// Anything not yet reported starts over.
    fn reset(&mut self) {
        match &mut self.state {
            State::Process(runs) => {
                let names: Vec<Arc<str>> = runs.fired_names().cloned().collect();
                if !names.is_empty() {
                    self.resumed = Resumed::Processes(names);
                }
                runs.clear();
            }
            State::Thermal { since, fired, .. } => {
                if *fired {
                    self.resumed = Resumed::Episode;
                }
                *since = None;
                *fired = false;
            }
            State::Unsupported => {}
        }
    }
}

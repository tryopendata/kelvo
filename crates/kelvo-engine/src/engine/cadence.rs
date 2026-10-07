//! The base tick and Performance mode: the user's interval, its battery and Low Power
//! Mode back-off, the background tick (D-094), and the slower collector periods
//! Performance mode and the background apply (D-088).

use std::sync::atomic::Ordering;
use std::time::Duration;

use kelvo_collect::{Cadence, Collector, Interest, Interests, Tick};
use kelvo_schema::PerformanceReason;
use kelvo_schema::settings::SamplingSettings;

use super::Engine;
use crate::power::PowerState;
use crate::ticker::leeway_for;

/// Base tick while backed off (battery with "slow down on battery", or Low Power Mode):
/// double the user's interval, capped at the longest interval Settings offers (D-061).
pub fn backoff_interval_ms(interval_ms: u32) -> u32 {
    interval_ms
        .saturating_mul(2)
        .min(SamplingSettings::MAX_INTERVAL_MS)
        .max(interval_ms)
}

/// The base tick for these settings and power state: [`backoff_interval_ms`] on battery
/// with "slow down on battery" or Performance mode on, and in Low Power Mode; otherwise
/// the user's interval.
pub fn effective_interval_ms(
    interval_ms: u32,
    slow_on_battery: bool,
    performance_mode: bool,
    power: PowerState,
) -> u32 {
    let base = interval_ms.max(100);
    if (power.on_battery && (slow_on_battery || performance_mode)) || power.low_power_mode {
        backoff_interval_ms(base)
    } else {
        base
    }
}

/// The slowest base tick the engine runs while backgrounded: no visible window shows
/// detail, only the menu bar (D-094).
pub const BACKGROUND_TICK_MS: u32 = 2_000;

/// The base tick while backgrounded, from [`effective_interval_ms`]: at least
/// [`BACKGROUND_TICK_MS`]. It replaces a back-off rather than stacking on it, so a 1 s
/// interval backed off on battery stays at 2 s.
pub fn background_interval_ms(effective_ms: u32) -> u32 {
    effective_ms.max(BACKGROUND_TICK_MS)
}

/// Performance mode (D-088) and the background (D-094): the process collector's period
/// while no window wants process rows (10 s otherwise).
pub const PERFORMANCE_IDLE_PROCESS_MS: u32 = 30_000;
/// The background (D-094): the temperature collectors' period while no window shows
/// detail, a temperature in the menu bar included (5 s otherwise).
pub const PERFORMANCE_IDLE_SENSOR_MS: u32 = 10_000;
/// Performance mode: the shortest process-row period a visible window gets.
pub const PERFORMANCE_VISIBLE_MS: u32 = 2_000;

/// What Performance mode, or the background, slows a collector down for (D-088, D-094).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerformanceSlowdown {
    /// The process collector while no window wants process rows.
    IdleProcesses,
    /// A temperature collector in the background, a temperature in the menu bar
    /// included (D-094). A visible window shows temperatures, so it keeps them at 5 s.
    IdleTemperatures,
}

/// What Performance mode or the background slows `collector` down for this tick, if
/// anything. `backgrounded`: no visible window shows detail.
pub(super) fn performance_slowdown(
    collector: &dyn Collector,
    cadence: Cadence,
    interests: Interests,
    backgrounded: bool,
) -> Option<PerformanceSlowdown> {
    match cadence {
        Cadence::Adaptive {
            interest: Interest::Processes,
            ..
        } if !interests.processes => Some(PerformanceSlowdown::IdleProcesses),
        _ if backgrounded && kelvo_collect::TEMPERATURE_COLLECTORS.contains(&collector.id()) => {
            Some(PerformanceSlowdown::IdleTemperatures)
        }
        _ => None,
    }
}

/// A collector's period in Performance mode or the background, from its normal
/// `period` this tick.
pub fn performance_period(slowdown: Option<PerformanceSlowdown>, period: u32) -> u32 {
    match slowdown {
        Some(PerformanceSlowdown::IdleProcesses) => period.max(PERFORMANCE_IDLE_PROCESS_MS),
        Some(PerformanceSlowdown::IdleTemperatures) => period.max(PERFORMANCE_IDLE_SENSOR_MS),
        None => period,
    }
}

impl Engine {
    /// No visible window shows detail (D-094).
    pub(super) fn backgrounded(&self) -> bool {
        self.shared.detail.load(Ordering::Acquire) == 0
    }

    /// The base tick with a window visible: the interval, or its back-off.
    pub(super) fn visible_interval(&self) -> u32 {
        effective_interval_ms(
            self.settings.interval_ms,
            self.settings.slow_on_battery,
            self.settings.performance_mode,
            self.power_state,
        )
    }

    pub(super) fn effective_interval(&self) -> u32 {
        let tick = self.visible_interval();
        if self.backgrounded() {
            background_interval_ms(tick)
        } else {
            tick
        }
    }

    /// Detail interest crossed between zero and one: re-choose the base tick and publish
    /// the status before the caller's window resumes. Returns the tick to sample now,
    /// once the caller is answered: a faster tick samples at once,
    /// unless the new ticker's first tick is close, the last sample is recent or a tick
    /// is already queued, so the window opens on a fresh frame without a sample a few ms
    /// from another (or one stamped after a queued tick, which would read as a clock
    /// step).
    pub(super) fn on_detail_change(&mut self) -> Option<Tick> {
        let before = self.interval_ms;
        self.apply_interval();
        self.publish_status();
        if self.interval_ms >= before || !self.running() || self.inbox.tick_pending() {
            return None;
        }
        let now = self.ticker.now();
        let interval = i64::from(self.interval_ms);
        let to_next = interval - now.wall_ms.rem_euclid(interval);
        // Before the first tick there is nothing stale to replace.
        let stale = self.last_tick.is_some_and(|t| {
            let ns = now.continuous_ns.saturating_sub(t.continuous_ns);
            i64::try_from(ns / 1_000_000).unwrap_or(i64::MAX) >= interval
        });
        (to_next > interval / 2 && stale).then(|| self.inbox.extra_tick(now))
    }

    pub(super) fn start_ticker(&mut self) {
        if !self.running() {
            return;
        }
        let period = Duration::from_millis(u64::from(self.interval_ms));
        self.ticker
            .start(self.inbox.clone(), period, leeway_for(period));
    }

    /// Why Performance mode is in effect: the setting, or Low Power Mode (D-088).
    pub(super) fn performance(&self) -> PerformanceReason {
        PerformanceReason::resolve(
            self.settings.performance_mode,
            self.power_state.low_power_mode,
        )
    }

    /// Re-reads power state; backs off or restores the base tick on a change.
    pub(super) fn update_power(&mut self) {
        let ps = self.power.poll();
        if ps == self.power_state {
            return;
        }
        self.power_state = ps;
        self.apply_interval();
        self.publish_status();
    }

    pub(super) fn apply_interval(&mut self) {
        let eff = self.effective_interval();
        if eff != self.interval_ms {
            tracing::info!(from = self.interval_ms, to = eff, "base tick changed");
            self.interval_ms = eff;
            if self.running() {
                self.ticker.stop();
                self.start_ticker();
            }
        }
    }
}

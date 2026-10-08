//! Engine, store and settings facts exported to TypeScript as generated constants
//! (D-090, D-092). Each is built from the constant or function the Rust code itself runs
//! on, so the webview and the mock transport never keep a copy that can drift.

use std::collections::BTreeMap;

use kelvo_engine::{
    EngineStatus, PerformanceSlowdown, PowerState, background_interval_ms, effective_interval_ms,
    performance_period,
};
use kelvo_schema::settings::{
    HistorySettings, ItemMode, Readout, SETTINGS_MODULES, SamplingSettings,
};
use kelvo_schema::{MetricKind, Module, PerformanceReason, Tier, Unit};
use kelvo_store::{FILL_MINUTES, FILL_ROLLED, FillMeasurement, Retention};
use serde::Serialize;

use crate::live::{effective_min_period_ms, frame_period_ms};
use crate::tray::model::Pacer;

/// Every catalog metric's kind, by metric id.
pub fn metric_kinds() -> BTreeMap<&'static str, MetricKind> {
    kelvo_schema::Catalog::builtin()
        .defs()
        .iter()
        .map(|d| (d.id.as_str(), d.kind))
        .collect()
}

/// Every catalog metric's unit, by metric id.
pub fn metric_units() -> BTreeMap<&'static str, Unit> {
    kelvo_schema::Catalog::builtin()
        .defs()
        .iter()
        .map(|d| (d.id.as_str(), d.unit))
        .collect()
}

/// Every catalog metric's module, by metric id: which gaps apply to it (the mock's
/// `query_series_stats` leaves out the module's own gaps, as `history.rs` does).
pub fn metric_modules() -> BTreeMap<&'static str, &'static str> {
    kelvo_schema::Catalog::builtin()
        .defs()
        .iter()
        .map(|d| (d.id.as_str(), d.module.as_str()))
        .collect()
}

/// Every catalog metric's nominal minimum sampling period in ms (`MetricDef::period_s`),
/// by metric id: the mock samples `self.cpu` at it, as the engine does.
pub fn metric_periods_ms() -> BTreeMap<&'static str, u32> {
    kelvo_schema::Catalog::builtin()
        .defs()
        .iter()
        .map(|d| (d.id.as_str(), u32::from(d.period_s) * 1_000))
        .collect()
}

/// How long a sample stays current, in sampling periods: `num / den` (D-047, D-090).
#[derive(Clone, Copy, Debug, Serialize, specta::Type)]
pub struct HoldFactor {
    pub num: i32,
    pub den: i32,
}

pub fn hold_factor() -> HoldFactor {
    HoldFactor {
        num: kelvo_engine::STALE_NUM as i32,
        den: kelvo_engine::STALE_DEN as i32,
    }
}

/// How often the store's writer commits, ms (`kelvo_store::DEFAULT_COMMIT_INTERVAL`,
/// D-070). Stored rows newer than this may not be readable yet.
pub fn history_commit_ms() -> i64 {
    i64::try_from(kelvo_store::DEFAULT_COMMIT_INTERVAL.as_millis()).unwrap_or(i64::MAX)
}

/// The own-item modes each settings module offers (`ItemMode::allowed_for`, D-102).
pub fn item_modes() -> BTreeMap<Module, &'static [ItemMode]> {
    SETTINGS_MODULES
        .into_iter()
        .map(|m| (m, ItemMode::allowed_for(m)))
        .collect()
}

/// Every menu bar readout, in the order the menu bar draws them (`Readout::ALL`).
pub const READOUTS: [Readout; 8] = Readout::ALL;

/// One persisted history tier.
#[derive(Clone, Copy, Debug, Serialize, specta::Type)]
pub struct HistoryTier {
    pub tier: Tier,
    pub bucket_ms: i32,
    /// How long the store keeps the tier; `null` for as long as history is kept
    /// (`history.retention_days`).
    pub kept_ms: Option<i32>,
}

/// The persisted tiers, finest first (D-076).
pub fn history_tiers() -> Vec<HistoryTier> {
    let r = Retention::default();
    Tier::PERSISTED
        .into_iter()
        .map(|tier| HistoryTier {
            tier,
            bucket_ms: tier.bucket_ms().map_or(0, ms),
            kept_ms: match tier {
                Tier::S10 => Some(ms(r.s10_ms)),
                Tier::M1 => Some(ms(r.m1_ms)),
                _ => None,
            },
        })
        .collect()
}

/// A span in ms as a JS number; every exported span is far below `i32::MAX`.
fn ms(v: i64) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

/// One fill test run, in the projection's units.
#[derive(Clone, Copy, Debug, Serialize, specta::Type)]
pub struct FillRun {
    pub series: u32,
    pub days: u32,
    pub mb: f64,
}

/// The inputs of Settings' history size projection (D-057, D-076). The arithmetic stays
/// in TypeScript because the retention options are projected synchronously.
#[derive(Clone, Debug, Serialize, specta::Type)]
pub struct HistoryProjection {
    /// Days kept at 1-minute resolution before the roll-down.
    pub minute_days: u32,
    /// Minute rows per 15-minute row.
    pub quarter_factor: u32,
    /// Where a cap trim stops, as a fraction of the cap.
    pub trim_low_water: f64,
    /// The fill test with every day in minutes, at two series counts.
    pub fill_minutes: Vec<FillRun>,
    /// The fill test with the roll-down.
    pub fill_rolled: Vec<FillRun>,
}

pub fn history_projection() -> HistoryProjection {
    let run = |f: &FillMeasurement| FillRun {
        series: f.series,
        days: f.days,
        mb: f.bytes as f64 / 1e6,
    };
    let cap = Retention::default();
    HistoryProjection {
        minute_days: u32::try_from(Retention::M1_WINDOW_MS / Retention::DAY_MS).unwrap_or(0),
        quarter_factor: match (Tier::M15.bucket_ms(), Tier::M1.bucket_ms()) {
            (Some(q), Some(m)) => u32::try_from(q / m).unwrap_or(0),
            _ => 0,
        },
        trim_low_water: cap.low_water_bytes() as f64 / cap.max_bytes as f64,
        fill_minutes: FILL_MINUTES.iter().map(run).collect(),
        fill_rolled: FILL_ROLLED.iter().map(run).collect(),
    }
}

/// What the engine, the tray and the live stream run at on one power source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct PlanFigures {
    /// The engine's base tick with a window open (`effective_interval_ms`).
    pub tick_ms: u32,
    /// The base tick with the menu bar only, in the background (D-094):
    /// `background_interval_ms` of `tick_ms`.
    pub background_tick_ms: u32,
    /// The shortest time between two menu bar draws in the background: the tray pacer's
    /// period, or the background tick when that is longer.
    pub menu_bar_ms: u32,
    /// How often a visible window gets frames: the tick, or Performance mode's floor.
    pub window_ms: u32,
    /// The shortest time between two menu bar draws with a window open.
    pub window_menu_bar_ms: u32,
    /// The process collector's period with a window open that lists no processes.
    pub window_idle_processes_ms: u32,
    /// The process collector's period in the background.
    pub background_processes_ms: u32,
    /// The temperature collectors' period in the background, a temperature in the menu
    /// bar included.
    pub background_temperatures_ms: u32,
}

/// The sampling plan for one combination of settings and power state (D-092). The table
/// covers the whole input domain, so the Settings panel can show what another choice
/// would do ("on battery it would be...") by reading a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct SamplingPlan {
    pub interval_ms: u32,
    pub slow_on_battery: bool,
    /// Performance mode in effect. The engine turns it on under Low Power Mode, so rows
    /// with `low_power_mode` and without `performance` never run: they say what the mode
    /// changes there.
    pub performance: bool,
    pub low_power_mode: bool,
    pub ac: PlanFigures,
    pub battery: PlanFigures,
}

fn figures(
    interval_ms: u32,
    slow_on_battery: bool,
    performance: bool,
    power: PowerState,
) -> PlanFigures {
    let tick_ms = effective_interval_ms(interval_ms, slow_on_battery, performance, power);
    let background_tick_ms = background_interval_ms(tick_ms);
    // `EngineStatus::backed_off`, and the background (`EngineStatus::backgrounded`).
    let backed_off = tick_ms != interval_ms.max(100);
    let pacer = Pacer::for_status(&EngineStatus {
        backed_off,
        backgrounded: true,
        ..EngineStatus::default()
    });
    let ms = |d: std::time::Duration| u32::try_from(d.as_millis()).unwrap_or(u32::MAX);
    let pacer_ms = ms(pacer);
    let reason = PerformanceReason::resolve(performance, false);
    // The background slows these whether or not Performance mode is on; a collector
    // samples at most once per tick.
    let background = |slowdown: PerformanceSlowdown, normal_ms: u32| {
        performance_period(Some(slowdown), normal_ms).max(background_tick_ms)
    };
    PlanFigures {
        tick_ms,
        background_tick_ms,
        menu_bar_ms: pacer_ms.max(background_tick_ms),
        window_ms: frame_period_ms(tick_ms, effective_min_period_ms(0, reason)),
        window_menu_bar_ms: ms(Pacer::for_status(&EngineStatus {
            backed_off,
            performance: reason,
            ..EngineStatus::default()
        }))
        .max(tick_ms),
        // Only Performance mode slows them with a window open.
        window_idle_processes_ms: performance_period(
            performance.then_some(PerformanceSlowdown::IdleProcesses),
            kelvo_engine::IDLE_MS,
        )
        .max(tick_ms),
        background_processes_ms: background(
            PerformanceSlowdown::IdleProcesses,
            kelvo_engine::IDLE_MS,
        ),
        background_temperatures_ms: background(
            PerformanceSlowdown::IdleTemperatures,
            kelvo_engine::TEMPERATURE_PERIOD_MS,
        ),
    }
}

/// Every [`SamplingPlan`], interval by interval.
pub fn sampling_plans() -> Vec<SamplingPlan> {
    let flags = [false, true];
    let mut out = Vec::new();
    for interval_ms in SamplingSettings::INTERVALS_MS {
        for slow_on_battery in flags {
            for performance in flags {
                for low_power_mode in flags {
                    let at = |on_battery| {
                        figures(
                            interval_ms,
                            slow_on_battery,
                            performance,
                            PowerState {
                                on_battery,
                                low_power_mode,
                                ..PowerState::default()
                            },
                        )
                    };
                    out.push(SamplingPlan {
                        interval_ms,
                        slow_on_battery,
                        performance,
                        low_power_mode,
                        ac: at(false),
                        battery: at(true),
                    });
                }
            }
        }
    }
    out
}

/// The settings domains Settings offers and Rust validates.
pub const SAMPLING_INTERVALS_MS: [u32; 7] = SamplingSettings::INTERVALS_MS;
pub const RETENTION_DAYS: [u16; 3] = HistorySettings::RETENTION_DAYS;
pub const SIZE_LIMITS_MB: [u32; 4] = HistorySettings::SIZE_LIMITS_MB;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_cover_the_settings_domain_once() {
        let plans = sampling_plans();
        assert_eq!(plans.len(), SamplingSettings::INTERVALS_MS.len() * 8);
        let mut keys: Vec<_> = plans
            .iter()
            .map(|p| {
                (
                    p.interval_ms,
                    p.slow_on_battery,
                    p.performance,
                    p.low_power_mode,
                )
            })
            .collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), plans.len());
    }

    /// The figures D-061, D-088 and D-094 describe, for a few rows.
    #[test]
    fn plans_match_the_documented_cadences() {
        let row = |interval, slow, perf, lpm| {
            sampling_plans()
                .into_iter()
                .find(|p| {
                    (
                        p.interval_ms,
                        p.slow_on_battery,
                        p.performance,
                        p.low_power_mode,
                    ) == (interval, slow, perf, lpm)
                })
                .unwrap()
        };
        let p = row(1_000, true, false, false);
        assert_eq!((p.ac.tick_ms, p.battery.tick_ms), (1_000, 2_000));
        // The background's 2 s replaces the back-off rather than stacking on it.
        assert_eq!(
            (p.ac.background_tick_ms, p.battery.background_tick_ms),
            (2_000, 2_000)
        );
        // Nor does the menu bar double on battery: the tick under it did not.
        assert_eq!((p.ac.menu_bar_ms, p.battery.menu_bar_ms), (4_000, 4_000));
        assert_eq!(p.ac.window_ms, 1_000);
        assert_eq!(
            (p.ac.window_menu_bar_ms, p.battery.window_menu_bar_ms),
            (2_000, 4_000)
        );
        assert_eq!(p.ac.window_idle_processes_ms, 10_000);
        assert_eq!(p.ac.background_processes_ms, 30_000);
        assert_eq!(p.ac.background_temperatures_ms, 10_000);

        let p = row(1_000, false, true, false);
        // Performance mode backs off on battery whatever the setting says, and paces
        // windows; the background already has its slow cadences.
        assert_eq!((p.ac.tick_ms, p.battery.tick_ms), (1_000, 2_000));
        assert_eq!((p.ac.menu_bar_ms, p.battery.menu_bar_ms), (4_000, 4_000));
        // With a window open it still slows the menu bar and idle process sampling.
        assert_eq!(
            (p.ac.window_menu_bar_ms, p.battery.window_menu_bar_ms),
            (4_000, 8_000)
        );
        assert_eq!(p.ac.window_idle_processes_ms, 30_000);
        assert_eq!(p.ac.window_ms, 2_000);
        assert_eq!(p.ac.background_processes_ms, 30_000);
        assert_eq!(p.ac.background_temperatures_ms, 10_000);

        // A slow interval keeps its own tick in the background.
        let p = row(5_000, true, false, false);
        assert_eq!(
            (p.ac.background_tick_ms, p.battery.background_tick_ms),
            (5_000, 10_000)
        );
        assert_eq!(p.battery.background_temperatures_ms, 10_000);

        // Low Power Mode backs off on AC too; the 60 s cap holds.
        let p = row(60_000, false, false, true);
        assert_eq!((p.ac.tick_ms, p.battery.tick_ms), (60_000, 60_000));
        let p = row(500, false, false, true);
        assert_eq!(p.ac.tick_ms, 1_000);
        assert_eq!(p.ac.background_tick_ms, 2_000);
    }

    /// Each row's tick is the engine's, for the same settings and power state.
    #[test]
    fn plan_ticks_are_the_engines() {
        for p in sampling_plans() {
            for (on_battery, f) in [(false, p.ac), (true, p.battery)] {
                let power = PowerState {
                    on_battery,
                    low_power_mode: p.low_power_mode,
                    ..PowerState::default()
                };
                assert_eq!(
                    f.tick_ms,
                    effective_interval_ms(p.interval_ms, p.slow_on_battery, p.performance, power),
                    "{p:?}"
                );
                assert_eq!(f.background_tick_ms, background_interval_ms(f.tick_ms));
                assert!(f.menu_bar_ms >= f.background_tick_ms && f.window_ms >= f.tick_ms);
            }
        }
    }

    #[test]
    fn tiers_and_projection_come_from_the_store() {
        let tiers = history_tiers();
        let widths: Vec<_> = tiers.iter().map(|t| t.bucket_ms).collect();
        assert_eq!(widths, [10_000, 60_000, 900_000]);
        assert_eq!(tiers[0].kept_ms, Some(86_400_000));
        assert_eq!(tiers[1].kept_ms, Some(7 * 86_400_000));
        assert_eq!(tiers[2].kept_ms, None);
        let p = history_projection();
        assert_eq!((p.minute_days, p.quarter_factor), (7, 15));
        assert!((p.trim_low_water - 0.9).abs() < 1e-9);
    }

    #[test]
    fn item_modes_cover_every_settings_module() {
        let modes = item_modes();
        assert_eq!(modes.len(), SETTINGS_MODULES.len());
        assert_eq!(modes[&Module::Cpu].last(), Some(&ItemMode::Cores));
        assert!(modes.values().all(|m| m.first() == Some(&ItemMode::Off)));
    }
}

//! Events: what the engine's detectors and alert rules noticed (v1.2, architecture.md
//! Store). One [`Event`] is one `events` row: `ts` is [`Event::ts_ms`], `kind` is
//! [`EventDetail::kind`], and the payload is the whole event in CBOR, attribution
//! included. Detector thresholds are data here, [`DetectorThresholds::DEFAULT`], so the
//! fixture tests and the engine read the same numbers.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::alert::ThermalState;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Event {
    /// When the event was detected: the row's `ts`. Two events of one kind never share
    /// it (the store upserts on host, ts and kind).
    #[specta(type = crate::JsSafeInt)]
    pub ts_ms: i64,
    /// Where the episode began (the start of a ramp, a spike or a sustained run); equal
    /// to `ts_ms` for an instant change. The Timeline shades `start_ms..ts_ms`.
    #[specta(type = crate::JsSafeInt)]
    pub start_ms: i64,
    /// Process names held responsible, most responsible first. Names, not pids: a pid
    /// means nothing once the process is gone. Empty when nothing stood out.
    pub processes: Vec<String>,
    pub detail: EventDetail,
}

/// What happened. Internally tagged with the `events.kind` text, so a payload reads
/// alone. There is no `Unknown` variant (D-040's fallback): `#[serde(other)]` cannot be
/// exported to TypeScript with serde phases off, and an event is a whole row, so a kind
/// from a newer build fails to decode and the reader skips that row (D-083).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventDetail {
    /// The fastest fan rose by at least the threshold within the window.
    FansRamped { from_rpm: f32, to_rpm: f32 },
    /// `thermal.state` changed level and held it. `from` is `None` only when the
    /// previous level was never seen.
    ThermalState {
        from: Option<ThermalState>,
        to: ThermalState,
    },
    /// One process stayed above the CPU threshold for the whole duration.
    SustainedProcess {
        process: String,
        /// Mean CPU (percent of one core) over the run.
        cpu_pct: f32,
        secs: u32,
    },
    /// A power component rose well above its rolling baseline and stayed there.
    PowerSpike {
        component: PowerComponent,
        /// Peak watts during the spike so far.
        watts: f32,
        baseline_watts: f32,
    },
    /// An alert rule fired.
    Alert {
        rule_id: Uuid,
        rule_name: String,
        cause: AlertCause,
    },
}

impl EventDetail {
    /// The `events.kind` text, equal to the serialized tag.
    pub const fn kind(&self) -> &'static str {
        match self {
            EventDetail::FansRamped { .. } => "fans_ramped",
            EventDetail::ThermalState { .. } => "thermal_state",
            EventDetail::SustainedProcess { .. } => "sustained_process",
            EventDetail::PowerSpike { .. } => "power_spike",
            EventDetail::Alert { .. } => "alert",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PowerComponent {
    Package,
    Ane,
}

impl PowerComponent {
    /// The metric this component is read from.
    pub const fn metric(self) -> &'static str {
        match self {
            PowerComponent::Package => "power.package",
            PowerComponent::Ane => "power.ane",
        }
    }
}

/// Why an alert fired.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AlertCause {
    ProcessCpu { process: String, cpu_pct: f32 },
    ThermalState { state: ThermalState },
}

/// Detector thresholds (v1-local-monitor.md 1.2-C). Times are wall-clock milliseconds,
/// so a detector behaves the same at every sampling interval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetectorThresholds {
    /// `fans_ramped`: the fastest fan's rise over its lowest reading in the window.
    pub fan_rise_rpm: f32,
    pub fan_window_ms: i64,
    /// After a ramp, the next one counts only once the fans have fallen back within
    /// half a rise of where the last one started, and not before this long.
    pub fan_refractory_ms: i64,
    /// ...or once this long has passed since the ramp, for fans that settle high and
    /// never come back down; the next ramp is then measured from where they are.
    pub fan_rearm_after_ms: i64,
    /// Readings below this are a stopped or spinning-up fan (Apple Silicon fans stop
    /// at idle), not a level a ramp is measured from.
    pub fan_floor_rpm: f32,
    /// `thermal_state`: a new level must hold this long before it is reported, so a
    /// level that flickers for one reading is not an event.
    pub thermal_hold_ms: i64,
    /// `sustained_process`: CPU (percent of one core) a process must stay at or above...
    pub sustained_cpu_pct: f32,
    /// ...for this long.
    pub sustained_ms: i64,
    /// One `sustained_process` event per process name in this long: a build runs many
    /// processes of one name (`rustc`, `clang`) one after another or side by side, and
    /// one pill says it.
    pub sustained_name_refractory_ms: i64,
    /// Once reported, the run ends when the process drops below this.
    pub sustained_release_pct: f32,
    /// `power_spike`: the baseline is an exponential moving average with this time
    /// constant, frozen while a spike lasts.
    pub power_baseline_tau_ms: i64,
    /// No spike is reported until the baseline has this much history...
    pub power_warmup_ms: i64,
    /// ...and at least this many readings: `power.package` is mostly gaps on macOS 27
    /// (D-043), and one reading is not a baseline.
    pub power_warmup_readings: u32,
    /// Readings further apart than this many sampling periods of the series (its
    /// collector's current period: 10 s tray-only, the base tick with a window open)
    /// restart the warm-up: a hold must be seen, not bridged across a gap.
    pub power_max_gap_periods: u32,
    /// A spike is power at least `ratio` times the baseline and at least `min_rise` watts
    /// above it, per component...
    pub power_ratio: f32,
    pub power_min_rise_package_w: f32,
    pub power_min_rise_ane_w: f32,
    /// ...held for this long.
    pub power_hold_ms: i64,
    /// After a spike ends, no second one is reported for this long.
    pub power_refractory_ms: i64,
    /// Attribution: the processes behind `fans_ramped` are the top ones by CPU over this
    /// window before the event, at most `attribution_top` of them, each with at least
    /// `attribution_min_cpu_pct` mean CPU.
    pub attribution_window_ms: i64,
    pub attribution_top: usize,
    pub attribution_min_cpu_pct: f32,
}

impl DetectorThresholds {
    pub const DEFAULT: DetectorThresholds = DetectorThresholds {
        fan_rise_rpm: 1000.0,
        fan_window_ms: 60_000,
        fan_refractory_ms: 120_000,
        fan_rearm_after_ms: 600_000,
        fan_floor_rpm: 500.0,
        thermal_hold_ms: 10_000,
        sustained_cpu_pct: 100.0,
        sustained_ms: 120_000,
        sustained_name_refractory_ms: 600_000,
        sustained_release_pct: 50.0,
        power_baseline_tau_ms: 300_000,
        power_warmup_ms: 120_000,
        power_warmup_readings: 10,
        power_max_gap_periods: 3,
        power_ratio: 1.6,
        power_min_rise_package_w: 8.0,
        power_min_rise_ane_w: 1.0,
        power_hold_ms: 10_000,
        power_refractory_ms: 300_000,
        attribution_window_ms: 60_000,
        attribution_top: 2,
        attribution_min_cpu_pct: 20.0,
    };

    /// The minimum rise for a power component.
    pub const fn power_min_rise_w(&self, c: PowerComponent) -> f32 {
        match c {
            PowerComponent::Package => self.power_min_rise_package_w,
            PowerComponent::Ane => self.power_min_rise_ane_w,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events() -> Vec<Event> {
        vec![
            Event {
                ts_ms: 1_000,
                start_ms: 900,
                processes: vec!["kernel_task".into(), "clang".into()],
                detail: EventDetail::FansRamped {
                    from_rpm: 1200.0,
                    to_rpm: 3400.0,
                },
            },
            Event {
                ts_ms: 2_000,
                start_ms: 2_000,
                processes: vec![],
                detail: EventDetail::ThermalState {
                    from: Some(ThermalState::Nominal),
                    to: ThermalState::Fair,
                },
            },
            Event {
                ts_ms: 3_000,
                start_ms: 1_000,
                processes: vec!["Xcode".into()],
                detail: EventDetail::Alert {
                    rule_id: Uuid::from_u128(1),
                    rule_name: "Hot process".into(),
                    cause: AlertCause::ProcessCpu {
                        process: "Xcode".into(),
                        cpu_pct: 250.0,
                    },
                },
            },
        ]
    }

    #[test]
    fn round_trips_cbor_and_json() {
        for e in events() {
            let mut buf = Vec::new();
            ciborium::into_writer(&e, &mut buf).unwrap();
            assert_eq!(
                ciborium::from_reader::<Event, _>(buf.as_slice()).unwrap(),
                e
            );
            let json = serde_json::to_string(&e).unwrap();
            assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), e);
        }
    }

    #[test]
    fn kind_matches_the_serialized_tag() {
        for e in events() {
            let v = serde_json::to_value(&e.detail).unwrap();
            assert_eq!(v["kind"], e.detail.kind());
        }
    }

    /// A kind from a newer build is an error, which the store's reader turns into a
    /// skipped row (D-083).
    #[test]
    fn unknown_kind_fails_to_decode() {
        let json =
            r#"{"ts_ms":1,"start_ms":1,"processes":[],"detail":{"kind":"battery_swap","x":1}}"#;
        assert!(serde_json::from_str::<Event>(json).is_err());
    }
}

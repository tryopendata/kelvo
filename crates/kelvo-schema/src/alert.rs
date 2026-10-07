//! Alert rules as plain data (architecture.md infra 7). Rules evaluate inside the engine,
//! so the same rule runs locally (v1.2) and on a remote agent (v4). v1.2 ships two
//! built-in rules, [`AlertRule::hot_process`] and [`AlertRule::thermal_serious`], each
//! switched on in settings ([`crate::settings::AlertSettings`]).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::series::SeriesSelector;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct AlertRule {
    pub id: Uuid,
    pub name: String,
    pub when: Condition,
    /// How long the condition must hold before the rule fires.
    pub for_secs: u32,
    /// Minimum time between two firings.
    pub cooldown_secs: u32,
    pub enabled: bool,
}

impl AlertRule {
    /// Built-in: a process above 200% CPU for 5 minutes.
    pub fn hot_process() -> Self {
        Self {
            id: Uuid::from_u128(0x6b65_6c76_6f00_0000_0000_0000_0000_0001),
            name: "Process above 200% CPU for 5 minutes".into(),
            when: Condition::ProcessCpuAbove {
                percent_of_core: 200.0,
            },
            for_secs: 300,
            cooldown_secs: 1800,
            enabled: false,
        }
    }

    /// Built-in: thermal state Serious or worse.
    pub fn thermal_serious() -> Self {
        Self {
            id: Uuid::from_u128(0x6b65_6c76_6f00_0000_0000_0000_0000_0002),
            name: "Thermal state Serious or worse".into(),
            when: Condition::ThermalStateAtLeast(ThermalState::Serious),
            for_secs: 0,
            cooldown_secs: 1800,
            enabled: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    /// Any series matching `series` compared with `value` in the metric's unit.
    Threshold {
        series: SeriesSelector,
        op: Cmp,
        value: f32,
    },
    /// `thermal.state` at or above this level.
    ThermalStateAtLeast(ThermalState),
    /// Any process above this CPU (percent of one core).
    ProcessCpuAbove { percent_of_core: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Cmp {
    Gt,
    Ge,
    Lt,
    Le,
}

impl Cmp {
    /// `lhs op rhs`. NaN (a missing value) never satisfies a comparison.
    pub fn eval(self, lhs: f32, rhs: f32) -> bool {
        match self {
            Cmp::Gt => lhs > rhs,
            Cmp::Ge => lhs >= rhs,
            Cmp::Lt => lhs < rhs,
            Cmp::Le => lhs <= rhs,
        }
    }
}

/// `NSProcessInfo.thermalState`, the value of the `thermal.state` metric. Ordered, so
/// `state >= ThermalState::Serious` reads naturally.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum ThermalState {
    Nominal = 0,
    Fair = 1,
    Serious = 2,
    Critical = 3,
}

impl ThermalState {
    /// From the metric value (0 to 3). `None` for NaN or anything out of range.
    pub fn from_value(v: f32) -> Option<ThermalState> {
        <Self as crate::MetricCode>::from_value(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::series::{Labels, MetricId};

    fn rules() -> Vec<AlertRule> {
        let id = |n| Uuid::from_u128(n);
        vec![
            AlertRule {
                id: id(1),
                name: "Hot process".into(),
                when: Condition::ProcessCpuAbove {
                    percent_of_core: 200.0,
                },
                for_secs: 300,
                cooldown_secs: 1800,
                enabled: true,
            },
            AlertRule {
                id: id(2),
                name: "Thermal".into(),
                when: Condition::ThermalStateAtLeast(ThermalState::Serious),
                for_secs: 0,
                cooldown_secs: 600,
                enabled: true,
            },
            AlertRule {
                id: id(3),
                name: "Core 7 pegged".into(),
                when: Condition::Threshold {
                    series: SeriesSelector {
                        metric: MetricId::from_static("cpu.load"),
                        labels: Labels::single("core", "P7"),
                    },
                    op: Cmp::Ge,
                    value: 95.0,
                },
                for_secs: 60,
                cooldown_secs: 60,
                enabled: false,
            },
        ]
    }

    #[test]
    fn round_trips_json() {
        let r = rules();
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Vec<AlertRule>>(&json).unwrap(), r);
    }

    #[test]
    fn round_trips_cbor() {
        let r = rules();
        let mut buf = Vec::new();
        ciborium::into_writer(&r, &mut buf).unwrap();
        assert_eq!(
            ciborium::from_reader::<Vec<AlertRule>, _>(buf.as_slice()).unwrap(),
            r
        );
    }

    #[test]
    fn condition_json_shape() {
        let c = Condition::ThermalStateAtLeast(ThermalState::Serious);
        assert_eq!(
            serde_json::to_string(&c).unwrap(),
            r#"{"thermal_state_at_least":"serious"}"#
        );
    }

    #[test]
    fn cmp_and_thermal_state() {
        assert!(Cmp::Gt.eval(2.0, 1.0));
        assert!(!Cmp::Gt.eval(f32::NAN, 1.0));
        assert!(!Cmp::Le.eval(f32::NAN, 1.0));
        assert_eq!(ThermalState::from_value(2.0), Some(ThermalState::Serious));
        assert_eq!(ThermalState::from_value(f32::NAN), None);
        assert_eq!(ThermalState::from_value(4.0), None);
        assert!(ThermalState::Critical >= ThermalState::Serious);
    }
}

//! The codes of the catalog's [`crate::Unit::Enum`] metrics: what each small integer a
//! collector writes means. Collectors write [`MetricCode::value`] and consumers read
//! [`MetricCode::from_value`], so the meaning is defined once (D-092). `fan.mode` is not
//! here: its SMC encoding is unverified (`kelvo-collect`'s `smc.rs`).

use std::collections::BTreeMap;

use crate::ThermalState;

/// An enum whose variants are the values of one `Unit::Enum` metric.
pub trait MetricCode: Copy + Sized + 'static {
    /// The metric whose values these are.
    const METRIC: &'static str;
    /// Every variant, in code order.
    const ALL: &'static [Self];

    fn code(self) -> u8;

    /// The variant's name as TypeScript sees it (snake case).
    fn name(self) -> &'static str;

    /// The value a collector writes.
    fn value(self) -> f32 {
        f32::from(self.code())
    }

    /// From a metric value. `None` for NaN or a code this build does not know.
    fn from_value(v: f32) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.value() == v)
    }
}

/// `mem.pressure_level`, from `kern.memorystatus_vm_pressure_level` (D-045).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PressureLevel {
    Normal = 0,
    Warn = 1,
    Critical = 2,
}

impl MetricCode for PressureLevel {
    const METRIC: &'static str = "mem.pressure_level";
    const ALL: &'static [Self] = &[Self::Normal, Self::Warn, Self::Critical];

    fn code(self) -> u8 {
        self as u8
    }

    fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Warn => "warn",
            Self::Critical => "critical",
        }
    }
}

/// `power.cpu_source` (D-054, D-065): present only where SMC P-cluster keys stand in for
/// PMP. The SMC with no scale yet, scaled by a calibration window that closed this
/// session, or scaled by a seed (a scale from an earlier session or the chip's default)
/// while no window has closed. Consumers treat an unknown code as `Uncalibrated`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuPowerCalibration {
    Uncalibrated = 1,
    Calibrated = 2,
    Seeded = 3,
}

impl MetricCode for CpuPowerCalibration {
    const METRIC: &'static str = "power.cpu_source";
    const ALL: &'static [Self] = &[Self::Uncalibrated, Self::Calibrated, Self::Seeded];

    fn code(self) -> u8 {
        self as u8
    }

    fn name(self) -> &'static str {
        match self {
            Self::Uncalibrated => "uncalibrated",
            Self::Calibrated => "calibrated",
            Self::Seeded => "seeded",
        }
    }
}

impl MetricCode for ThermalState {
    const METRIC: &'static str = "thermal.state";
    const ALL: &'static [Self] = &[Self::Nominal, Self::Fair, Self::Serious, Self::Critical];

    fn code(self) -> u8 {
        self as u8
    }

    fn name(self) -> &'static str {
        match self {
            Self::Nominal => "nominal",
            Self::Fair => "fair",
            Self::Serious => "serious",
            Self::Critical => "critical",
        }
    }
}

fn codes<T: MetricCode>() -> (&'static str, BTreeMap<&'static str, u8>) {
    (
        T::METRIC,
        T::ALL.iter().map(|c| (c.name(), c.code())).collect(),
    )
}

/// Every coded metric's codes by name, by metric id: the generated `METRIC_CODES`.
pub fn metric_codes() -> BTreeMap<&'static str, BTreeMap<&'static str, u8>> {
    [
        codes::<PressureLevel>(),
        codes::<CpuPowerCalibration>(),
        codes::<ThermalState>(),
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Catalog, Unit};

    #[test]
    fn coded_metrics_are_enum_metrics_and_round_trip() {
        let catalog = Catalog::builtin();
        for (metric, by_name) in metric_codes() {
            let def = catalog.defs().iter().find(|d| d.id.as_str() == metric);
            assert_eq!(def.map(|d| d.unit), Some(Unit::Enum), "{metric}");
            assert!(!by_name.is_empty());
        }
        for &c in PressureLevel::ALL {
            assert_eq!(PressureLevel::from_value(c.value()), Some(c));
        }
        for &c in CpuPowerCalibration::ALL {
            assert_eq!(CpuPowerCalibration::from_value(c.value()), Some(c));
        }
        assert_eq!(ThermalState::from_value(2.0), Some(ThermalState::Serious));
        assert_eq!(ThermalState::from_value(4.0), None);
        assert_eq!(PressureLevel::from_value(f32::NAN), None);
    }

    #[test]
    fn thermal_names_match_serde() {
        for &c in ThermalState::ALL {
            let json = serde_json::to_string(&c).unwrap();
            assert_eq!(json, format!("\"{}\"", c.name()));
        }
    }
}

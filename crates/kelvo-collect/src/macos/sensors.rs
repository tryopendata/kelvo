//! Mapped temperatures from the SMC: `thermal.cpu`, `thermal.gpu` and
//! `thermal.sensor{name}`, through a sensor map per chip family.
//!
//! The SMC exposes hundreds of `T???` keys and their meaning differs per chip generation.
//! The per-generation CPU and GPU key lists come from Stats
//! (<https://github.com/exelban/stats>, `Modules/Sensors/values.swift`, MIT, Copyright (c)
//! 2019 Serhiy Mytrovtsiy). Verification status:
//!
//! - M3 (checked on an M3 Max, macOS 27.0.1): all 16 CPU and 8 GPU keys read 40 to 55 °C
//!   at idle and track load. Prefix-matching (`Tp*`, `Tf*`) would not work: the M3 Max
//!   also has `Tp1g`..`Tp3j` pinned at 40.0 and `Tf06`/`Tf16` reading 92 to 109, which
//!   look like limits, not sensors.
//! - M1, M2, M4: taken from Stats unverified; covered only by fixture tests here.
//!
//! `thermal.cpu` and `thermal.gpu` are the max over the family's keys (the hottest core),
//! per v1-local-monitor.md 6.1. Keys that do not read a plausible value at probe time are
//! dropped, so a Pro chip with fewer cores than the list simply skips the missing ones.
//!
//! Chips without a map (M5 and later, or an unrecognised brand string) probe as
//! `Unsupported(UnknownChip)`; raw HID zones (`hid.rs`) keep working there.

use kelvo_schema::{Entitlement, Labels, MetricId, Module, SeriesKey, UnsupportedReason};

use super::vendor::smc::{Smc, four_cc};
use super::vendor::soc;
use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

pub const ID: CollectorId = crate::TEMPERATURE_COLLECTORS[0];

/// Apple Silicon generation and tier, parsed from `machdep.cpu.brand_string`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chip {
    pub generation: u8,
    pub tier: Tier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Base,
    Pro,
    Max,
    Ultra,
}

/// "Apple M3 Max" -> M3 Max. `None` for anything that is not an Apple M-series name.
pub fn parse_chip(brand: &str) -> Option<Chip> {
    let rest = brand.trim().strip_prefix("Apple M")?;
    let mut parts = rest.split_whitespace();
    let generation: u8 = parts.next()?.parse().ok()?;
    let tier = match parts.next() {
        None => Tier::Base,
        Some("Pro") => Tier::Pro,
        Some("Max") => Tier::Max,
        Some("Ultra") => Tier::Ultra,
        Some(_) => return None,
    };
    Some(Chip { generation, tier })
}

/// SMC keys for one chip family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SensorMap {
    pub cpu: &'static [&'static str],
    pub gpu: &'static [&'static str],
}

/// Named sensors shared by every Apple Silicon Mac that has them (Stats' "Apple Silicon"
/// group plus `TB0T`). Missing keys (no battery on a desktop) are skipped at probe time.
/// Verified on the M3 Max: battery 31 to 33 °C, NAND 32 °C, Wi-Fi 39 °C.
pub const NAMED: &[(&str, &[&str])] = &[
    ("battery", &["TB0T", "TB1T", "TB2T"]),
    ("ssd", &["TH0x"]),
    ("wifi", &["TW0P"]),
];

const M1: SensorMap = SensorMap {
    cpu: &[
        "Tp09", "Tp0T", "Tp01", "Tp05", "Tp0D", "Tp0H", "Tp0L", "Tp0P", "Tp0X", "Tp0b",
    ],
    gpu: &["Tg05", "Tg0D", "Tg0L", "Tg0T"],
};

const M2: SensorMap = SensorMap {
    cpu: &[
        "Tp1h", "Tp1t", "Tp1p", "Tp1l", "Tp01", "Tp05", "Tp09", "Tp0D", "Tp0X", "Tp0b", "Tp0f",
        "Tp0j",
    ],
    gpu: &["Tg0f", "Tg0j"],
};

const M3: SensorMap = SensorMap {
    cpu: &[
        "Te05", "Te0L", "Te0P", "Te0S", "Tf04", "Tf09", "Tf0A", "Tf0B", "Tf0D", "Tf0E", "Tf44",
        "Tf49", "Tf4A", "Tf4B", "Tf4D", "Tf4E",
    ],
    gpu: &[
        "Tf14", "Tf18", "Tf19", "Tf1A", "Tf24", "Tf28", "Tf29", "Tf2A",
    ],
};

const M4_CPU: &[&str] = &[
    "Te05", "Te0S", "Te09", "Te0H", "Tp01", "Tp05", "Tp09", "Tp0D", "Tp0V", "Tp0Y", "Tp0b", "Tp0e",
];

const M4: SensorMap = SensorMap {
    cpu: M4_CPU,
    gpu: &[
        "Tg0G", "Tg0H", "Tg0K", "Tg0L", "Tg0d", "Tg0e", "Tg0j", "Tg0k",
    ],
};

const M4_PRO_MAX: SensorMap = SensorMap {
    cpu: M4_CPU,
    gpu: &[
        "Tg1U", "Tg1k", "Tg0K", "Tg0L", "Tg0d", "Tg0e", "Tg0j", "Tg0k",
    ],
};

/// The sensor map for a chip, or `None` if Kelvo does not know the family yet.
pub fn sensor_map(chip: Chip) -> Option<SensorMap> {
    match (chip.generation, chip.tier) {
        (1, _) => Some(M1),
        (2, _) => Some(M2),
        (3, _) => Some(M3),
        (4, Tier::Base) => Some(M4),
        (4, _) => Some(M4_PRO_MAX),
        _ => None,
    }
}

/// A temperature worth reporting. 0 °C and below means "no sensor"; above 150 °C is a
/// limit or garbage (macmon's bounds).
pub fn plausible_celsius(t: f32) -> bool {
    t.is_finite() && t > 0.0 && t <= 150.0
}

/// The max of the plausible values, if any.
pub fn max_plausible(values: impl IntoIterator<Item = Option<f32>>) -> Option<f32> {
    values
        .into_iter()
        .flatten()
        .filter(|t| plausible_celsius(*t))
        .reduce(f32::max)
}

struct Group {
    key: SeriesKey,
    smc_keys: Vec<u32>,
}

/// The SMC sensor collector.
pub struct SmcSensors {
    smc: Option<Smc>,
    groups: Vec<Group>,
    chip_override: Option<String>,
}

impl Default for SmcSensors {
    fn default() -> Self {
        Self::new()
    }
}

impl SmcSensors {
    pub fn new() -> Self {
        Self {
            smc: None,
            groups: Vec::new(),
            chip_override: None,
        }
    }

    /// Probes as if the machine reported `brand` (tests of the unknown-chip path on
    /// known hardware).
    pub fn with_brand(brand: &str) -> Self {
        Self {
            chip_override: Some(brand.to_owned()),
            ..Self::new()
        }
    }
}

impl Collector for SmcSensors {
    fn id(&self) -> CollectorId {
        ID
    }

    fn cadence(&self) -> Cadence {
        Cadence::Every(crate::TEMPERATURE_PERIOD_MS)
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Sensors]
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::SmcUserClient]
    }

    fn probe(&mut self) -> Probe {
        self.smc = None;
        self.groups.clear();
        let brand = self
            .chip_override
            .clone()
            .or_else(soc::chip_name)
            .unwrap_or_default();
        let Some(map) = parse_chip(&brand).and_then(sensor_map) else {
            return Probe::Unsupported {
                reason: UnsupportedReason::UnknownChip,
            };
        };
        let Ok(mut smc) = Smc::open() else {
            return Probe::Unsupported {
                reason: UnsupportedReason::NoHardware,
            };
        };
        let m = MetricId::from_static;
        let mut usable = |names: &[&str]| -> Vec<u32> {
            names
                .iter()
                .filter_map(|n| four_cc(n))
                .filter(|k| smc.read_f32(*k).is_some_and(plausible_celsius))
                .collect()
        };
        let cpu = usable(map.cpu);
        let gpu = usable(map.gpu);
        if cpu.is_empty() && gpu.is_empty() {
            // The family is known but none of its keys read: the map does not fit.
            return Probe::Unsupported {
                reason: UnsupportedReason::UnknownChip,
            };
        }
        let mut groups = Vec::new();
        for (id, keys) in [("thermal.cpu", cpu), ("thermal.gpu", gpu)] {
            if !keys.is_empty() {
                groups.push(Group {
                    key: SeriesKey::bare(m(id)),
                    smc_keys: keys,
                });
            }
        }
        for (name, names) in NAMED {
            let keys = usable(names);
            if !keys.is_empty() {
                groups.push(Group {
                    key: SeriesKey::new(m("thermal.sensor"), Labels::single("name", name)),
                    smc_keys: keys,
                });
            }
        }
        self.groups = groups;
        self.smc = Some(smc);
        Probe::Supported(self.groups.iter().map(|g| g.key.clone()).collect())
    }

    fn sample(&mut self, _tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let smc = self.smc.as_mut().ok_or(CollectError::NotProbed)?;
        for g in &self.groups {
            if let Some(t) = max_plausible(g.smc_keys.iter().map(|k| smc.read_f32(*k))) {
                out.push(&g.key, t);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_brand_strings() {
        let c = |generation, tier| Some(Chip { generation, tier });
        assert_eq!(parse_chip("Apple M1"), c(1, Tier::Base));
        assert_eq!(parse_chip("Apple M3 Max"), c(3, Tier::Max));
        assert_eq!(parse_chip("Apple M2 Ultra"), c(2, Tier::Ultra));
        assert_eq!(parse_chip("Apple M4 Pro"), c(4, Tier::Pro));
        assert_eq!(parse_chip("Apple M12 Pro"), c(12, Tier::Pro));
        assert_eq!(parse_chip("Intel(R) Core(TM) i9-9980HK"), None);
        assert_eq!(parse_chip("Apple M3 Hyper"), None);
        assert_eq!(parse_chip(""), None);
    }

    #[test]
    fn maps_known_families_only() {
        for brand in [
            "Apple M1 Max",
            "Apple M2",
            "Apple M3 Pro",
            "Apple M4",
            "Apple M4 Ultra",
        ] {
            let chip = parse_chip(brand).unwrap();
            let map = sensor_map(chip).unwrap();
            assert!(!map.cpu.is_empty() && !map.gpu.is_empty(), "{brand}");
            for k in map.cpu.iter().chain(map.gpu) {
                assert!(four_cc(k).is_some(), "{k} is a four-character key");
            }
        }
        assert_eq!(sensor_map(parse_chip("Apple M5 Max").unwrap()), None);
        assert_ne!(
            sensor_map(parse_chip("Apple M4").unwrap()),
            sensor_map(parse_chip("Apple M4 Max").unwrap()),
            "M4 base and Pro/Max differ in GPU keys"
        );
    }

    #[test]
    fn max_ignores_missing_and_implausible() {
        // A missing key, a 0.0 placeholder, a negative and a NaN are dropped; the max of
        // what is left wins.
        let v = [
            Some(44.9),
            None,
            Some(0.0),
            Some(-20.6),
            Some(f32::NAN),
            Some(47.5),
        ];
        assert_eq!(max_plausible(v), Some(47.5));
        assert_eq!(max_plausible([Some(0.0), None]), None, "nothing plausible");
        assert_eq!(max_plausible([Some(151.0)]), None);
    }

    #[test]
    fn unknown_chip_is_unsupported_without_touching_the_smc() {
        let mut c = SmcSensors::with_brand("Apple M9 Max");
        assert_eq!(
            c.probe(),
            Probe::Unsupported {
                reason: UnsupportedReason::UnknownChip
            }
        );
        let mut c = SmcSensors::with_brand("Intel(R) Core(TM) i7");
        assert!(matches!(c.probe(), Probe::Unsupported { .. }));
    }

    /// Samples the mapped SMC temperatures twice on real hardware.
    #[test]
    #[ignore = "needs Apple Silicon hardware; run by hand with --ignored --nocapture"]
    fn live_smc_sensors() {
        println!("chip: {:?}", soc::chip_name());
        let mut c = SmcSensors::new();
        let probe = c.probe();
        println!("probe: {probe:?}");
        let Probe::Supported(series) = probe else {
            return;
        };
        for g in &c.groups {
            let names: Vec<String> = g
                .smc_keys
                .iter()
                .map(|k| String::from_utf8_lossy(&k.to_be_bytes()).into_owned())
                .collect();
            println!("  {} <- {}", g.key, names.join(" "));
        }
        for n in 0..2 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let tick = Tick {
                n,
                wall_ms: 0,
                continuous_ns: 0,
                interval_ms: 1_000,
            };
            let mut out = SampleBuf::new();
            c.sample(&tick, &mut out).unwrap();
            for s in out.values() {
                println!("  {} = {:.1}", s.key, s.value);
                assert!((20.0..=110.0).contains(&s.value), "{} {}", s.key, s.value);
            }
            assert_eq!(out.values().len(), series.len());
        }
    }
}

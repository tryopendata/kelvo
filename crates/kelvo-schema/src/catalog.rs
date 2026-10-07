//! The metric catalog: every metric Kelvo knows, with unit, kind, labels and module.
//!
//! [`CATALOG`] is the v1 content from v1-local-monitor.md 6.1. IDs are stable; renaming one
//! is a decision entry. Receivers ignore metric IDs that are not in their catalog, which
//! is what lets an older build read data from a newer one.
//!
//! Cadence and entitlement are not here: they belong to the collector that produces the
//! metric (`kelvo-collect`), since one collector can produce several metrics.

use serde::{Deserialize, Serialize};

use crate::series::{MetricId, SeriesKey};

/// Unit of a metric's values. Values are always `f32` in this unit.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// 0 to 100. Per-process CPU is percent of one core and can exceed 100.
    Percent,
    Hz,
    Watts,
    WattHours,
    Celsius,
    Bytes,
    BytesPerSec,
    BitsPerSec,
    PagesPerSec,
    Rpm,
    Minutes,
    Count,
    /// 0 or 1.
    Bool,
    /// A small integer code whose meaning is documented on the metric
    /// (`mem.pressure_level` 0 to 2, `thermal.state` 0 to 3, `fan.mode`).
    Enum,
}

/// How a metric's values behave over time.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    /// A level read at the sample time.
    Gauge,
    /// Per-second rate over the span since the series' previous sample.
    Rate,
    /// The mean over the span since the series' previous sample: a counter delta such as
    /// CPU load, residency or energy turned into watts (D-090). A collector that slows
    /// down still covers every second, at a coarser resolution.
    Mean,
    /// Monotonic counter. Collectors convert counters to rates, so no v1 metric is a
    /// counter; the variant exists for v4 sources that may send raw counters.
    Counter,
}

/// A UI and settings module. Every metric belongs to exactly one.
///
/// Serialized in snake_case (`"cpu"`), the same text the store writes to `gaps.module`.
/// The UI shows `Power` and `Sensors` as one "Power & Sensors" module; see
/// [`crate::settings`] for how settings treat the pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Module {
    Cpu,
    Gpu,
    Memory,
    Power,
    Sensors,
    Network,
    Disk,
    Battery,
    /// A module this build does not know, sent by a newer peer or written by a newer
    /// build (D-040). Never produced locally and not in [`Module::ALL`]. Consumers skip
    /// it: capabilities and settings drop it on decode, the store skips its gaps.
    Unknown,
}

impl Module {
    pub const ALL: [Module; 8] = [
        Module::Cpu,
        Module::Gpu,
        Module::Memory,
        Module::Power,
        Module::Sensors,
        Module::Network,
        Module::Disk,
        Module::Battery,
    ];

    /// The snake_case text form, as stored in `gaps.module`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Module::Cpu => "cpu",
            Module::Gpu => "gpu",
            Module::Memory => "memory",
            Module::Power => "power",
            Module::Sensors => "sensors",
            Module::Network => "network",
            Module::Disk => "disk",
            Module::Battery => "battery",
            Module::Unknown => "unknown",
        }
    }

    /// Inverse of [`Module::as_str`] for the known modules. `None` for anything else,
    /// including `"unknown"`.
    pub fn parse(s: &str) -> Option<Module> {
        Module::ALL.into_iter().find(|m| m.as_str() == s)
    }
}

crate::compat::text_enum_deserialize!(Module);

/// `deserialize_with` for a map keyed by [`Module`]: entries for modules this build does
/// not know are dropped, so a newer peer's extra module neither fails the decode nor
/// shows up as an `"unknown"` key (D-040).
pub(crate) fn deserialize_known_modules<'de, D, V>(
    deserializer: D,
) -> Result<std::collections::BTreeMap<Module, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: Deserialize<'de>,
{
    let mut map = std::collections::BTreeMap::<Module, V>::deserialize(deserializer)?;
    map.remove(&Module::Unknown);
    Ok(map)
}

/// One catalog entry.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricDef {
    pub id: MetricId,
    pub unit: Unit,
    pub kind: MetricKind,
    /// The exact label keys every series of this metric carries, in sorted order. Empty
    /// for unlabelled metrics.
    pub label_keys: &'static [&'static str],
    pub module: Module,
    /// Whether the series goes to `tier_10s` and `tier_1m`. Non-persisted series live only
    /// in the 1 s ring buffer.
    pub persisted: bool,
    /// Upper bound on the number of series of this metric per host, for metrics whose
    /// label values are unbounded (v4 containers). `None` means no cap.
    pub max_cardinality: Option<u16>,
    /// Nominal minimum sampling period in seconds (the 6.1 "Cadence" column, read as
    /// wall-clock time since D-061): 1 for every tick at the default 1 s, 60 for disk
    /// capacity. A collector may sample a series less often than its own cadence
    /// (`fan.max` inside the every-2-s fan collector); the engine uses the larger of this,
    /// the collector's period and the base tick to decide how long a value stays current
    /// in the Snapshot.
    pub period_s: u16,
}

impl MetricDef {
    /// Whether `key`'s label keys are exactly this metric's `label_keys`.
    pub fn labels_match(&self, key: &SeriesKey) -> bool {
        key.labels.len() == self.label_keys.len()
            && key.labels.keys().zip(self.label_keys).all(|(a, b)| a == *b)
    }
}

const fn def(
    id: &'static str,
    module: Module,
    unit: Unit,
    kind: MetricKind,
    label_keys: &'static [&'static str],
    persisted: bool,
) -> MetricDef {
    MetricDef {
        id: MetricId::from_static(id),
        unit,
        kind,
        label_keys,
        module,
        persisted,
        max_cardinality: None,
        period_s: 1,
    }
}

impl MetricDef {
    /// The same definition with a nominal minimum period of `secs` seconds.
    const fn every(mut self, secs: u16) -> Self {
        self.period_s = secs;
        self
    }
}

use MetricKind::{Gauge, Mean, Rate};
use Module as M;
use Unit as U;

const P: bool = true; // persisted
const R: bool = false; // ring buffer only

/// The v1 metric catalog (v1-local-monitor.md 6.1).
pub static CATALOG: &[MetricDef] = &[
    // CPU
    def("cpu.total", M::Cpu, U::Percent, Mean, &[], P),
    def("cpu.user", M::Cpu, U::Percent, Mean, &[], P),
    def("cpu.system", M::Cpu, U::Percent, Mean, &[], P),
    def("cpu.load", M::Cpu, U::Percent, Mean, &["core"], P),
    def("cpu.loadavg", M::Cpu, U::Count, Gauge, &["window"], P).every(5),
    def("cpu.cluster.freq", M::Cpu, U::Hz, Mean, &["cluster"], P),
    def(
        "cpu.cluster.active",
        M::Cpu,
        U::Percent,
        Mean,
        &["cluster"],
        P,
    ),
    def(
        "cpu.cluster.residency",
        M::Cpu,
        U::Percent,
        Mean,
        &["cluster", "state"],
        R,
    ),
    def("cpu.cluster.power", M::Cpu, U::Watts, Mean, &["cluster"], P),
    // GPU
    def("gpu.util", M::Gpu, U::Percent, Gauge, &[], P),
    def("gpu.render", M::Gpu, U::Percent, Gauge, &[], P),
    def("gpu.tiler", M::Gpu, U::Percent, Gauge, &[], P),
    def("gpu.freq", M::Gpu, U::Hz, Mean, &[], P),
    def("gpu.residency", M::Gpu, U::Percent, Mean, &["state"], R),
    // Memory
    def("mem.used", M::Memory, U::Bytes, Gauge, &[], P),
    def("mem.app", M::Memory, U::Bytes, Gauge, &[], P),
    def("mem.wired", M::Memory, U::Bytes, Gauge, &[], P),
    def("mem.compressed", M::Memory, U::Bytes, Gauge, &[], P),
    def("mem.cached", M::Memory, U::Bytes, Gauge, &[], P),
    def("mem.free", M::Memory, U::Bytes, Gauge, &[], P),
    def("mem.pressure", M::Memory, U::Percent, Gauge, &[], P),
    // 0 normal, 1 warn, 2 critical.
    def("mem.pressure_level", M::Memory, U::Enum, Gauge, &[], P),
    def("mem.swap_used", M::Memory, U::Bytes, Gauge, &[], P),
    def("mem.swap_in", M::Memory, U::PagesPerSec, Rate, &[], P),
    def("mem.swap_out", M::Memory, U::PagesPerSec, Rate, &[], P),
    // Power
    def("power.cpu", M::Power, U::Watts, Mean, &[], P),
    def("power.gpu", M::Power, U::Watts, Mean, &[], P),
    def("power.ane", M::Power, U::Watts, Mean, &[], P),
    def("power.dram", M::Power, U::Watts, Mean, &[], P),
    def("power.package", M::Power, U::Watts, Mean, &[], P),
    def("power.system", M::Power, U::Watts, Gauge, &[], P),
    // Where `power.cpu` comes from (D-054). Absent: PMP counters, all clusters (D-043).
    // 1: SMC, P clusters only, no scale (reads about 25% low). 2: SMC, P clusters only,
    // scaled to PMP energy by a window that closed this session. 3: SMC, P clusters
    // only, scaled by a seed (an earlier session's scale or the chip default) until a
    // window closes (D-065). The UI labels the value with it; treat unknown values as 1.
    def("power.cpu_source", M::Power, U::Enum, Gauge, &[], R),
    // Sensors. Temperatures are read every 5 ticks (D-055): the HID and SMC reads are
    // the engine's largest kernel cost, and die temperatures move over seconds.
    def(
        "thermal.zone",
        M::Sensors,
        U::Celsius,
        Gauge,
        &["sensor"],
        P,
    )
    .every(5),
    def("thermal.cpu", M::Sensors, U::Celsius, Gauge, &[], P).every(5),
    def("thermal.gpu", M::Sensors, U::Celsius, Gauge, &[], P).every(5),
    def("thermal.hottest", M::Sensors, U::Celsius, Gauge, &[], P).every(5),
    def(
        "thermal.sensor",
        M::Sensors,
        U::Celsius,
        Gauge,
        &["name"],
        P,
    )
    .every(5),
    // NSProcessInfo.thermalState: 0 nominal, 1 fair, 2 serious, 3 critical.
    def("thermal.state", M::Sensors, U::Enum, Gauge, &[], P).every(2),
    def("fan.rpm", M::Sensors, U::Rpm, Gauge, &["fan"], P).every(2),
    def("fan.max", M::Sensors, U::Rpm, Gauge, &["fan"], R).every(60),
    def("fan.mode", M::Sensors, U::Enum, Gauge, &[], R).every(60),
    // Network
    def("net.rx", M::Network, U::BytesPerSec, Rate, &["iface"], P),
    def("net.tx", M::Network, U::BytesPerSec, Rate, &["iface"], P),
    def(
        "net.link_rate",
        M::Network,
        U::BitsPerSec,
        Gauge,
        &["iface"],
        R,
    )
    .every(60),
    // The sum over the interfaces `net.rx`/`net.tx` report on that sample, computed by
    // the network collector in the same sample (D-092). A gap when any reported
    // interface's own value is a gap; rx and tx are gated separately.
    def("net.rx_total", M::Network, U::BytesPerSec, Rate, &[], P),
    def("net.tx_total", M::Network, U::BytesPerSec, Rate, &[], P),
    // Disk
    def("disk.read", M::Disk, U::BytesPerSec, Rate, &["dev"], P),
    def("disk.write", M::Disk, U::BytesPerSec, Rate, &["dev"], P),
    // The sum over the devices `disk.read`/`disk.write` report, with the network
    // totals' gap rule (D-092).
    def("disk.read_total", M::Disk, U::BytesPerSec, Rate, &[], P),
    def("disk.write_total", M::Disk, U::BytesPerSec, Rate, &[], P),
    def("disk.used", M::Disk, U::Bytes, Gauge, &["vol"], P).every(60),
    def("disk.free", M::Disk, U::Bytes, Gauge, &["vol"], P).every(60),
    def("disk.total", M::Disk, U::Bytes, Gauge, &["vol"], P).every(60),
    // Battery
    def("battery.charge", M::Battery, U::Percent, Gauge, &[], P).every(10),
    def("battery.charging", M::Battery, U::Bool, Gauge, &[], P).every(10),
    def("battery.external", M::Battery, U::Bool, Gauge, &[], P).every(10),
    def(
        "battery.time_remaining",
        M::Battery,
        U::Minutes,
        Gauge,
        &[],
        R,
    )
    .every(10),
    def("battery.health", M::Battery, U::Percent, Gauge, &[], P).every(60),
    def("battery.cycles", M::Battery, U::Count, Gauge, &[], P).every(60),
    def(
        "battery.capacity_wh",
        M::Battery,
        U::WattHours,
        Gauge,
        &[],
        R,
    )
    .every(60),
    def("battery.design_wh", M::Battery, U::WattHours, Gauge, &[], R).every(60),
    // Signed: negative while discharging.
    def("battery.power", M::Battery, U::Watts, Gauge, &[], P).every(10),
    def("battery.temp", M::Battery, U::Celsius, Gauge, &[], P).every(10),
    // Kelvo's own CPU across the app and its WebKit helpers, CPU time over the span since
    // the previous sample (D-092). Filed under Cpu because the module list has no "app"
    // entry; nothing gates it on the CPU module switch.
    def("self.cpu", M::Cpu, U::Percent, Mean, &[], P).every(10),
];

/// Why a series key is not valid against the catalog.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CatalogError {
    #[error("unknown metric {0}")]
    UnknownMetric(String),
    #[error("series {key} has labels that do not match {expected:?}")]
    LabelMismatch {
        key: String,
        expected: &'static [&'static str],
    },
}

/// An indexed view of a metric catalog. Build it once (it sorts an index) and share it.
#[derive(Clone, Debug)]
pub struct Catalog {
    defs: &'static [MetricDef],
    /// Indices into `defs`, sorted by metric ID.
    by_id: Vec<usize>,
}

impl Catalog {
    /// The built-in v1 catalog.
    pub fn builtin() -> Self {
        Self::new(CATALOG)
    }

    pub fn new(defs: &'static [MetricDef]) -> Self {
        let mut by_id: Vec<usize> = (0..defs.len()).collect();
        by_id.sort_by(|&a, &b| {
            let ka = defs.get(a).map(|d| d.id.as_str());
            let kb = defs.get(b).map(|d| d.id.as_str());
            ka.cmp(&kb)
        });
        Self { defs, by_id }
    }

    pub fn defs(&self) -> &'static [MetricDef] {
        self.defs
    }

    /// The definition for a metric ID, or `None` if this build does not know it.
    pub fn get(&self, id: &str) -> Option<&'static MetricDef> {
        let defs = self.defs;
        self.by_id
            .binary_search_by(|&i| defs.get(i).map(|d| d.id.as_str()).cmp(&Some(id)))
            .ok()
            .and_then(|pos| self.by_id.get(pos))
            .and_then(|&i| defs.get(i))
    }

    /// Checks a key against the catalog: the metric must be known and the label keys must
    /// match its definition exactly.
    pub fn validate(&self, key: &SeriesKey) -> Result<&'static MetricDef, CatalogError> {
        let def = self
            .get(key.metric.as_str())
            .ok_or_else(|| CatalogError::UnknownMetric(key.metric.to_string()))?;
        if def.labels_match(key) {
            Ok(def)
        } else {
            Err(CatalogError::LabelMismatch {
                key: key.to_string(),
                expected: def.label_keys,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::series::Labels;

    #[test]
    fn ids_are_unique_and_valid() {
        let mut seen = BTreeSet::new();
        for d in CATALOG {
            assert!(d.id.is_valid(), "{} is not a valid metric id", d.id);
            assert!(seen.insert(d.id.as_str()), "{} is listed twice", d.id);
        }
    }

    #[test]
    fn label_keys_are_sorted_unique_and_parse() {
        for d in CATALOG {
            let sorted: Vec<_> = {
                let mut v = d.label_keys.to_vec();
                v.sort_unstable();
                v.dedup();
                v
            };
            assert_eq!(
                sorted, d.label_keys,
                "{}: label keys must be sorted and unique",
                d.id
            );
            // A key built from the definition validates against it, and its display form
            // parses back to the same key.
            let labels = Labels::from_pairs(d.label_keys.iter().map(|k| (*k, "x"))).unwrap();
            let key = SeriesKey::new(d.id.clone(), labels);
            assert_eq!(Catalog::builtin().validate(&key).unwrap(), d);
            assert_eq!(SeriesKey::parse(&key.to_string()).unwrap(), key);
        }
    }

    #[test]
    fn covers_every_v1_metric() {
        // The 6.1 table, expanded. A change here is a change to the plan doc too.
        let expected = [
            "cpu.total",
            "cpu.user",
            "cpu.system",
            "cpu.load",
            "cpu.loadavg",
            "cpu.cluster.freq",
            "cpu.cluster.active",
            "cpu.cluster.residency",
            "cpu.cluster.power",
            "gpu.util",
            "gpu.render",
            "gpu.tiler",
            "gpu.freq",
            "gpu.residency",
            "mem.used",
            "mem.app",
            "mem.wired",
            "mem.compressed",
            "mem.cached",
            "mem.free",
            "mem.pressure",
            "mem.pressure_level",
            "mem.swap_used",
            "mem.swap_in",
            "mem.swap_out",
            "power.cpu",
            "power.gpu",
            "power.ane",
            "power.dram",
            "power.package",
            "power.system",
            "power.cpu_source",
            "thermal.zone",
            "thermal.cpu",
            "thermal.gpu",
            "thermal.hottest",
            "thermal.sensor",
            "thermal.state",
            "fan.rpm",
            "fan.max",
            "fan.mode",
            "net.rx",
            "net.tx",
            "net.link_rate",
            "net.rx_total",
            "net.tx_total",
            "disk.read",
            "disk.write",
            "disk.read_total",
            "disk.write_total",
            "disk.used",
            "disk.free",
            "disk.total",
            "battery.charge",
            "battery.charging",
            "battery.external",
            "battery.time_remaining",
            "battery.health",
            "battery.cycles",
            "battery.capacity_wh",
            "battery.design_wh",
            "battery.power",
            "battery.temp",
            "self.cpu",
        ];
        let have: BTreeSet<_> = CATALOG.iter().map(|d| d.id.as_str()).collect();
        let want: BTreeSet<_> = expected.into_iter().collect();
        assert_eq!(have, want);
    }

    #[test]
    fn non_persisted_set_matches_plan() {
        let ring_only: BTreeSet<_> = CATALOG
            .iter()
            .filter(|d| !d.persisted)
            .map(|d| d.id.as_str())
            .collect();
        let want: BTreeSet<_> = [
            "cpu.cluster.residency",
            "gpu.residency",
            "power.cpu_source",
            "fan.max",
            "fan.mode",
            "net.link_rate",
            "battery.time_remaining",
            "battery.capacity_wh",
            "battery.design_wh",
        ]
        .into_iter()
        .collect();
        assert_eq!(ring_only, want);
    }

    #[test]
    fn validate_rejects_unknown_and_mislabelled() {
        let c = Catalog::builtin();
        let unknown = SeriesKey::parse("gpu.quantum").unwrap();
        assert!(matches!(
            c.validate(&unknown),
            Err(CatalogError::UnknownMetric(_))
        ));
        let missing_label = SeriesKey::parse("cpu.load").unwrap();
        assert!(matches!(
            c.validate(&missing_label),
            Err(CatalogError::LabelMismatch { .. })
        ));
        let wrong_label = SeriesKey::parse("cpu.load{cpu=1}").unwrap();
        assert!(matches!(
            c.validate(&wrong_label),
            Err(CatalogError::LabelMismatch { .. })
        ));
        let extra_label = SeriesKey::parse("cpu.total{core=1}").unwrap();
        assert!(c.validate(&extra_label).is_err());
    }

    #[test]
    fn unseen_module_decodes_as_unknown() {
        let mut buf = Vec::new();
        ciborium::into_writer(&"npu", &mut buf).unwrap();
        assert_eq!(
            ciborium::from_reader::<Module, _>(buf.as_slice()).unwrap(),
            Module::Unknown
        );
        assert_eq!(Module::parse("unknown"), None);
        assert!(!Module::ALL.contains(&Module::Unknown));
    }

    #[test]
    fn module_text_round_trip() {
        for m in Module::ALL {
            assert_eq!(Module::parse(m.as_str()), Some(m));
            assert_eq!(
                serde_json::to_string(&m).unwrap(),
                format!("\"{}\"", m.as_str())
            );
        }
    }
}

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
    /// 0 to 100. Per-process and per-cgroup CPU is percent of one core and can exceed 100.
    Percent,
    Hz,
    Watts,
    WattHours,
    Celsius,
    Bytes,
    BytesPerSec,
    BitsPerSec,
    PagesPerSec,
    OperationsPerSec,
    MillisecondsPerSec,
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

pub use crate::module::Module;
pub(crate) use crate::module::deserialize_known_modules;

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

    /// The same definition with at most `n` series per host.
    const fn cap(mut self, n: u16) -> Self {
        self.max_cardinality = Some(n);
        self
    }
}

use MetricKind::{Gauge, Mean, Rate};
use Module as M;
use Unit as U;

const P: bool = true; // persisted
const R: bool = false; // ring buffer only
/// The labels of every `cgroup.*` series: `kind` is service, docker, podman, user or
/// other; `name` the unit or container name (D-115).
const CG: &[&str] = &["kind", "name"];

/// The v1 metric catalog (v1-local-monitor.md 6.1).
pub static CATALOG: &[MetricDef] = &[
    // CPU. `cpu.total` is all non-idle time: on Linux every `/proc/stat` field except
    // idle and iowait, the macOS meaning (D-115).
    def("cpu.total", M::Cpu, U::Percent, Mean, &[], P),
    def("cpu.user", M::Cpu, U::Percent, Mean, &[], P),
    def("cpu.system", M::Cpu, U::Percent, Mean, &[], P),
    // Linux only (D-115): idle time waiting on I/O, and time a hypervisor took.
    def("cpu.iowait", M::Cpu, U::Percent, Mean, &[], P),
    def("cpu.steal", M::Cpu, U::Percent, Mean, &[], P),
    // Pressure stall information, Linux only (D-115): the share of the span some
    // (`stall=some`) or all (`stall=full`) tasks were stalled, from deltas of the PSI
    // `total` counter, never its averages. The memory and disk pairs follow.
    def("cpu.psi", M::Cpu, U::Percent, Mean, &["stall"], P).every(5),
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
    def("mem.psi", M::Memory, U::Percent, Mean, &["stall"], P).every(5),
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
    // A board power sensor by name, watts only (hwmon `power*_input`; current and
    // voltage inputs are not power). Names follow `thermal.sensor` (D-115).
    def("power.sensor", M::Power, U::Watts, Gauge, &["name"], P).every(5),
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
    // From CPU/SoC-class sensors only; absent where there are none (D-115).
    def("thermal.hottest", M::Sensors, U::Celsius, Gauge, &[], P).every(5),
    // On Linux `name` is the hwmon kernel label ("NAND Flash Temperature"), or
    // `<chip>/<label>` when the label is generic (`Composite`) or another chip has it,
    // and `<chip>/tempN` without one; thermal zones are `zone/<type>`. A chip name that
    // repeats is qualified by its parent device (`nvme0`, an i2c address), never by
    // enumeration order, so a name survives a reboot (D-115).
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
    def(
        "disk.read_ops",
        M::Disk,
        U::OperationsPerSec,
        Rate,
        &["dev"],
        P,
    ),
    def(
        "disk.write_ops",
        M::Disk,
        U::OperationsPerSec,
        Rate,
        &["dev"],
        P,
    ),
    def(
        "disk.read_timed_ops",
        M::Disk,
        U::OperationsPerSec,
        Rate,
        &["dev"],
        P,
    ),
    def(
        "disk.write_timed_ops",
        M::Disk,
        U::OperationsPerSec,
        Rate,
        &["dev"],
        P,
    ),
    def(
        "disk.read_time",
        M::Disk,
        U::MillisecondsPerSec,
        Rate,
        &["dev"],
        P,
    ),
    def(
        "disk.write_time",
        M::Disk,
        U::MillisecondsPerSec,
        Rate,
        &["dev"],
        P,
    ),
    // The sum over the devices `disk.read`/`disk.write` report, with the network
    // totals' gap rule (D-092).
    def("disk.read_total", M::Disk, U::BytesPerSec, Rate, &[], P),
    def("disk.write_total", M::Disk, U::BytesPerSec, Rate, &[], P),
    def(
        "disk.read_ops_total",
        M::Disk,
        U::OperationsPerSec,
        Rate,
        &[],
        P,
    ),
    def(
        "disk.write_ops_total",
        M::Disk,
        U::OperationsPerSec,
        Rate,
        &[],
        P,
    ),
    def(
        "disk.read_timed_ops_total",
        M::Disk,
        U::OperationsPerSec,
        Rate,
        &[],
        P,
    ),
    def(
        "disk.write_timed_ops_total",
        M::Disk,
        U::OperationsPerSec,
        Rate,
        &[],
        P,
    ),
    def(
        "disk.read_time_total",
        M::Disk,
        U::MillisecondsPerSec,
        Rate,
        &[],
        P,
    ),
    def(
        "disk.write_time_total",
        M::Disk,
        U::MillisecondsPerSec,
        Rate,
        &[],
        P,
    ),
    def("disk.psi", M::Disk, U::Percent, Mean, &["stall"], P).every(5),
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
    // Containers: cgroup v2 groups on a Linux host (D-115). The top 32 by CPU plus
    // memory keep their own series; the rest fold into `{kind=other,name=_other}`, and
    // `cgroup.other_count` says how many groups that is. CPU is percent of one core,
    // like a process row; `mem_psi` is the `some` share of `memory.pressure`.
    def("cgroup.cpu", M::Containers, U::Percent, Mean, CG, P)
        .every(5)
        .cap(32),
    def("cgroup.mem", M::Containers, U::Bytes, Gauge, CG, P)
        .every(5)
        .cap(32),
    def("cgroup.mem_max", M::Containers, U::Bytes, Gauge, CG, P)
        .every(5)
        .cap(32),
    def("cgroup.io_read", M::Containers, U::BytesPerSec, Rate, CG, P)
        .every(5)
        .cap(32),
    def(
        "cgroup.io_write",
        M::Containers,
        U::BytesPerSec,
        Rate,
        CG,
        P,
    )
    .every(5)
    .cap(32),
    def("cgroup.mem_psi", M::Containers, U::Percent, Mean, CG, P)
        .every(5)
        .cap(32),
    def("cgroup.other_count", M::Containers, U::Count, Gauge, &[], P).every(5),
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
            "cpu.iowait",
            "cpu.steal",
            "cpu.psi",
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
            "mem.psi",
            "power.cpu",
            "power.gpu",
            "power.ane",
            "power.dram",
            "power.package",
            "power.system",
            "power.cpu_source",
            "power.sensor",
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
            "disk.read_ops",
            "disk.write_ops",
            "disk.read_timed_ops",
            "disk.write_timed_ops",
            "disk.read_time",
            "disk.write_time",
            "disk.read_total",
            "disk.write_total",
            "disk.read_ops_total",
            "disk.write_ops_total",
            "disk.read_timed_ops_total",
            "disk.write_timed_ops_total",
            "disk.read_time_total",
            "disk.write_time_total",
            "disk.psi",
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
            "cgroup.cpu",
            "cgroup.mem",
            "cgroup.mem_max",
            "cgroup.io_read",
            "cgroup.io_write",
            "cgroup.mem_psi",
            "cgroup.other_count",
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

    /// D-115: only the container groups are capped, all at 32, and they are all filed
    /// under Containers.
    #[test]
    fn cgroup_metrics_are_capped_containers() {
        for d in CATALOG {
            let cgroup = d.id.as_str().starts_with("cgroup.");
            assert_eq!(cgroup, d.module == Module::Containers, "{}", d.id);
            let want = (cgroup && !d.label_keys.is_empty()).then_some(32);
            assert_eq!(d.max_cardinality, want, "{}", d.id);
        }
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
}

//! SMC collectors: system and CPU power (`power.system`, `power.cpu`,
//! `cpu.cluster.power`) and fans (`fan.rpm`, `fan.max`, `fan.mode`). Temperatures from
//! the SMC are in `sensors.rs`.
//!
//! Keys, as read on the development M3 Max (macOS 27.0.1):
//! - `PSTR` (`flt `, W): total system power. 7 to 78 W idle to loaded, consistent with
//!   the IOReport component sum. Unverified on other chips.
//! - CPU power per P cluster ([`cpu_power_map`], D-054): `PC02` + `PC03` (P cluster 0),
//!   `PC42` + `PC43` (P cluster 1), `flt ` W, updated every second. Found by a read-only
//!   scan of all 2,877 keys under alternating CPU, E-core and GPU load, and checked
//!   against the PMP cluster energy counters over their 5-minute refresh windows. They
//!   read about 25% low, so they are scaled live to PMP ([`crate::calib`]). No key tracks
//!   the E cluster, so `power.cpu` from the SMC is the P clusters only, and
//!   `power.cpu_source` says so. Other chips have no map and keep IOReport's PMP path.
//! - `FNum` (`ui8 `): fan count. 2 on the M3 Max MacBook Pro; 0 or missing on fanless
//!   Macs, which report `NotPresent`.
//! - `F<n>Ac` / `F<n>Mx` (`flt `, rpm): actual and maximum speed (1359 and 5349 rpm).
//! - `F<n>Md` (`ui8 `): 0 = automatic, 1 = forced (unverified meaning; observed 0).

use kelvo_schema::{
    CpuPowerCalibration, Entitlement, Labels, MetricCode, MetricId, Module, SeriesKey,
};

use super::CpuPowerSource;
use super::sensors::{Chip, Tier, parse_chip};
use super::vendor::smc::{Smc, four_cc, four_cc_bytes};
use super::vendor::soc;
use crate::calib::CalibState;
use crate::{Cadence, CollectError, Collector, CollectorId, Every, Probe, SampleBuf, Tick};

pub const POWER_ID: CollectorId = CollectorId("smc.power");
pub const FANS_ID: CollectorId = CollectorId("smc.fans");

const PSTR: u32 = four_cc_bytes(b"PSTR");
const FNUM: u32 = four_cc_bytes(b"FNum");

/// `fan.max` and `fan.mode` are sampled at most every 60 s (v1-local-monitor.md 6.1).
const SLOW_MS: u32 = 60_000;
/// More than this many fans means the key read garbage.
const MAX_FANS: u32 = 8;

/// Plausible system power: anything outside this is a bad read, not a reading.
fn plausible_watts(w: f32) -> bool {
    w.is_finite() && (0.0..2_000.0).contains(&w)
}

fn plausible_rpm(rpm: f32) -> bool {
    rpm.is_finite() && (0.0..=100_000.0).contains(&rpm)
}

/// SMC keys that carry CPU power on one chip, per P cluster. A cluster's power is the sum
/// of its keys. The labels are the ones the IOReport collector gives the same clusters
/// (`PCPU` -> P0, `PCPU1` -> P1), so `cpu.cluster.power{cluster}` lines up with
/// `cpu.cluster.freq{cluster}`. E clusters are left out: no SMC key tracks them (`PPMC`
/// reads 4 to 6 times the E cluster's PMP energy, D-054), so they have no power series
/// on these chips.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CpuPowerMap {
    pub clusters: &'static [(&'static str, &'static [&'static str])],
    /// The SMC-to-PMP scale measured on this chip, used before any calibration (no scale
    /// stored from an earlier session).
    pub default_scale: Option<f64>,
}

/// M3 Max (verified on Mac15,9, macOS 27.0.1; D-054). The default scale is 1 / 0.75,
/// the ratio the SMC keys read against PMP over D-054's refresh windows (0.75 to 0.79).
const M3_MAX_CPU_POWER: CpuPowerMap = CpuPowerMap {
    clusters: &[("P0", &["PC02", "PC03"]), ("P1", &["PC42", "PC43"])],
    default_scale: Some(1.33),
};

/// The CPU power keys for a chip, or `None` where none are verified. Those chips keep
/// IOReport's PMP path for `power.cpu`.
pub fn cpu_power_map(chip: Chip) -> Option<CpuPowerMap> {
    match (chip.generation, chip.tier) {
        (3, Tier::Max) => Some(M3_MAX_CPU_POWER),
        _ => None,
    }
}

/// One cluster's power from its key readings. `None` if any key is missing or
/// implausible: a partial sum would read low rather than be missing.
fn cluster_watts(values: impl IntoIterator<Item = Option<f32>>) -> Option<f32> {
    let mut sum = 0.0;
    for v in values {
        sum += v.filter(|w| plausible_watts(*w))?;
    }
    Some(sum)
}

struct CpuCluster {
    key: SeriesKey,
    smc_keys: Vec<u32>,
}

struct CpuPower {
    total: SeriesKey,
    status: SeriesKey,
    clusters: Vec<CpuCluster>,
    /// This tick's cluster readings, kept to avoid an allocation per tick.
    watts: Vec<Option<f32>>,
}

impl CpuPower {
    /// The map's keys as cluster groups, if every key reads a plausible value now.
    fn probe(smc: &mut Smc, map: CpuPowerMap) -> Option<Self> {
        let m = MetricId::from_static;
        let mut clusters = Vec::new();
        for (label, names) in map.clusters {
            let smc_keys: Vec<u32> = names.iter().filter_map(|n| four_cc(n)).collect();
            cluster_watts(smc_keys.iter().map(|k| smc.read_f32(*k)))?;
            clusters.push(CpuCluster {
                key: SeriesKey::new(m("cpu.cluster.power"), Labels::single("cluster", label)),
                smc_keys,
            });
        }
        (!clusters.is_empty()).then(|| Self {
            total: SeriesKey::bare(m("power.cpu")),
            status: SeriesKey::bare(m("power.cpu_source")),
            watts: Vec::with_capacity(clusters.len()),
            clusters,
        })
    }

    fn series(&self) -> impl Iterator<Item = SeriesKey> + '_ {
        [&self.total, &self.status]
            .into_iter()
            .chain(self.clusters.iter().map(|c| &c.key))
            .cloned()
    }

    /// Reads the clusters, feeds the P-cluster total to the calibrator and pushes the
    /// readings times its scale (seeded, calibrated, or unscaled without either).
    fn sample(&mut self, smc: &mut Smc, t_ns: u64, source: &CpuPowerSource, out: &mut SampleBuf) {
        self.watts.clear();
        for c in &self.clusters {
            self.watts
                .push(cluster_watts(c.smc_keys.iter().map(|k| smc.read_f32(*k))));
        }
        // A missing cluster makes the total missing, not low.
        let total = self.watts.iter().try_fold(0.0_f32, |t, w| w.map(|w| t + w));
        let state = {
            let mut cal = source.calib();
            cal.fast(total.map(f64::from), t_ns);
            cal.state()
        };
        let k = state.factor() as f32;
        for (c, w) in self.clusters.iter().zip(&self.watts) {
            if let Some(w) = w {
                out.push(&c.key, w * k);
            }
        }
        if let Some(t) = total {
            out.push(&self.total, t * k);
        }
        let status = match state {
            CalibState::Uncalibrated => CpuPowerCalibration::Uncalibrated,
            CalibState::Seeded(_) => CpuPowerCalibration::Seeded,
            CalibState::Calibrated(_) => CpuPowerCalibration::Calibrated,
        };
        out.push(&self.status, status.value());
    }
}

/// `power.system` from SMC `PSTR`, and on chips with a [`CpuPowerMap`], `power.cpu` (P
/// clusters), `cpu.cluster.power{cluster}` and `power.cpu_source` every tick (D-054). The
/// readings are instantaneous watts, so it stays on every tick with nothing on screen
/// (D-070), which also keeps the calibrator's fast steps under `MAX_FAST_STEP_NS`.
pub struct Power {
    smc: Option<Smc>,
    system: Option<SeriesKey>,
    cpu: Option<CpuPower>,
    source: CpuPowerSource,
    chip_override: Option<String>,
}

impl Power {
    pub fn new(source: CpuPowerSource) -> Self {
        Self {
            smc: None,
            system: None,
            cpu: None,
            source,
            chip_override: None,
        }
    }

    /// Probes as if the machine reported `brand` (tests of the unmapped-chip path on
    /// known hardware).
    pub fn with_brand(source: CpuPowerSource, brand: &str) -> Self {
        Self {
            chip_override: Some(brand.to_owned()),
            ..Self::new(source)
        }
    }
}

impl Collector for Power {
    fn id(&self) -> CollectorId {
        POWER_ID
    }

    fn cadence(&self) -> Cadence {
        // Every tick, also with nothing on screen: PSTR and the CPU power keys are instantaneous watts, so a
        // 10 s idle period would leave one point per S10 bucket and an inexact M1 average.
        // Only counter-derived collectors use LIVE_OR_IDLE (D-070).
        Cadence::EveryTick
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Power]
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::SmcUserClient]
    }

    fn probe(&mut self) -> Probe {
        self.smc = None;
        self.system = None;
        self.cpu = None;
        self.source.set_smc(false);
        self.source.calib().restart();
        let Ok(mut smc) = Smc::open() else {
            return Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::NoHardware,
            };
        };
        if smc.read_f32(PSTR).is_some_and(plausible_watts) {
            self.system = Some(SeriesKey::bare(MetricId::from_static("power.system")));
        }
        let brand = self
            .chip_override
            .clone()
            .or_else(soc::chip_name)
            .unwrap_or_default();
        let map = parse_chip(&brand).and_then(cpu_power_map);
        self.cpu = map.and_then(|map| CpuPower::probe(&mut smc, map));
        if let (Some(map), Some(_)) = (map, &self.cpu) {
            self.source.seed(&brand, map.default_scale);
        }
        let mut series: Vec<SeriesKey> = self.system.iter().cloned().collect();
        if let Some(cpu) = &self.cpu {
            series.extend(cpu.series());
        }
        if series.is_empty() {
            return Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::UnknownChip,
            };
        }
        self.source.set_smc(self.cpu.is_some());
        self.smc = Some(smc);
        Probe::Supported(series)
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let smc = self.smc.as_mut().ok_or(CollectError::NotProbed)?;
        if let Some(key) = &self.system
            && let Some(w) = smc.read_f32(PSTR).filter(|w| plausible_watts(*w))
        {
            out.push(key, w);
        }
        if let Some(cpu) = &mut self.cpu {
            cpu.sample(smc, tick.continuous_ns, &self.source, out);
        }
        Ok(())
    }
}

struct Fan {
    actual: u32,
    max: Option<u32>,
    mode: Option<u32>,
    rpm_key: SeriesKey,
    max_key: Option<SeriesKey>,
}

/// `fan.rpm{fan}`, `fan.max{fan}` and `fan.mode` from the SMC.
pub struct Fans {
    smc: Option<Smc>,
    fans: Vec<Fan>,
    mode_key: Option<SeriesKey>,
    slow: Every,
}

impl Default for Fans {
    fn default() -> Self {
        Self::new()
    }
}

impl Fans {
    pub fn new() -> Self {
        Self {
            smc: None,
            fans: Vec::new(),
            mode_key: None,
            slow: Every::new(SLOW_MS),
        }
    }
}

/// The `F<n><suffix>` key, for example `F0Ac`.
fn fan_key(n: u32, suffix: &str) -> Option<u32> {
    four_cc(&format!("F{n}{suffix}"))
}

impl Collector for Fans {
    fn id(&self) -> CollectorId {
        FANS_ID
    }

    fn cadence(&self) -> Cadence {
        Cadence::Every(2_000)
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Sensors]
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::SmcUserClient]
    }

    fn probe(&mut self) -> Probe {
        self.smc = None;
        self.fans.clear();
        self.mode_key = None;
        self.slow.reset();
        let Ok(mut smc) = Smc::open() else {
            return Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::NoHardware,
            };
        };
        let count = smc
            .read_f32(FNUM)
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map_or(0, |n| n as u32)
            .min(MAX_FANS);
        let m = MetricId::from_static;
        for n in 0..count {
            let Some(actual) = fan_key(n, "Ac") else {
                continue;
            };
            if !smc.read_f32(actual).is_some_and(plausible_rpm) {
                continue;
            }
            let label = Labels::single("fan", &n.to_string());
            let max = fan_key(n, "Mx").filter(|k| smc.read_f32(*k).is_some_and(plausible_rpm));
            let mode = fan_key(n, "Md").filter(|k| smc.read_f32(*k).is_some());
            self.fans.push(Fan {
                actual,
                max,
                mode,
                rpm_key: SeriesKey::new(m("fan.rpm"), label.clone()),
                max_key: max.map(|_| SeriesKey::new(m("fan.max"), label)),
            });
        }
        if self.fans.is_empty() {
            return Probe::NotPresent;
        }
        if self.fans.iter().any(|f| f.mode.is_some()) {
            self.mode_key = Some(SeriesKey::bare(m("fan.mode")));
        }
        self.smc = Some(smc);
        let mut series: Vec<SeriesKey> = Vec::new();
        for f in &self.fans {
            series.push(f.rpm_key.clone());
            series.extend(f.max_key.iter().cloned());
        }
        series.extend(self.mode_key.iter().cloned());
        Probe::Supported(series)
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let smc = self.smc.as_mut().ok_or(CollectError::NotProbed)?;
        let slow = self.slow.due(tick);
        // fan.mode has no fan label: 1 if any fan is under forced control, else 0.
        let mut mode: Option<f32> = None;
        for f in &self.fans {
            if let Some(rpm) = smc.read_f32(f.actual).filter(|r| plausible_rpm(*r)) {
                out.push(&f.rpm_key, rpm);
            }
            if !slow {
                continue;
            }
            if let (Some(k), Some(key)) = (f.max, &f.max_key)
                && let Some(rpm) = smc.read_f32(k).filter(|r| plausible_rpm(*r))
            {
                out.push(key, rpm);
            }
            if let Some(md) = f.mode.and_then(|k| smc.read_f32(k)) {
                let forced = if md > 0.0 { 1.0 } else { 0.0 };
                mode = Some(mode.map_or(forced, |m: f32| m.max(forced)));
            }
        }
        if let (Some(key), Some(v)) = (&self.mode_key, mode) {
            out.push(key, v);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fan_keys() {
        assert_eq!(fan_key(0, "Ac"), four_cc("F0Ac"));
        assert_eq!(fan_key(10, "Ac"), None, "five characters is not an SMC key");
    }

    #[test]
    fn plausibility_bounds_reject_garbage() {
        assert!(plausible_watts(24.7));
        assert!(!plausible_watts(-1.0));
        assert!(!plausible_watts(f32::NAN));
        assert!(!plausible_watts(1e9));
        assert!(plausible_rpm(0.0), "a stopped fan is a reading");
        assert!(!plausible_rpm(-5.0));
        assert!(!plausible_rpm(f32::INFINITY));
    }

    #[test]
    fn cpu_power_map_only_for_verified_chips() {
        let map = |b: &str| parse_chip(b).and_then(cpu_power_map);
        let m3_max = map("Apple M3 Max").unwrap();
        let labels: Vec<&str> = m3_max.clusters.iter().map(|(l, _)| *l).collect();
        assert_eq!(labels, ["P0", "P1"], "no SMC key tracks the E cluster");
        for (_, keys) in m3_max.clusters {
            for k in *keys {
                assert!(four_cc(k).is_some(), "{k} is a four-character key");
            }
        }
        for unverified in ["Apple M3 Pro", "Apple M3", "Apple M4 Max", "Apple M1"] {
            assert_eq!(map(unverified), None, "{unverified}");
        }
    }

    #[test]
    fn cluster_power_is_missing_rather_than_low() {
        assert_eq!(cluster_watts([Some(4.5), Some(0.5)]), Some(5.0));
        assert_eq!(
            cluster_watts([Some(0.0)]),
            Some(0.0),
            "an idle cluster is 0 W"
        );
        // One unreadable or garbage key drops the whole cluster.
        assert_eq!(cluster_watts([Some(4.5), None]), None);
        assert_eq!(cluster_watts([Some(4.5), Some(-1.0)]), None);
        assert_eq!(cluster_watts([Some(f32::NAN)]), None);
        assert_eq!(cluster_watts([Some(1e9)]), None);
    }

    #[test]
    fn reprobe_on_unmapped_chip_releases_cpu_power() {
        // Without SMC access (CI) the probe fails before the map; with it, an unmapped
        // chip keeps only `power.system`. Either way IOReport keeps CPU power.
        // A previous probe (before a wake or a module re-probe) had claimed it; the new
        // probe must clear the claim, or IOReport would drop CPU power with no SMC source.
        let source = CpuPowerSource::default();
        source.set_smc(true);
        let mut p = Power::with_brand(source.clone(), "Apple M9 Max");
        if let Probe::Supported(series) = p.probe() {
            assert!(series.iter().all(|k| k.metric.as_str() == "power.system"));
        }
        assert!(p.cpu.is_none());
        assert!(!source.smc());
    }

    /// Samples system power, CPU power and fans twice on real hardware.
    #[test]
    #[ignore = "needs Apple Silicon hardware; run by hand with --ignored --nocapture"]
    fn live_smc_power_and_fans() {
        let source = CpuPowerSource::default();
        let mut power = Power::new(source.clone());
        let mut fans = Fans::new();
        println!("power probe: {:?}", power.probe());
        println!("SMC supplies CPU power: {}", source.smc());
        println!("fans probe: {:?}", fans.probe());
        // Tick 0 and 60 both carry the slow fan.max / fan.mode series.
        for n in [0, 60] {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let tick = Tick {
                n,
                wall_ms: 0,
                continuous_ns: 0,
                interval_ms: 1_000,
            };
            let mut out = SampleBuf::new();
            power.sample(&tick, &mut out).unwrap();
            if fans.smc.is_some() {
                fans.sample(&tick, &mut out).unwrap();
            }
            for s in out.values() {
                println!("  {} = {:.2}", s.key, s.value);
                assert!(s.value >= 0.0, "{}", s.key);
            }
            let w = out
                .get(power.system.as_ref().expect("PSTR"))
                .expect("power.system");
            assert!(w > 0.5 && w < 400.0, "{w} W");
            if let Some(cpu) = &power.cpu {
                let total = out.get(&cpu.total).expect("power.cpu");
                let clusters: f32 = cpu.clusters.iter().filter_map(|c| out.get(&c.key)).sum();
                assert!((total - clusters).abs() < 1e-3, "{total} vs {clusters}");
                assert!(total < w, "CPU {total} W is part of system {w} W");
            }
        }
    }
}

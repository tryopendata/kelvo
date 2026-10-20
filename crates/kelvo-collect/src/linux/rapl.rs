//! RAPL energy counters from the powercap framework (D-116): `power.package` (the sum
//! over `package-N` zones) and `power.dram` (the sum over `dram` subzones), watts
//! averaged over the span since the previous sample. Intel and AMD (Zen) both register
//! as `intel-rapl:*`; the `intel-rapl-mmio:*` duplicate of the package zone is left out.
//! Other zones (`core`, `uncore`, `psys`) have no catalog id yet and are not read.
//!
//! `energy_uj` wraps at `max_energy_range_uj`; a decrease is taken as one wrap when the
//! previous value was within the range, otherwise the sample has no value. Since Linux
//! 5.10 `energy_uj` is root-only (CVE-2020-8694), so a host with zones whose counters
//! this process cannot read probes `Unsupported { PermissionDenied }`; a host without
//! powercap zones is `NotPresent` (Apple Silicon, most ARM boards, many VMs).

use std::path::{Path, PathBuf};

use kelvo_schema::{Entitlement, MetricId, Module, SeriesKey, UnsupportedReason};

use super::SysRoot;
use super::counters::{PartSum, read_u64_at, span_secs};
use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

/// Watts above this are a counter glitch.
const MAX_PLAUSIBLE_W: f64 = 5_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Domain {
    Package,
    Dram,
}

struct Zone {
    domain: Domain,
    energy: PathBuf,
    /// `max_energy_range_uj`, 0 when unreadable (a decrease is then a reset).
    range: u64,
    prev: Option<u64>,
}

/// Energy delta in µJ across one wrap at `range`; `None` for a reset.
fn energy_delta(prev: u64, cur: u64, range: u64) -> Option<u64> {
    if cur >= prev {
        Some(cur - prev)
    } else if range > 0 && prev <= range {
        Some(range - prev + cur)
    } else {
        None
    }
}

/// The zone directories with a domain this collector reads, sorted.
fn zones(root: &SysRoot) -> Vec<(Domain, PathBuf, u64)> {
    let base = root.path("/sys/class/powercap");
    let mut dirs: Vec<String> = std::fs::read_dir(&base)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|d| d.starts_with("intel-rapl:"))
        .collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|d| {
            let name = root.read_trimmed(&format!("/sys/class/powercap/{d}/name"))?;
            let domain = if name.starts_with("package-") {
                Domain::Package
            } else if name == "dram" {
                Domain::Dram
            } else {
                return None;
            };
            let range = root
                .read_u64(&format!("/sys/class/powercap/{d}/max_energy_range_uj"))
                .unwrap_or(0);
            Some((domain, base.join(d).join("energy_uj"), range))
        })
        .collect()
}

/// Whether `energy_uj` is there but this process may not read it.
fn denied(path: &Path) -> bool {
    matches!(std::fs::read_to_string(path), Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied)
}

/// `power.package` and `power.dram` from RAPL.
pub struct Rapl {
    root: SysRoot,
    zones: Vec<Zone>,
    prev_ns: u64,
    package: SeriesKey,
    dram: SeriesKey,
}

impl Default for Rapl {
    fn default() -> Self {
        Self::new()
    }
}

impl Rapl {
    pub fn new() -> Self {
        Self::at(SysRoot::host())
    }

    fn at(root: SysRoot) -> Self {
        Self {
            root,
            zones: Vec::new(),
            prev_ns: 0,
            package: SeriesKey::bare(MetricId::from_static("power.package")),
            dram: SeriesKey::bare(MetricId::from_static("power.dram")),
        }
    }

    fn has(&self, d: Domain) -> bool {
        self.zones.iter().any(|z| z.domain == d)
    }
}

impl Collector for Rapl {
    fn id(&self) -> CollectorId {
        CollectorId("rapl")
    }

    fn cadence(&self) -> Cadence {
        // Counter-derived: span averages stay exact at the idle cadence (D-067).
        crate::LIVE_OR_IDLE
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Power]
    }

    fn probe(&mut self) -> Probe {
        self.prev_ns = 0;
        self.zones.clear();
        let found = zones(&self.root);
        if found.is_empty() {
            return Probe::NotPresent;
        }
        if found.iter().all(|(_, p, _)| denied(p)) {
            return Probe::Unsupported {
                reason: UnsupportedReason::PermissionDenied,
            };
        }
        self.zones = found
            .into_iter()
            .filter(|(_, p, _)| read_u64_at(p).is_some())
            .map(|(domain, energy, range)| Zone {
                domain,
                energy,
                range,
                prev: None,
            })
            .collect();
        if !self.has(Domain::Package) && !self.has(Domain::Dram) {
            return Probe::Unsupported {
                reason: UnsupportedReason::NoHardware,
            };
        }
        let mut series = Vec::new();
        if self.has(Domain::Package) {
            series.push(self.package.clone());
        }
        if self.has(Domain::Dram) {
            series.push(self.dram.clone());
        }
        Probe::Supported(series)
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let now = tick.continuous_ns;
        let secs = span_secs(self.prev_ns, now);
        let mut package = PartSum::default();
        let mut dram = PartSum::default();
        for z in &mut self.zones {
            let cur = read_u64_at(&z.energy);
            let watts = match (z.prev, cur, secs) {
                (Some(p), Some(c), Some(s)) => energy_delta(p, c, z.range)
                    .map(|d| d as f64 / 1e6 / s)
                    .filter(|w| *w <= MAX_PLAUSIBLE_W)
                    .map(|w| w as f32),
                _ => None,
            };
            match z.domain {
                Domain::Package => package.add(watts),
                Domain::Dram => dram.add(watts),
            }
            z.prev = cur;
        }
        if secs.is_some() {
            if let Some(w) = package.total().filter(|_| self.has(Domain::Package)) {
                out.push(&self.package, w);
            }
            if let Some(w) = dram.total().filter(|_| self.has(Domain::Dram)) {
                out.push(&self.dram, w);
            }
        }
        self.prev_ns = now;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testdata::{asahi_t0, tree, write};
    use super::*;

    fn tick(secs: u64) -> Tick {
        Tick {
            n: secs,
            wall_ms: 0,
            continuous_ns: secs * 1_000_000_000,
            interval_ms: 1_000,
        }
    }

    fn zone(dir: &tempfile::TempDir, z: &str, name: &str, energy: u64, range: u64) {
        let base = format!("sys/class/powercap/{z}");
        write(dir, &format!("{base}/name"), &format!("{name}\n"));
        write(dir, &format!("{base}/energy_uj"), &format!("{energy}\n"));
        write(
            dir,
            &format!("{base}/max_energy_range_uj"),
            &format!("{range}\n"),
        );
    }

    #[test]
    fn asahi_has_no_powercap() {
        assert_eq!(Rapl::at(asahi_t0()).probe(), Probe::NotPresent);
    }

    /// Two packages and a DRAM zone over 2 s, with package-1 wrapping at its range;
    /// `core` and the mmio duplicate are not counted.
    #[test]
    fn sums_packages_and_dram_across_a_wrap() {
        let range = 262_143_328_850;
        let dir = tree(&[]);
        zone(&dir, "intel-rapl:0", "package-0", 1_000_000, range);
        zone(&dir, "intel-rapl:0:0", "core", 0, range);
        zone(&dir, "intel-rapl:0:1", "dram", 500_000, range);
        zone(&dir, "intel-rapl:1", "package-1", range - 4_000_000, range);
        zone(&dir, "intel-rapl-mmio:0", "package-0", 0, range);
        let mut c = Rapl::at(SysRoot::at(dir.path()));
        let Probe::Supported(series) = c.probe() else {
            panic!("probe");
        };
        assert_eq!(series, [c.package.clone(), c.dram.clone()]);
        let mut buf = SampleBuf::new();
        c.sample(&tick(10), &mut buf).unwrap();
        assert!(buf.values().is_empty(), "baseline");
        zone(&dir, "intel-rapl:0", "package-0", 31_000_000, range);
        zone(&dir, "intel-rapl:0:0", "core", 999_000_000, range);
        zone(&dir, "intel-rapl:0:1", "dram", 4_500_000, range);
        zone(&dir, "intel-rapl:1", "package-1", 6_000_000, range);
        zone(&dir, "intel-rapl-mmio:0", "package-0", 999_000_000, range);
        c.sample(&tick(12), &mut buf).unwrap();
        // (30 J + 10 J across the wrap) / 2 s, and 4 J / 2 s.
        assert_eq!(buf.get(&c.package), Some(20.0));
        assert_eq!(buf.get(&c.dram), Some(2.0));
    }

    #[test]
    fn decrease_without_range_is_a_reset() {
        assert_eq!(energy_delta(10, 25, 100), Some(15));
        assert_eq!(energy_delta(90, 5, 100), Some(15));
        assert_eq!(energy_delta(90, 5, 0), None);
        assert_eq!(energy_delta(200, 5, 100), None, "previous beyond the range");
    }

    /// A one-package host whose counter goes unreadable mid-run records a gap, not 0.
    #[test]
    fn unreadable_part_is_a_gap() {
        let dir = tree(&[]);
        zone(&dir, "intel-rapl:0", "package-0", 0, 1 << 32);
        let mut c = Rapl::at(SysRoot::at(dir.path()));
        c.probe();
        let mut buf = SampleBuf::new();
        c.sample(&tick(1), &mut buf).unwrap();
        write(&dir, "sys/class/powercap/intel-rapl:0/energy_uj", "garbage");
        c.sample(&tick(2), &mut buf).unwrap();
        assert!(buf.values().is_empty());
    }

    /// Root reads anything, so the denied path only shows unprivileged. Mutation check:
    /// the same tree readable probes Supported.
    #[test]
    fn unreadable_counters_are_permission_denied() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tree(&[]);
        zone(&dir, "intel-rapl:0", "package-0", 1, 1 << 32);
        let path = dir.path().join("sys/class/powercap/intel-rapl:0/energy_uj");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let mut c = Rapl::at(SysRoot::at(dir.path()));
        if std::fs::read(&path).is_ok() {
            eprintln!("skipped: running as root, mode bits do not apply");
        } else {
            assert_eq!(
                c.probe(),
                Probe::Unsupported {
                    reason: UnsupportedReason::PermissionDenied
                }
            );
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
        assert!(matches!(c.probe(), Probe::Supported(s) if s.len() == 1));
    }
}

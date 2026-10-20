//! Per-cluster CPU frequency, `cpu.cluster.freq{cluster}` in Hz, every 5 s, from each
//! cpufreq policy's `scaling_cur_freq` (kHz).
//!
//! The `cluster` labels are [`super::topology`]'s (`E0`, `P0`, `C0`), the names in
//! `HostInfo::cpu_topology`. A cluster is one policy on a heterogeneous host (the M1's
//! `policy0` and `policy4`); a `Uniform` cluster can span several (acpi-cpufreq keeps
//! one per CPU), and its value is their mean.
//!
//! The value is the frequency the kernel reports at the read, not the residency-weighted
//! mean over active time that IOReport gives on macOS. A 5 s sample of it is a coarse
//! view of the same thing. Clusters with no readable `scaling_cur_freq` have no series;
//! with none at all (a VM without a cpufreq driver) the collector is `NotPresent`.

use std::path::PathBuf;

use kelvo_schema::{Entitlement, Labels, MetricId, Module, SeriesKey};

use super::SysRoot;
use super::counters::read_u64_at;
use super::topology::{clusters, policy_dirs};
use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

const PERIOD_MS: u32 = 5_000;

struct Cluster {
    key: SeriesKey,
    /// `scaling_cur_freq` of each policy in the cluster.
    files: Vec<PathBuf>,
}

impl Cluster {
    /// Mean of the readable policies, in Hz; `None` when none could be read.
    fn hz(&self) -> Option<f32> {
        let (mut sum, mut n) = (0u64, 0u64);
        for khz in self.files.iter().filter_map(|f| read_u64_at(f)) {
            sum = sum.saturating_add(khz);
            n += 1;
        }
        (n > 0).then(|| (sum as f64 / n as f64 * 1000.0) as f32)
    }
}

/// `cpu.cluster.freq{cluster}`.
pub struct CpuFreq {
    root: SysRoot,
    clusters: Vec<Cluster>,
}

impl Default for CpuFreq {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuFreq {
    pub fn new() -> Self {
        Self::at(SysRoot::host())
    }

    /// Reads the tree under `root` (tests: a fixture).
    pub fn at(root: SysRoot) -> Self {
        Self {
            root,
            clusters: Vec::new(),
        }
    }
}

impl Collector for CpuFreq {
    fn id(&self) -> CollectorId {
        CollectorId("cpufreq")
    }

    fn cadence(&self) -> Cadence {
        Cadence::Every(PERIOD_MS)
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Cpu]
    }

    fn probe(&mut self) -> Probe {
        let topo = clusters(&self.root);
        let cpus: Vec<usize> = topo.iter().flat_map(|c| c.cpus.iter().copied()).collect();
        let policies = policy_dirs(&self.root, &cpus);
        self.clusters = topo
            .iter()
            .filter_map(|c| {
                let files: Vec<PathBuf> = policies
                    .iter()
                    .filter(|(_, p)| p.iter().any(|cpu| c.cpus.contains(cpu)))
                    .map(|(base, _)| self.root.path(&format!("{base}/scaling_cur_freq")))
                    .filter(|f| read_u64_at(f).is_some())
                    .collect();
                let key = SeriesKey::new(
                    MetricId::from_static("cpu.cluster.freq"),
                    Labels::single("cluster", &c.info.name),
                );
                (!files.is_empty()).then_some(Cluster { key, files })
            })
            .collect();
        if self.clusters.is_empty() {
            return Probe::NotPresent;
        }
        Probe::Supported(self.clusters.iter().map(|c| c.key.clone()).collect())
    }

    fn sample(&mut self, _tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        for c in &self.clusters {
            if let Some(hz) = c.hz() {
                out.push(&c.key, hz);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    fn tick() -> Tick {
        Tick {
            n: 0,
            wall_ms: 0,
            continuous_ns: 1,
            interval_ms: 1_000,
        }
    }

    fn freq(cluster: &str) -> SeriesKey {
        SeriesKey::new(
            MetricId::from_static("cpu.cluster.freq"),
            Labels::single("cluster", cluster),
        )
    }

    #[test]
    fn asahi_clusters_in_hz() {
        let mut c = CpuFreq::at(testdata::asahi_t0());
        assert_eq!(c.probe(), Probe::Supported(vec![freq("E0"), freq("P0")]));
        let mut buf = SampleBuf::new();
        c.sample(&tick(), &mut buf).unwrap();
        // policy0 600000 kHz, policy4 1056000 kHz.
        assert_eq!(buf.get(&freq("E0")), Some(600e6));
        assert_eq!(buf.get(&freq("P0")), Some(1_056e6));
        assert_eq!(buf.values().len(), 2);
    }

    #[test]
    fn a_uniform_cluster_is_the_mean_of_its_policies() {
        let pol = |n: usize| format!("sys/devices/system/cpu/cpufreq/policy{n}");
        let (r0, r1, r2) = (pol(0), pol(1), pol(2));
        let dir = testdata::tree(&[
            ("sys/devices/system/cpu/online", "0-2\n"),
            (&format!("{r0}/related_cpus"), "0\n"),
            (&format!("{r0}/scaling_cur_freq"), "1800000\n"),
            (&format!("{r1}/related_cpus"), "1\n"),
            (&format!("{r1}/scaling_cur_freq"), "3000000\n"),
            // A policy whose frequency is unreadable is left out of the mean.
            (&format!("{r2}/related_cpus"), "2\n"),
        ]);
        let mut c = CpuFreq::at(SysRoot::at(dir.path()));
        assert_eq!(c.probe(), Probe::Supported(vec![freq("C0")]));
        let mut buf = SampleBuf::new();
        c.sample(&tick(), &mut buf).unwrap();
        assert_eq!(buf.get(&freq("C0")), Some(2_400e6));

        // A policy that stops answering drops out of the mean; none answering, no value.
        std::fs::remove_file(dir.path().join(format!("{r1}/scaling_cur_freq"))).unwrap();
        buf.clear();
        c.sample(&tick(), &mut buf).unwrap();
        assert_eq!(buf.get(&freq("C0")), Some(1_800e6));
        std::fs::remove_file(dir.path().join(format!("{r0}/scaling_cur_freq"))).unwrap();
        buf.clear();
        c.sample(&tick(), &mut buf).unwrap();
        assert!(buf.values().is_empty());
    }

    #[test]
    fn no_cpufreq_is_not_present() {
        let dir = testdata::tree(&[("sys/devices/system/cpu/online", "0-3\n")]);
        assert_eq!(
            CpuFreq::at(SysRoot::at(dir.path())).probe(),
            Probe::NotPresent
        );
        // The x86 fixture has policies but no `scaling_cur_freq`.
        assert_eq!(CpuFreq::at(testdata::x86()).probe(), Probe::NotPresent);
    }

    #[test]
    #[ignore = "reads the live cpufreq policies; run by hand on Linux"]
    fn live_smoke() {
        let mut c = CpuFreq::new();
        assert!(matches!(c.probe(), Probe::Supported(_)));
        let mut buf = SampleBuf::new();
        c.sample(&tick(), &mut buf).unwrap();
        for s in buf.values() {
            println!("{} = {} Hz", s.key, s.value);
        }
        assert!(!buf.values().is_empty());
    }
}

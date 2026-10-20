//! CPU clusters and core labels, shared by the CPU collector (`cpu.load{core}`) and the
//! host record (`HostInfo::cpu_topology`), so the two always agree.
//!
//! One cluster per cpufreq policy (its `related_cpus`), or per distinct `cpu_capacity`
//! without cpufreq. When every online CPU reports a capacity and they differ, clusters
//! are ranked by it: the lowest is `Efficiency`, the rest `Performance` (on an M1, 493
//! vs 1024). Otherwise the host is homogeneous: one `Uniform` cluster of every online
//! CPU. Core labels follow the macOS form: numbered within their kind in CPU order
//! (`E0..E3`, `P0..P3`, `C0..`); cluster names are the kind letter and the cluster's
//! index within its kind (`E0`, `P0`, `P1`), the `cluster` label of `cpu.cluster.*`.

use std::collections::BTreeMap;

use kelvo_schema::{ClusterInfo, CoreKind};

use super::SysRoot;
use super::procfs::{cpu_list, word};

const CPU: &str = "/sys/devices/system/cpu";

/// One cluster, with the logical CPU number of each core.
#[derive(Clone, Debug, PartialEq)]
pub struct Cluster {
    pub info: ClusterInfo,
    /// Logical CPU numbers, in the order of `info.cores`.
    pub cpus: Vec<usize>,
}

/// The online CPUs, from sysfs or, without it, the `cpuN` lines of `/proc/stat`.
fn online_cpus(root: &SysRoot) -> Vec<usize> {
    if let Some(text) = root.read_trimmed(&format!("{CPU}/online")) {
        let cpus = cpu_list(&text);
        if !cpus.is_empty() {
            return cpus;
        }
    }
    let stat = root.read("/proc/stat").unwrap_or_default();
    let mut cpus: Vec<usize> = stat
        .lines()
        .filter_map(|l| l.split_ascii_whitespace().next()?.strip_prefix("cpu"))
        .filter_map(|n| n.parse().ok())
        .collect();
    cpus.sort_unstable();
    cpus
}

/// cpufreq policies: their CPUs (online only) and DVFS table in MHz, ascending.
fn policies(root: &SysRoot, online: &[usize]) -> Vec<(Vec<usize>, Vec<u32>)> {
    let Ok(dir) = std::fs::read_dir(root.path(&format!("{CPU}/cpufreq"))) else {
        return Vec::new();
    };
    let mut names: Vec<String> = dir
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| {
            n.strip_prefix("policy")
                .is_some_and(|i| i.parse::<u32>().is_ok())
        })
        .collect();
    names.sort_by_key(|n| n.trim_start_matches("policy").parse::<u32>().unwrap_or(0));
    names
        .iter()
        .filter_map(|n| {
            let base = format!("{CPU}/cpufreq/{n}");
            let cpus: Vec<usize> = cpu_list(&root.read_trimmed(&format!("{base}/related_cpus"))?)
                .into_iter()
                .filter(|c| online.contains(c))
                .collect();
            if cpus.is_empty() {
                return None;
            }
            let mut mhz: Vec<u32> = root
                .read_trimmed(&format!("{base}/scaling_available_frequencies"))
                .unwrap_or_default()
                .split_ascii_whitespace()
                .filter_map(|khz| khz.parse::<u32>().ok())
                .map(|khz| khz / 1000)
                .filter(|&m| m > 0)
                .collect();
            mhz.sort_unstable();
            mhz.dedup();
            Some((cpus, mhz))
        })
        .collect()
}

/// The clusters of the host under `root`, in CPU order. Empty when no CPU is found.
pub fn clusters(root: &SysRoot) -> Vec<Cluster> {
    let online = online_cpus(root);
    if online.is_empty() {
        return Vec::new();
    }
    let capacity: Vec<Option<u64>> = online
        .iter()
        .map(|c| root.read_u64(&format!("{CPU}/cpu{c}/cpu_capacity")))
        .collect();
    let cap_of = |cpu: usize| {
        online
            .iter()
            .position(|&c| c == cpu)
            .and_then(|i| capacity.get(i).copied().flatten())
    };
    let mut groups = policies(root, &online);
    // Online CPUs no policy covers (no cpufreq driver, a VM): grouped by capacity.
    let mut rest: BTreeMap<Option<u64>, Vec<usize>> = BTreeMap::new();
    for &c in &online {
        if !groups.iter().any(|(cpus, _)| cpus.contains(&c)) {
            rest.entry(cap_of(c)).or_default().push(c);
        }
    }
    groups.extend(rest.into_values().map(|cpus| (cpus, Vec::new())));
    groups.sort_by_key(|(cpus, _)| cpus.first().copied());

    let mut levels: Vec<u64> = capacity.iter().flatten().copied().collect();
    levels.sort_unstable();
    levels.dedup();
    let heterogeneous = capacity.iter().all(Option::is_some) && levels.len() > 1;
    if !heterogeneous {
        return vec![uniform(&online, groups)];
    }
    let lowest = levels.first().copied();
    let mut counts = [0usize; 2];
    let mut core_counts = [0usize; 2];
    groups
        .into_iter()
        .map(|(cpus, dvfs_mhz)| {
            let cap = cpus.iter().filter_map(|&c| cap_of(c)).max();
            let (slot, letter, kind) = if cap == lowest {
                (0, 'E', CoreKind::Efficiency)
            } else {
                (1, 'P', CoreKind::Performance)
            };
            let name = format!("{letter}{}", counts[slot]);
            counts[slot] += 1;
            let cores = cpus
                .iter()
                .map(|_| {
                    let label = format!("{letter}{}", core_counts[slot]);
                    core_counts[slot] += 1;
                    label
                })
                .collect();
            Cluster {
                info: ClusterInfo {
                    name,
                    kind,
                    cores,
                    dvfs_mhz,
                },
                cpus,
            }
        })
        .collect()
}

/// Every online CPU as one `Uniform` cluster; its DVFS table is every policy's steps.
fn uniform(online: &[usize], groups: Vec<(Vec<usize>, Vec<u32>)>) -> Cluster {
    let mut dvfs_mhz: Vec<u32> = groups.into_iter().flat_map(|(_, mhz)| mhz).collect();
    dvfs_mhz.sort_unstable();
    dvfs_mhz.dedup();
    Cluster {
        info: ClusterInfo {
            name: "C0".into(),
            kind: CoreKind::Uniform,
            cores: (0..online.len()).map(|i| format!("C{i}")).collect(),
            dvfs_mhz,
        },
        cpus: online.to_vec(),
    }
}

/// `(logical CPU, core label)` for every core, in CPU order.
pub fn core_labels(root: &SysRoot) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = clusters(root)
        .into_iter()
        .flat_map(|c| c.cpus.into_iter().zip(c.info.cores))
        .collect();
    out.sort_unstable_by_key(|(cpu, _)| *cpu);
    out
}

/// Boot time from `/proc/stat` `btime`, in seconds since the epoch.
pub fn boot_time_s(root: &SysRoot) -> Option<u64> {
    word(&root.read("/proc/stat").ok()?, "btime")
}

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    #[test]
    fn asahi_m1_has_an_efficiency_and_a_performance_cluster() {
        let c = clusters(&testdata::asahi_t0());
        let infos: Vec<_> = c.iter().map(|c| &c.info).collect();
        assert_eq!(
            infos,
            [
                &ClusterInfo {
                    name: "E0".into(),
                    kind: CoreKind::Efficiency,
                    cores: vec!["E0".into(), "E1".into(), "E2".into(), "E3".into()],
                    dvfs_mhz: vec![600, 972, 1332, 1704, 2064],
                },
                &ClusterInfo {
                    name: "P0".into(),
                    kind: CoreKind::Performance,
                    cores: vec!["P0".into(), "P1".into(), "P2".into(), "P3".into()],
                    dvfs_mhz: vec![
                        600, 828, 1056, 1284, 1500, 1728, 1956, 2184, 2388, 2592, 2772, 2988
                    ],
                },
            ]
        );
        assert_eq!(c[1].cpus, [4, 5, 6, 7]);
        let labels = core_labels(&testdata::asahi_t0());
        assert_eq!(labels.first(), Some(&(0, "E0".to_owned())));
        assert_eq!(labels.get(7), Some(&(7, "P3".to_owned())));
    }

    #[test]
    fn x86_without_capacities_is_one_uniform_cluster() {
        let c = clusters(&testdata::x86());
        assert_eq!(c.len(), 1, "{c:?}");
        let info = &c[0].info;
        assert_eq!(info.kind, CoreKind::Uniform);
        assert_eq!(info.name, "C0");
        assert_eq!(info.cores, ["C0", "C1", "C2", "C3"]);
        // acpi-cpufreq lists its steps descending, per CPU; merged and ascending.
        assert_eq!(info.dvfs_mhz, [1800, 2400, 3000]);
        assert_eq!(c[0].cpus, [0, 1, 2, 3]);
    }

    #[test]
    fn equal_or_partial_capacities_are_uniform() {
        let dir = testdata::tree(&[
            ("sys/devices/system/cpu/online", "0-1\n"),
            ("sys/devices/system/cpu/cpu0/cpu_capacity", "1024\n"),
            ("sys/devices/system/cpu/cpu1/cpu_capacity", "1024\n"),
        ]);
        let c = clusters(&SysRoot::at(dir.path()));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].info.kind, CoreKind::Uniform);

        // Two capacities but one CPU without any: the ranking is not trusted.
        let dir = testdata::tree(&[
            ("sys/devices/system/cpu/online", "0-2\n"),
            ("sys/devices/system/cpu/cpu0/cpu_capacity", "512\n"),
            ("sys/devices/system/cpu/cpu1/cpu_capacity", "1024\n"),
        ]);
        let c = clusters(&SysRoot::at(dir.path()));
        assert_eq!(c[0].info.kind, CoreKind::Uniform);
    }

    #[test]
    fn capacities_without_cpufreq_group_by_capacity() {
        // Three tiers (a phone-style big.LITTLE.prime) and no cpufreq: only the lowest
        // tier is Efficiency; the P cores are numbered across both P clusters.
        let dir = testdata::tree(&[
            ("sys/devices/system/cpu/online", "0-3\n"),
            ("sys/devices/system/cpu/cpu0/cpu_capacity", "300\n"),
            ("sys/devices/system/cpu/cpu1/cpu_capacity", "300\n"),
            ("sys/devices/system/cpu/cpu2/cpu_capacity", "800\n"),
            ("sys/devices/system/cpu/cpu3/cpu_capacity", "1024\n"),
        ]);
        let c = clusters(&SysRoot::at(dir.path()));
        let summary: Vec<_> = c
            .iter()
            .map(|c| (c.info.name.as_str(), c.info.kind, c.info.cores.clone()))
            .collect();
        assert_eq!(
            summary,
            [
                ("E0", CoreKind::Efficiency, vec!["E0".into(), "E1".into()]),
                ("P0", CoreKind::Performance, vec!["P0".into()]),
                ("P1", CoreKind::Performance, vec!["P1".into()]),
            ]
        );
    }

    #[test]
    fn online_falls_back_to_proc_stat() {
        let dir = testdata::tree(&[(
            "proc/stat",
            "cpu  1 2 3 4\ncpu0 1 1 1 1\ncpu1 1 1 1 1\nintr 5\nbtime 17\n",
        )]);
        let root = SysRoot::at(dir.path());
        assert_eq!(clusters(&root)[0].info.cores, ["C0", "C1"]);
        assert_eq!(boot_time_s(&root), Some(17));
        assert!(clusters(&SysRoot::at("/nonexistent")).is_empty());
    }
}

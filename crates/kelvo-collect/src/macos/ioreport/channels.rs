//! Channel-name parsing: cluster, energy and idle-state names as IOReport spells them.

/// Kind letter of a CPU cluster as IOReport names it. `M` is the M5 middle tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ClusterKind {
    E,
    M,
    P,
}

impl ClusterKind {
    pub(super) fn letter(self) -> char {
        match self {
            ClusterKind::E => 'E',
            ClusterKind::M => 'M',
            ClusterKind::P => 'P',
        }
    }
}

/// A cluster as named by IOReport: kind, die (Ultra) and index within the die.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ClusterId {
    pub(super) kind: ClusterKind,
    pub(super) die: u32,
    pub(super) n: u32,
}

/// Splits an optional `DIE_<n>_` prefix.
fn split_die(name: &str) -> (u32, &str) {
    if let Some(rest) = name.strip_prefix("DIE_")
        && let Some((die, tail)) = rest.split_once('_')
        && let Ok(die) = die.parse()
    {
        return (die, tail);
    }
    (0, name)
}

/// Strips the first of `prefixes` (in E, M, P order) and returns its kind and the rest.
fn split_kind<'a>(s: &'a str, prefixes: [&str; 3]) -> Option<(ClusterKind, &'a str)> {
    let kinds = [ClusterKind::E, ClusterKind::M, ClusterKind::P];
    kinds
        .into_iter()
        .zip(prefixes)
        .find_map(|(k, p)| s.strip_prefix(p).map(|rest| (k, rest)))
}

fn parse_index(s: &str) -> Option<u32> {
    if s.is_empty() {
        Some(0)
    } else if s.bytes().all(|b| b.is_ascii_digit()) {
        s.parse().ok()
    } else {
        None
    }
}

/// A cluster state channel: `ECPU`, `PCPU1`, `MCPU`, `DIE_1_PCPU`. Not `ECPM`,
/// `PCPM_IDLE` (cluster power-manager channels) or per-core `PCPU010`-style names, which
/// live in another subgroup. Ultra and M5 names are unverified on hardware.
pub(crate) fn parse_cluster_channel(name: &str) -> Option<ClusterId> {
    let (die, rest) = split_die(name);
    let (kind, idx) = split_kind(rest, ["ECPU", "MCPU", "PCPU"])?;
    // Per-core channels are three digits (`PCPU010`); clusters have at most one.
    if idx.len() > 1 {
        return None;
    }
    Some(ClusterId {
        kind,
        die,
        n: parse_index(idx)?,
    })
}

/// A cluster energy channel: `EACC_CPU`, `PACC0_CPU`, `PACC1_CPU`, `DIE_0_PACC1_CPU`.
/// Verified on M3 Max (`EACC_CPU`, `PACC0_CPU`, `PACC1_CPU` sum to "CPU Energy"); other
/// chips' names are unverified.
pub(crate) fn parse_cluster_energy(name: &str) -> Option<ClusterId> {
    let (die, rest) = split_die(name);
    let rest = rest.strip_suffix("_CPU")?;
    let (kind, idx) = split_kind(rest, ["EACC", "MACC", "PACC"])?;
    Some(ClusterId {
        kind,
        die,
        n: parse_index(idx)?,
    })
}

/// `ANE`, `ANE0`, `ANE0_1` (Ultra) style names: the prefix followed by digits and `_`.
fn is_numbered(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| rest.bytes().all(|b| b.is_ascii_digit() || b == b'_'))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnergyRole {
    CpuTotal,
    Cluster(ClusterId),
    /// "GPU Energy", from the GPU driver.
    Gpu,
    Ane,
    Dram,
}

/// What an Energy Model channel contributes, if anything.
pub(crate) fn energy_role(name: &str) -> Option<EnergyRole> {
    if name == "CPU Energy" {
        Some(EnergyRole::CpuTotal)
    } else if name == "GPU Energy" {
        Some(EnergyRole::Gpu)
    } else if is_numbered(name, "ANE") {
        Some(EnergyRole::Ane)
    } else if is_numbered(name, "DRAM") {
        Some(EnergyRole::Dram)
    } else {
        parse_cluster_energy(name).map(EnergyRole::Cluster)
    }
}

/// Whether the subscription keeps a channel.
pub(super) fn keep_channel(group: &str, name: &str) -> bool {
    match group {
        "CPU Stats" => parse_cluster_channel(name).is_some(),
        "GPU Stats" => name == "GPUPH",
        "Energy Model" => energy_role(name).is_some(),
        _ => false,
    }
}

/// Idle states of CPU and GPU state channels.
pub(super) fn is_idle_state(name: &str) -> bool {
    matches!(name, "IDLE" | "DOWN" | "OFF")
}

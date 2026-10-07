//! IOReport collector: CPU cluster frequency, active and per-state residency, cluster
//! power; GPU frequency and residency; component power (CPU, GPU, ANE, DRAM, package).
//!
//! One subscription covers the channel groups the enabled [`Groups`] need, sampled as a
//! delta against the previous tick (architecture.md, Engine). IOReport is private
//! (vendored from macmon, see `vendor/`); a missing framework or group is a capability
//! (`Probe::Unsupported`), never an error.
//!
//! # Definitions
//!
//! - Cluster series come from "CPU Stats / CPU Complex Performance States", one state
//!   channel per cluster (`ECPU`, `PCPU`, `PCPU1`; `DIE_n_` prefixed on Ultra). States are
//!   `[DOWN,] IDLE, <active states...>`; the i-th active state runs at the i-th entry of
//!   the cluster's DVFS table (macmon's mapping).
//! - `cpu.cluster.active` is the share of the interval the cluster was in any active
//!   state (powermetrics' "HW active residency"). macmon's `*_active_ratio` is the mean
//!   of per-core residencies instead, which reads lower on a partly busy cluster.
//! - `cpu.cluster.freq` is the residency-weighted frequency over the active states only
//!   (powermetrics' "HW active frequency"); a fully idle interval reports the table
//!   minimum, as macmon does.
//! - `cpu.cluster.residency{state}` is the share of the whole interval per DVFS frequency
//!   (`state` = MHz) plus `idle` (DOWN + IDLE). The values sum to 100. States that share
//!   a frequency are merged.
//! - GPU series come from "GPU Stats / GPU Performance States / GPUPH" with `OFF` as idle
//!   and the GPU table without its leading 0 MHz entry.
//! - Power comes from "Energy Model" counters divided by the measured interval.
//!   `power.gpu` uses the "GPU Energy" (nJ) channel published by the GPU driver.
//!   CPU, ANE, DRAM and cluster power come from mJ channels the PMP publishes.
//!
//! # PMP energy counters refresh slowly on macOS 27
//!
//! On the development M3 Max (macOS 27.0.1) the PMP-published mJ counters only change
//! about every 5 minutes (a 15-minute watch saw updates after 255.7 s, 299.9 s and
//! 302.3 s, averaging 27.2, 18.3 and 16.8 W of CPU power; D-043). A delta
//! over one tick is then 0 most of the time and a multi-minute jump at the refresh.
//! Neither is the power over that tick, so the collector emits no PMP-derived value
//! unless the counters moved within [`MAX_PMP_SPAN_TICKS`] tick intervals; the engine
//! records the missing values as gaps. "GPU Energy" is not affected.

use std::time::Duration;

use kelvo_schema::{Entitlement, Labels, MetricId, Module, SeriesKey, UnsupportedReason};

use super::CpuPowerSource;
use super::vendor::ioreport::{self as ior, Delta, GroupSpec, Subscription};
use super::vendor::soc;
use crate::{Cadence, CollectError, Collector, CollectorId, Interest, Probe, SampleBuf, Tick};

pub const ID: CollectorId = CollectorId("ioreport");

/// Minimum period with no detail interest (tray-only mode, D-061). Every series here is
/// a counter delta, so a 10 s sample is the exact average over those 10 s: history keeps
/// correct averages and only the min/max envelope inside a bucket narrows.
pub const TRAY_ONLY_MS: u32 = 10_000;

const CPU_STATES: GroupSpec = GroupSpec {
    group: "CPU Stats",
    subgroup: Some("CPU Complex Performance States"),
};
const GPU_STATES: GroupSpec = GroupSpec {
    group: "GPU Stats",
    subgroup: Some("GPU Performance States"),
};
const ENERGY: GroupSpec = GroupSpec {
    group: "Energy Model",
    subgroup: None,
};

/// PMP-derived power is emitted only when its counters moved within this many tick
/// intervals; see the module docs.
pub const MAX_PMP_SPAN_TICKS: f64 = 2.5;

/// Which channel groups to subscribe to. Built from the modules the engine has enabled,
/// so a disabled module costs no IOReport channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Groups {
    /// `cpu.cluster.freq`, `.active`, `.residency`.
    pub cpu: bool,
    /// `gpu.freq`, `gpu.residency`.
    pub gpu: bool,
    /// `power.*` and `cpu.cluster.power`.
    pub energy: bool,
    /// Whether `power.cpu` and `cpu.cluster.power` come from the PMP counters. False when
    /// the SMC supplies them (D-054). The PMP channels stay subscribed for ANE, DRAM and
    /// package power.
    pub cpu_power: bool,
}

impl Groups {
    pub const ALL: Groups = Groups {
        cpu: true,
        gpu: true,
        energy: true,
        cpu_power: true,
    };

    fn specs(self) -> Vec<GroupSpec> {
        let mut v = Vec::new();
        // Cluster channels are also needed to label cluster power.
        if self.cpu || self.energy {
            v.push(CPU_STATES);
        }
        if self.gpu {
            v.push(GPU_STATES);
        }
        if self.energy {
            v.push(ENERGY);
        }
        v
    }
}

/// Kind letter of a CPU cluster as IOReport names it. `M` is the M5 middle tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ClusterKind {
    E,
    M,
    P,
}

impl ClusterKind {
    fn letter(self) -> char {
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
    kind: ClusterKind,
    die: u32,
    n: u32,
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
fn keep_channel(group: &str, name: &str) -> bool {
    match group {
        "CPU Stats" => parse_cluster_channel(name).is_some(),
        "GPU Stats" => name == "GPUPH",
        "Energy Model" => energy_role(name).is_some(),
        _ => false,
    }
}

/// Idle states of CPU and GPU state channels.
fn is_idle_state(name: &str) -> bool {
    matches!(name, "IDLE" | "DOWN" | "OFF")
}

/// How each state of a state channel maps to output: frequency and residency bin.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StateMap {
    /// Per state index: `None` for idle, `Some(Some(mhz_bin))` for a mapped active state,
    /// `Some(None)` for an active state beyond the DVFS table.
    states: Vec<Option<Option<usize>>>,
    /// Per state index, the state's frequency in MHz (0 for idle/unmapped).
    mhz: Vec<u32>,
    /// Residency bin labels: `idle` first, then unique MHz ascending.
    pub bins: Vec<String>,
    min_mhz: u32,
}

impl StateMap {
    /// `names` are the channel's state names in order; `table` the DVFS MHz table for
    /// its active states, in the same order.
    pub(crate) fn new(names: &[String], table: &[u32]) -> Self {
        let mut uniq: Vec<u32> = table.to_vec();
        uniq.sort_unstable();
        uniq.dedup();
        let mut bins = vec!["idle".to_owned()];
        bins.extend(uniq.iter().map(u32::to_string));
        let mut states = Vec::with_capacity(names.len());
        let mut mhz = Vec::with_capacity(names.len());
        let mut active_i = 0;
        for name in names {
            if is_idle_state(name) {
                states.push(None);
                mhz.push(0);
                continue;
            }
            let f = table.get(active_i).copied();
            active_i += 1;
            match f {
                Some(f) => {
                    // `uniq` holds every table entry, so the search finds it.
                    let bin = uniq.binary_search(&f).ok().map(|i| i + 1);
                    states.push(Some(bin));
                    mhz.push(f);
                }
                None => {
                    states.push(Some(None));
                    mhz.push(0);
                }
            }
        }
        Self {
            states,
            mhz,
            bins,
            min_mhz: uniq.first().copied().unwrap_or(0),
        }
    }

    /// Reduces one interval's residencies. `bins` is scratch space reused across calls.
    pub(crate) fn reduce(
        &self,
        residency: impl Fn(usize) -> i64,
        bins: &mut Vec<f64>,
    ) -> Option<Reduced> {
        bins.clear();
        bins.resize(self.bins.len(), 0.0);
        let (mut total, mut active, mut weighted, mut mapped) = (0.0, 0.0, 0.0, 0.0);
        for (i, state) in self.states.iter().enumerate() {
            let r = residency(i).max(0) as f64;
            total += r;
            match state {
                None => {
                    if let Some(b) = bins.first_mut() {
                        *b += r;
                    }
                }
                Some(bin) => {
                    active += r;
                    if let Some(bin) = bin {
                        if let Some(b) = bins.get_mut(*bin) {
                            *b += r;
                        }
                        weighted += r * f64::from(self.mhz.get(i).copied().unwrap_or(0));
                        mapped += r;
                    }
                }
            }
        }
        if total <= 0.0 {
            return None;
        }
        for b in bins.iter_mut() {
            *b = *b / total * 100.0;
        }
        let mhz = if mapped > 0.0 {
            weighted / mapped
        } else {
            f64::from(self.min_mhz)
        };
        Some(Reduced {
            active_pct: active / total * 100.0,
            freq_hz: mhz * 1e6,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Reduced {
    pub active_pct: f64,
    pub freq_hz: f64,
}

/// Channel metadata read once at probe time.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ChannelMeta {
    pub group: String,
    pub name: String,
    pub unit: String,
    pub states: Vec<String>,
}

fn read_meta(delta: &Delta) -> Vec<ChannelMeta> {
    (0..delta.len())
        .map(|i| match delta.channel(i) {
            Some(ch) => ChannelMeta {
                group: ch.group(),
                name: ch.name(),
                unit: ch.unit(),
                states: (0..ch.state_count()).map(|s| ch.state_name(s)).collect(),
            },
            None => ChannelMeta::default(),
        })
        .collect()
}

/// One tick's raw values, abstracted so the reduction is testable without IOReport.
pub(crate) trait Readings {
    fn len(&self) -> usize;
    fn integer(&self, i: usize) -> i64;
    fn residency(&self, i: usize, state: usize) -> i64;
}

impl Readings for Delta {
    fn len(&self) -> usize {
        Delta::len(self)
    }

    fn integer(&self, i: usize) -> i64 {
        self.channel(i).map_or(0, |c| c.integer())
    }

    fn residency(&self, i: usize, state: usize) -> i64 {
        self.channel(i).map_or(0, |c| c.residency(state))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Role {
    Skip,
    /// Index into `Plan::clusters`.
    Cluster(usize),
    Gpu,
    /// Joules per counter unit, and what the channel contributes.
    Energy {
        scale: f64,
        role: EnergyRole,
    },
}

struct ClusterOut {
    id: ClusterId,
    map: Option<StateMap>,
    freq: SeriesKey,
    active: SeriesKey,
    residency: Vec<SeriesKey>,
    power: Option<SeriesKey>,
}

struct GpuOut {
    map: StateMap,
    freq: SeriesKey,
    residency: Vec<SeriesKey>,
}

#[derive(Default)]
struct PowerKeys {
    cpu: Option<SeriesKey>,
    gpu: Option<SeriesKey>,
    ane: Option<SeriesKey>,
    dram: Option<SeriesKey>,
    package: Option<SeriesKey>,
}

/// What to read from each channel and where it goes. Built once per probe.
pub(crate) struct Plan {
    roles: Vec<Role>,
    clusters: Vec<ClusterOut>,
    gpu: Option<GpuOut>,
    power: PowerKeys,
    /// Whether the PMP CPU energy channels are subscribed; they tell whether the PMP
    /// counters moved this tick, even when `power.cpu` comes from the SMC.
    pmp: bool,
}

impl Plan {
    pub(crate) fn new(metas: &[ChannelMeta], tables: &soc::DvfsTables, groups: Groups) -> Self {
        let m = MetricId::from_static;

        let mut ids: Vec<ClusterId> = Vec::new();
        for meta in metas {
            match meta.group.as_str() {
                "CPU Stats" => ids.extend(parse_cluster_channel(&meta.name)),
                "Energy Model" => {
                    if let Some(EnergyRole::Cluster(id)) = energy_role(&meta.name) {
                        ids.push(id);
                    }
                }
                _ => {}
            }
        }
        ids.sort_unstable();
        ids.dedup();

        // Labels per kind in (die, n) order: `PCPU` -> P0, `PCPU1` -> P1, and the energy
        // channels `PACC0_CPU`, `PACC1_CPU` land on the same labels.
        let mut clusters: Vec<ClusterOut> = Vec::new();
        for id in &ids {
            let rank = ids.iter().filter(|c| c.kind == id.kind && *c < id).count();
            let label = format!("{}{rank}", id.kind.letter());
            let table = match id.kind {
                ClusterKind::E | ClusterKind::M => &tables.ecpu_mhz,
                ClusterKind::P => &tables.pcpu_mhz,
            };
            let map = metas
                .iter()
                .find(|c| c.group == "CPU Stats" && parse_cluster_channel(&c.name) == Some(*id))
                .filter(|_| groups.cpu)
                .map(|c| StateMap::new(&c.states, table));
            let residency = map.as_ref().map_or_else(Vec::new, |map| {
                map.bins
                    .iter()
                    .filter_map(|state| {
                        let l = Labels::from_pairs([("cluster", label.as_str()), ("state", state)]);
                        l.ok()
                            .map(|l| SeriesKey::new(m("cpu.cluster.residency"), l))
                    })
                    .collect()
            });
            let has_power = groups.energy
                && groups.cpu_power
                && metas.iter().any(|c| {
                    c.group == "Energy Model"
                        && ior::joules(1, &c.unit).is_some()
                        && energy_role(&c.name) == Some(EnergyRole::Cluster(*id))
                });
            let one = |metric| SeriesKey::new(m(metric), Labels::single("cluster", &label));
            clusters.push(ClusterOut {
                id: *id,
                map,
                freq: one("cpu.cluster.freq"),
                active: one("cpu.cluster.active"),
                residency,
                power: has_power.then(|| one("cpu.cluster.power")),
            });
        }

        let mut gpu = None;
        let mut seen: Vec<EnergyRole> = Vec::new();
        let roles = metas
            .iter()
            .map(|meta| match meta.group.as_str() {
                "CPU Stats" if groups.cpu => parse_cluster_channel(&meta.name)
                    .and_then(|id| clusters.iter().position(|c| c.id == id))
                    .map_or(Role::Skip, Role::Cluster),
                "GPU Stats" if groups.gpu && meta.name == "GPUPH" && gpu.is_none() => {
                    // The GPU table starts with a 0 MHz "off" entry that has no state.
                    let table = tables.gpu_mhz.get(1..).unwrap_or_default();
                    let map = StateMap::new(&meta.states, table);
                    let residency = map
                        .bins
                        .iter()
                        .map(|s| SeriesKey::new(m("gpu.residency"), Labels::single("state", s)))
                        .collect();
                    gpu = Some(GpuOut {
                        map,
                        freq: SeriesKey::bare(m("gpu.freq")),
                        residency,
                    });
                    Role::Gpu
                }
                "Energy Model" if groups.energy => {
                    match (energy_role(&meta.name), ior::joules(1, &meta.unit)) {
                        (Some(role), Some(scale)) => {
                            seen.push(role);
                            Role::Energy { scale, role }
                        }
                        _ => Role::Skip,
                    }
                }
                _ => Role::Skip,
            })
            .collect();

        let has = |pred: fn(&EnergyRole) -> bool| seen.iter().any(pred);
        let cpu = has(|r| matches!(r, EnergyRole::CpuTotal | EnergyRole::Cluster(_)));
        let gpu_p = has(|r| matches!(r, EnergyRole::Gpu));
        let key = |on: bool, id| on.then(|| SeriesKey::bare(m(id)));
        let power = PowerKeys {
            cpu: key(cpu && groups.cpu_power, "power.cpu"),
            gpu: key(gpu_p, "power.gpu"),
            ane: key(has(|r| matches!(r, EnergyRole::Ane)), "power.ane"),
            dram: key(has(|r| matches!(r, EnergyRole::Dram)), "power.dram"),
            package: key(cpu && gpu_p, "power.package"),
        };

        Self {
            roles,
            clusters,
            gpu,
            power,
            pmp: cpu,
        }
    }

    /// Every series this plan can emit.
    pub(crate) fn series(&self) -> Vec<SeriesKey> {
        let mut v = Vec::new();
        for c in &self.clusters {
            if c.map.is_some() {
                v.push(c.freq.clone());
                v.push(c.active.clone());
                v.extend(c.residency.iter().cloned());
            }
            v.extend(c.power.iter().cloned());
        }
        if let Some(g) = &self.gpu {
            v.push(g.freq.clone());
            v.extend(g.residency.iter().cloned());
        }
        let p = &self.power;
        for k in [&p.cpu, &p.gpu, &p.ane, &p.dram, &p.package]
            .into_iter()
            .flatten()
        {
            v.push(k.clone());
        }
        v
    }
}

/// Mutable per-tick state next to the plan.
#[derive(Default)]
pub(crate) struct Accum {
    /// Seconds since the PMP counters last moved, see the module docs.
    pmp_span_s: f64,
    cluster_j: Vec<f64>,
    bins: Vec<f64>,
    /// Whether the last tick skipped PMP power because the counters were stale.
    pub pmp_stale: bool,
    /// Whether the PMP counters moved in the last tick, and the P clusters' energy in
    /// it (J). The SMC CPU power calibration reads them (D-054).
    pub pmp_moved: bool,
    pub p_cluster_j: f64,
}

/// Reduces one tick. `elapsed` is the delta's measured interval.
pub(crate) fn reduce(
    plan: &Plan,
    acc: &mut Accum,
    r: &impl Readings,
    elapsed: Duration,
    out: &mut SampleBuf,
) -> Result<(), CollectError> {
    if r.len() != plan.roles.len() {
        return Err(CollectError::UnexpectedShape {
            source_name: "IOReport",
            detail: "channel count changed since probe",
        });
    }
    acc.pmp_moved = false;
    acc.p_cluster_j = 0.0;
    let dt = elapsed.as_secs_f64();
    if dt <= 0.0 {
        return Ok(());
    }
    acc.cluster_j.clear();
    acc.cluster_j.resize(plan.clusters.len(), 0.0);
    let (mut cpu_total, mut cpu_total_seen) = (0.0, false);
    let (mut gpu_j, mut ane_j, mut dram_j) = (None::<f64>, 0.0, 0.0);

    for (i, role) in plan.roles.iter().enumerate() {
        match *role {
            Role::Skip => {}
            Role::Cluster(ci) => {
                let Some(c) = plan.clusters.get(ci) else {
                    continue;
                };
                let Some(map) = &c.map else { continue };
                if let Some(red) = map.reduce(|s| r.residency(i, s), &mut acc.bins) {
                    out.push(&c.freq, red.freq_hz as f32);
                    out.push(&c.active, red.active_pct as f32);
                    for (k, v) in c.residency.iter().zip(&acc.bins) {
                        out.push(k, *v as f32);
                    }
                }
            }
            Role::Gpu => {
                let Some(g) = &plan.gpu else { continue };
                if let Some(red) = g.map.reduce(|s| r.residency(i, s), &mut acc.bins) {
                    out.push(&g.freq, red.freq_hz as f32);
                    for (k, v) in g.residency.iter().zip(&acc.bins) {
                        out.push(k, *v as f32);
                    }
                }
            }
            Role::Energy { scale, role } => {
                let j = r.integer(i).max(0) as f64 * scale;
                match role {
                    EnergyRole::CpuTotal => {
                        cpu_total += j;
                        cpu_total_seen = true;
                    }
                    EnergyRole::Cluster(id) => {
                        let slot = plan
                            .clusters
                            .iter()
                            .position(|c| c.id == id)
                            .and_then(|p| acc.cluster_j.get_mut(p));
                        if let Some(slot) = slot {
                            *slot += j;
                        }
                    }
                    EnergyRole::Gpu => *gpu_j.get_or_insert(0.0) += j,
                    EnergyRole::Ane => ane_j += j,
                    EnergyRole::Dram => dram_j += j,
                }
            }
        }
    }

    let gpu_w = gpu_j.map(|j| j / dt);
    if let (Some(k), Some(w)) = (&plan.power.gpu, gpu_w) {
        out.push(k, w as f32);
    }

    // PMP counters (CPU, ANE, DRAM, clusters) refresh together, so the CPU total (or the
    // cluster sum) tells whether they moved during this tick.
    if !plan.pmp {
        return Ok(());
    }
    let cpu_j = if cpu_total_seen {
        cpu_total
    } else {
        acc.cluster_j.iter().sum()
    };
    acc.pmp_moved = cpu_j > 0.0;
    acc.p_cluster_j = plan
        .clusters
        .iter()
        .zip(&acc.cluster_j)
        .filter(|(c, _)| c.id.kind == ClusterKind::P)
        .map(|(_, j)| j)
        .sum();
    acc.pmp_span_s += dt;
    if cpu_j <= 0.0 {
        acc.pmp_stale = true;
        return Ok(());
    }
    let span = std::mem::take(&mut acc.pmp_span_s);
    if span > dt * MAX_PMP_SPAN_TICKS {
        // The counters jumped after a stale stretch: the delta covers minutes, not this
        // tick. Skip it rather than smear it over one tick.
        acc.pmp_stale = true;
        return Ok(());
    }
    acc.pmp_stale = false;
    let cpu_w = cpu_j / dt;
    let ane_w = plan.power.ane.as_ref().map(|_| ane_j / dt);
    let dram_w = plan.power.dram.as_ref().map(|_| dram_j / dt);
    if let Some(k) = &plan.power.cpu {
        out.push(k, cpu_w as f32);
    }
    if let (Some(k), Some(w)) = (&plan.power.ane, ane_w) {
        out.push(k, w as f32);
    }
    if let (Some(k), Some(w)) = (&plan.power.dram, dram_w) {
        out.push(k, w as f32);
    }
    for (c, j) in plan.clusters.iter().zip(&acc.cluster_j) {
        if let Some(k) = &c.power {
            out.push(k, (j / dt) as f32);
        }
    }
    if let (Some(k), Some(gpu_w)) = (&plan.power.package, gpu_w) {
        let total = cpu_w + gpu_w + ane_w.unwrap_or(0.0) + dram_w.unwrap_or(0.0);
        out.push(k, total as f32);
    }
    Ok(())
}

struct Active {
    sub: Subscription,
    plan: Plan,
    acc: Accum,
}

/// The IOReport collector.
pub struct IoReport {
    groups: Groups,
    cpu_power: CpuPowerSource,
    active: Option<Active>,
}

impl IoReport {
    pub fn new(groups: Groups) -> Self {
        Self {
            groups,
            cpu_power: CpuPowerSource::default(),
            active: None,
        }
    }

    /// Leaves `power.cpu` and `cpu.cluster.power` to the SMC when `source` says so at
    /// probe time (D-054).
    pub fn with_cpu_power(self, source: CpuPowerSource) -> Self {
        Self {
            cpu_power: source,
            ..self
        }
    }

    /// Whether the last sample skipped PMP-derived power because the counters had not
    /// moved within a tick (see the module docs).
    pub fn pmp_stale(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.acc.pmp_stale)
    }

    fn setup(&self) -> Option<Active> {
        let groups = Groups {
            cpu_power: self.groups.cpu_power && !self.cpu_power.smc(),
            ..self.groups
        };
        let specs = groups.specs();
        if specs.is_empty() {
            return None;
        }
        let mut sub = Subscription::new(&specs, keep_channel).ok()?;
        // The first sample is a baseline; the delta to the second is only used to read
        // channel and state names. The second sample is the first tick's baseline.
        let _ = sub.delta(ior::now_ns());
        let metas = read_meta(&sub.delta(ior::now_ns())?);
        let chip = soc::chip_name().unwrap_or_default();
        let tables = soc::dvfs_tables(&chip).unwrap_or_default();
        Some(Active {
            sub,
            plan: Plan::new(&metas, &tables, groups),
            acc: Accum::default(),
        })
    }
}

impl Collector for IoReport {
    fn id(&self) -> CollectorId {
        ID
    }

    fn cadence(&self) -> Cadence {
        // D-061: every tick while a window shows cluster, GPU-state or component-power
        // detail; every 10 s for history while only the tray is open.
        Cadence::Adaptive {
            idle_ms: TRAY_ONLY_MS,
            interest: Interest::Detail,
        }
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Cpu, Module::Gpu, Module::Power]
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::IoReport]
    }

    fn probe(&mut self) -> Probe {
        self.cpu_power.calib().restart();
        self.active = self.setup();
        let Some(active) = &self.active else {
            return Probe::Unsupported {
                reason: UnsupportedReason::NoHardware,
            };
        };
        let series = active.plan.series();
        if series.is_empty() {
            // IOReport is there but none of the channels Kelvo knows are.
            self.active = None;
            return Probe::Unsupported {
                reason: UnsupportedReason::UnknownChip,
            };
        }
        Probe::Supported(series)
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let active = self.active.as_mut().ok_or(CollectError::NotProbed)?;
        let Some(delta) = active.sub.delta(ior::now_ns()) else {
            // A failed sample only re-establishes the baseline.
            return Ok(());
        };
        let elapsed = Duration::from_nanos(delta.elapsed_ns);
        let res = reduce(&active.plan, &mut active.acc, &delta, elapsed, out);
        if res.is_err() {
            active.sub.reset();
        } else if self.cpu_power.smc() {
            // The SMC collector ran earlier in this tick, so its integral reaches the same
            // instant as this delta.
            let acc = &active.acc;
            self.cpu_power.slow(
                acc.p_cluster_j,
                acc.pmp_moved,
                delta.elapsed_ns,
                tick.continuous_ns,
            );
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    fn meta(group: &str, name: &str, unit: &str, states: Vec<String>) -> ChannelMeta {
        ChannelMeta {
            group: group.into(),
            name: name.into(),
            unit: unit.into(),
            states,
        }
    }

    fn cpu_states(n: usize) -> Vec<String> {
        let mut v = s(&["DOWN", "IDLE"]);
        v.extend((0..n).map(|i| format!("V{i}P{}", n - 1 - i)));
        v
    }

    /// Channel layout as subscribed on the development M3 Max (macOS 27.0.1).
    fn m3_max() -> (Vec<ChannelMeta>, soc::DvfsTables) {
        let mut gpu = s(&["OFF"]);
        gpu.extend((1..=15).map(|i| format!("P{i}")));
        let metas = vec![
            meta("CPU Stats", "ECPU", "24Mticks", cpu_states(6)),
            meta("CPU Stats", "PCPU", "24Mticks", cpu_states(20)),
            meta("CPU Stats", "PCPU1", "24Mticks", cpu_states(20)),
            meta("GPU Stats", "GPUPH", "24Mticks", gpu),
            meta("Energy Model", "EACC_CPU", "mJ", vec![]),
            meta("Energy Model", "PACC0_CPU", "mJ", vec![]),
            meta("Energy Model", "PACC1_CPU", "mJ", vec![]),
            meta("Energy Model", "CPU Energy", "mJ", vec![]),
            meta("Energy Model", "ANE0", "mJ", vec![]),
            meta("Energy Model", "DRAM0", "mJ", vec![]),
            meta("Energy Model", "GPU Energy", "nJ", vec![]),
        ];
        let tables = soc::DvfsTables {
            ecpu_mhz: vec![1020, 1320, 1704, 2088, 2484, 2568],
            pcpu_mhz: vec![
                1092, 1356, 1596, 1884, 2172, 2424, 2616, 2808, 2988, 3144, 3288, 3420, 3516, 3576,
                3624, 3708, 3780, 3864, 3960, 4056,
            ],
            gpu_mhz: vec![
                0, 338, 618, 796, 832, 924, 952, 1056, 1064, 1182, 1182, 1312, 1242, 1380,
            ],
        };
        (metas, tables)
    }

    struct Fake {
        ints: Vec<i64>,
        res: Vec<Vec<i64>>,
    }

    impl Readings for Fake {
        fn len(&self) -> usize {
            self.ints.len()
        }
        fn integer(&self, i: usize) -> i64 {
            self.ints[i]
        }
        fn residency(&self, i: usize, state: usize) -> i64 {
            self.res[i].get(state).copied().unwrap_or(0)
        }
    }

    /// One second on the M3 Max layout. `cpu_mj` is the "CPU Energy" delta; the cluster
    /// channels split it 1:4:5.
    fn tick(cpu_mj: i64) -> Fake {
        let mut res = vec![Vec::new(); 11];
        // ECPU: 50% idle, 25% at 1020, 25% at 2568.
        res[0] = vec![100, 400, 250, 0, 0, 0, 0, 250];
        // PCPU: all DOWN/IDLE.
        res[1] = vec![600, 400];
        // PCPU1: 90% idle, 10% at 4056 (the last active state).
        let mut p1 = vec![0, 900];
        p1.extend(std::iter::repeat_n(0, 19));
        p1.push(100);
        res[2] = p1;
        // GPU: 80% off, 10% at P1 (338), 5% at P9 and P10 (both 1182), 5% at P14 (beyond
        // the table).
        let mut g = vec![800, 100];
        g.extend(std::iter::repeat_n(0, 7));
        g.extend([25, 25]);
        g.extend([0, 0, 0]);
        g.push(50);
        res[3] = g;
        let pmp = |v: i64| if cpu_mj > 0 { v } else { 0 };
        // Indexes 0-3 are the state channels (no integer value), 4-10 the energy channels
        // in `m3_max` order.
        let ints = vec![
            0,
            0,
            0,
            0,
            cpu_mj / 10,
            cpu_mj * 4 / 10,
            cpu_mj * 5 / 10,
            cpu_mj,
            pmp(100),
            pmp(400),
            2_500_000_000, // GPU Energy, nJ: 2.5 W over 1 s
        ];
        Fake { ints, res }
    }

    fn key(id: &'static str, labels: &[(&str, &str)]) -> SeriesKey {
        let labels = Labels::from_pairs(labels.iter().copied()).unwrap();
        SeriesKey::new(MetricId::from_static(id), labels)
    }

    fn run(plan: &Plan, acc: &mut Accum, r: &Fake) -> SampleBuf {
        let mut out = SampleBuf::new();
        reduce(plan, acc, r, Duration::from_secs(1), &mut out).unwrap();
        out
    }

    #[test]
    fn parses_cluster_channel_names() {
        let id = |kind, die, n| Some(ClusterId { kind, die, n });
        assert_eq!(parse_cluster_channel("ECPU"), id(ClusterKind::E, 0, 0));
        assert_eq!(parse_cluster_channel("PCPU1"), id(ClusterKind::P, 0, 1));
        assert_eq!(parse_cluster_channel("MCPU"), id(ClusterKind::M, 0, 0));
        // Ultra names, unverified on hardware.
        assert_eq!(
            parse_cluster_channel("DIE_1_PCPU1"),
            id(ClusterKind::P, 1, 1)
        );
        for not_a_cluster in [
            "ECPM",
            "PCPM_IDLE",
            "PCPU010",
            "ECPM_IDLE",
            "GPUPH",
            "DIE_x_PCPU",
        ] {
            assert_eq!(
                parse_cluster_channel(not_a_cluster),
                None,
                "{not_a_cluster}"
            );
        }
        assert_eq!(parse_cluster_energy("EACC_CPU"), id(ClusterKind::E, 0, 0));
        assert_eq!(parse_cluster_energy("PACC1_CPU"), id(ClusterKind::P, 0, 1));
        assert_eq!(
            parse_cluster_energy("DIE_1_PACC0_CPU"),
            id(ClusterKind::P, 1, 0)
        );
        for no in ["EACC_CPU0", "PACC0_CPM", "EACC_CPU0_SRAM", "CPU Energy"] {
            assert_eq!(parse_cluster_energy(no), None, "{no}");
        }
        assert_eq!(energy_role("ANE0"), Some(EnergyRole::Ane));
        assert_eq!(energy_role("ANE0_SRAM"), None);
        assert_eq!(energy_role("DRAM0"), Some(EnergyRole::Dram));
        assert_eq!(
            energy_role("GPU0"),
            None,
            "PMP GPU channel; GPU Energy is used"
        );
        assert!(keep_channel("GPU Stats", "GPUPH"));
        assert!(!keep_channel("GPU Stats", "GPU_SW"));
    }

    #[test]
    fn plan_labels_clusters_and_lists_series() {
        let (metas, tables) = m3_max();
        let plan = Plan::new(&metas, &tables, Groups::ALL);
        let series = plan.series();
        for k in [
            key("cpu.cluster.freq", &[("cluster", "E0")]),
            key("cpu.cluster.active", &[("cluster", "P1")]),
            key(
                "cpu.cluster.residency",
                &[("cluster", "P0"), ("state", "idle")],
            ),
            key(
                "cpu.cluster.residency",
                &[("cluster", "P0"), ("state", "4056")],
            ),
            key("cpu.cluster.power", &[("cluster", "P1")]),
            key("gpu.residency", &[("state", "1182")]),
            key("power.package", &[]),
            key("power.ane", &[]),
        ] {
            assert!(series.contains(&k), "missing {k}");
        }
        // E0: idle + 6 MHz states; GPU: idle + 12 unique MHz (1182 appears twice).
        let count = |id: &str| series.iter().filter(|k| k.metric.as_str() == id).count();
        assert_eq!(count("cpu.cluster.residency"), 7 + 21 + 21);
        assert_eq!(count("gpu.residency"), 13);
        assert_eq!(
            series.len(),
            series
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
        );
    }

    #[test]
    fn disabled_groups_emit_nothing_from_them() {
        let (metas, tables) = m3_max();
        let plan = Plan::new(
            &metas,
            &tables,
            Groups {
                cpu: false,
                gpu: true,
                energy: false,
                cpu_power: true,
            },
        );
        let series = plan.series();
        assert!(series.iter().all(|k| k.metric.as_str().starts_with("gpu.")));
        assert!(!series.is_empty());
    }

    #[test]
    fn reduces_residency_frequency_and_power() {
        let (metas, tables) = m3_max();
        let plan = Plan::new(&metas, &tables, Groups::ALL);
        let mut acc = Accum::default();
        let out = run(&plan, &mut acc, &tick(10_000));

        let get = |k: SeriesKey| out.get(&k).unwrap();
        // ECPU: half the time at 1020 and 2568 -> 1794 MHz, 50% active.
        assert_eq!(get(key("cpu.cluster.freq", &[("cluster", "E0")])), 1_794e6);
        assert_eq!(get(key("cpu.cluster.active", &[("cluster", "E0")])), 50.0);
        let res = |c: &str| -> f32 {
            out.values()
                .iter()
                .filter(|s| {
                    s.key.metric.as_str() == "cpu.cluster.residency"
                        && s.key.labels.get("cluster") == Some(c)
                })
                .map(|s| s.value)
                .sum()
        };
        for c in ["E0", "P0", "P1"] {
            assert!(
                (res(c) - 100.0).abs() < 1e-3,
                "{c} residency sums to {}",
                res(c)
            );
        }
        // Fully idle cluster reports its minimum frequency, 0% active.
        assert_eq!(get(key("cpu.cluster.freq", &[("cluster", "P0")])), 1_092e6);
        assert_eq!(get(key("cpu.cluster.active", &[("cluster", "P0")])), 0.0);
        assert_eq!(get(key("cpu.cluster.freq", &[("cluster", "P1")])), 4_056e6);
        // GPU: 20% active; P14 has no table entry, so the frequency averages the mapped
        // 15%: (10*338 + 5*1182) / 15 = 619.3 MHz. Residency of mapped states sums to 95.
        let gpu_f = get(key("gpu.freq", &[]));
        assert!((gpu_f - 619.333e6).abs() < 1e3, "{gpu_f}");
        assert_eq!(get(key("gpu.residency", &[("state", "1182")])), 5.0);
        assert_eq!(get(key("gpu.residency", &[("state", "idle")])), 80.0);
        // Power over one second.
        assert_eq!(get(key("power.cpu", &[])), 10.0);
        assert_eq!(get(key("power.gpu", &[])), 2.5);
        assert_eq!(get(key("power.ane", &[])), 0.1);
        assert_eq!(get(key("power.dram", &[])), 0.4);
        assert_eq!(get(key("cpu.cluster.power", &[("cluster", "P1")])), 5.0);
        assert_eq!(get(key("power.package", &[])), 13.0);
        assert!(!acc.pmp_stale);
    }

    #[test]
    fn stale_pmp_counters_emit_no_cpu_power() {
        let (metas, tables) = m3_max();
        let plan = Plan::new(&metas, &tables, Groups::ALL);
        let mut acc = Accum::default();
        let cpu = key("power.cpu", &[]);
        let gpu = key("power.gpu", &[]);
        let pkg = key("power.package", &[]);

        // Counters frozen for four ticks: GPU power still flows, CPU-side power does not.
        for _ in 0..4 {
            let out = run(&plan, &mut acc, &tick(0));
            assert_eq!(
                out.get(&cpu),
                None,
                "0 W from a frozen counter is not a reading"
            );
            assert_eq!(out.get(&pkg), None);
            assert_eq!(out.get(&gpu), Some(2.5));
            assert!(acc.pmp_stale);
        }
        // The refresh: five minutes' worth of energy lands in one tick. Not this tick's
        // power, so skipped.
        let out = run(&plan, &mut acc, &tick(5_481_355));
        assert_eq!(out.get(&cpu), None, "a multi-tick jump must not be emitted");
        assert!(acc.pmp_stale);
        // Counters moving every tick again: values resume.
        let out = run(&plan, &mut acc, &tick(10_000));
        assert_eq!(out.get(&cpu), Some(10.0));
        assert!(!acc.pmp_stale);
    }

    #[test]
    fn smc_cpu_power_leaves_only_cpu_series_to_the_smc() {
        let (metas, tables) = m3_max();
        let groups = Groups {
            cpu_power: false,
            ..Groups::ALL
        };
        let plan = Plan::new(&metas, &tables, groups);
        let series = plan.series();
        let cpu = key("power.cpu", &[]);
        let p1 = key("cpu.cluster.power", &[("cluster", "P1")]);
        assert!(!series.contains(&cpu));
        assert!(
            !series
                .iter()
                .any(|k| k.metric.as_str() == "cpu.cluster.power")
        );
        for k in ["power.gpu", "power.ane", "power.dram", "power.package"] {
            assert!(series.contains(&key(k, &[])), "{k} stays on IOReport");
        }

        // The PMP gating still runs on the CPU energy channels: frozen counters mean no
        // ANE, DRAM or package value; moving ones bring them back.
        let mut acc = Accum::default();
        let out = run(&plan, &mut acc, &tick(0));
        assert_eq!(out.get(&key("power.dram", &[])), None);
        assert_eq!(out.get(&key("power.gpu", &[])), Some(2.5));
        assert!(acc.pmp_stale);
        assert!(!acc.pmp_moved);
        let out = run(&plan, &mut acc, &tick(10_000));
        // The calibration gets the P clusters' energy only (4 J + 5 J), not EACC's 1 J.
        assert!(acc.pmp_moved);
        assert!((acc.p_cluster_j - 9.0).abs() < 1e-9, "{}", acc.p_cluster_j);
        assert_eq!(out.get(&key("power.dram", &[])), Some(0.4));
        assert_eq!(out.get(&key("power.package", &[])), Some(13.0));
        assert_eq!(out.get(&cpu), None);
        assert_eq!(out.get(&p1), None);
    }

    #[test]
    fn changed_channel_count_is_an_error() {
        let (metas, tables) = m3_max();
        let plan = Plan::new(&metas, &tables, Groups::ALL);
        let mut r = tick(10_000);
        r.ints.pop();
        let mut out = SampleBuf::new();
        let err = reduce(
            &plan,
            &mut Accum::default(),
            &r,
            Duration::from_secs(1),
            &mut out,
        );
        assert!(matches!(err, Err(CollectError::UnexpectedShape { .. })));
        assert!(out.values().is_empty());
    }

    /// Samples the real hardware twice and checks plausibility.
    #[test]
    #[ignore = "needs Apple Silicon hardware; run by hand with --ignored --nocapture"]
    fn live_ioreport() {
        let chip = soc::chip_name().unwrap_or_default();
        let tables = soc::dvfs_tables(&chip).expect("DVFS tables");
        println!("chip: {chip}\nDVFS: {tables:?}");
        let mut c = IoReport::new(Groups::ALL);
        let Probe::Supported(series) = c.probe() else {
            panic!("IOReport unsupported");
        };
        println!("{} series", series.len());
        let all_cpu: Vec<u32> = tables
            .ecpu_mhz
            .iter()
            .chain(&tables.pcpu_mhz)
            .copied()
            .collect();
        let (lo, hi) = (
            f64::from(*all_cpu.iter().min().unwrap()),
            f64::from(*all_cpu.iter().max().unwrap()),
        );
        for n in 0..2 {
            std::thread::sleep(Duration::from_secs(1));
            let mut out = SampleBuf::new();
            let tick = Tick {
                n,
                wall_ms: 0,
                continuous_ns: 0,
                interval_ms: 1_000,
            };
            c.sample(&tick, &mut out).unwrap();
            println!("--- sample {n} (PMP stale: {})", c.pmp_stale());
            let mut residency: std::collections::BTreeMap<String, f32> = Default::default();
            for s in out.values() {
                let id = s.key.metric.as_str();
                if id == "cpu.cluster.residency" || id == "gpu.residency" {
                    let who = s.key.labels.get("cluster").unwrap_or("gpu").to_owned();
                    *residency.entry(who).or_default() += s.value;
                    if s.value > 0.5 {
                        println!("  {} = {:.1}", s.key, s.value);
                    }
                    continue;
                }
                println!("  {} = {:.3}", s.key, s.value);
                match id {
                    "cpu.cluster.freq" => {
                        let f = f64::from(s.value);
                        assert!(
                            (lo * 1e6 - 1.0..=hi * 1e6 + 1.0).contains(&f),
                            "{} {f}",
                            s.key
                        );
                    }
                    "gpu.freq" => {
                        let max = f64::from(*tables.gpu_mhz.iter().max().unwrap()) * 1e6;
                        assert!((0.0..=max + 1.0).contains(&f64::from(s.value)));
                    }
                    "cpu.cluster.active" => assert!((0.0..=100.0).contains(&s.value)),
                    _ if id.starts_with("power.") || id == "cpu.cluster.power" => {
                        assert!(s.value >= 0.0 && s.value < 500.0, "{} {}", s.key, s.value);
                    }
                    _ => {}
                }
            }
            for (who, sum) in &residency {
                println!("  residency sum {who} = {sum:.2}");
                // GPU states beyond the DVFS table are not labelled, so its sum may be lower.
                if who == "gpu" {
                    assert!(*sum <= 100.01 && *sum > 90.0, "{who} {sum}");
                } else {
                    assert!((sum - 100.0).abs() < 0.01, "{who} {sum}");
                }
            }
        }
    }

    /// Runs the SMC power collector and IOReport together at 1 s, as the engine does, and
    /// checks the calibrated `power.cpu` against PMP over whole refresh windows that
    /// started after the first calibration. That is an out-of-sample check: each window's
    /// scale comes from earlier windows. Takes three or more PMP refreshes (15 to 45 min);
    /// `KELVO_CALIB_MINUTES` caps it (default 60).
    #[test]
    #[ignore = "needs an M3 Max; runs up to an hour; run by hand with --ignored --nocapture"]
    fn live_cpu_power_calibration_matches_pmp() {
        use super::super::smc;
        use super::super::sysctl::continuous_ns;
        let minutes: u64 = std::env::var("KELVO_CALIB_MINUTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);
        let source = CpuPowerSource::default();
        let mut power = smc::Power::new(source.clone());
        let mut ior = IoReport::new(Groups::ALL).with_cpu_power(source.clone());
        assert!(matches!(power.probe(), Probe::Supported(_)));
        assert!(source.smc(), "this chip has no SMC CPU power map");
        assert!(matches!(ior.probe(), Probe::Supported(_)));
        let cpu = SeriesKey::bare(MetricId::from_static("power.cpu"));
        let status = SeriesKey::bare(MetricId::from_static("power.cpu_source"));

        let start = std::time::Instant::now();
        let mut last_ns = continuous_ns();
        // Calibrated SMC energy since the last PMP move, if the whole span was calibrated.
        let mut smc_j: Option<f64> = None;
        let mut errors = Vec::new();
        let mut n = 0;
        while start.elapsed() < Duration::from_secs(minutes * 60) && errors.len() < 3 {
            std::thread::sleep(Duration::from_secs(1));
            n += 1;
            let now = continuous_ns();
            let tick = Tick {
                n,
                wall_ms: 0,
                continuous_ns: now,
                interval_ms: 1_000,
            };
            let mut out = SampleBuf::new();
            power.sample(&tick, &mut out).unwrap();
            let dt = (now - last_ns) as f64 / 1e9;
            last_ns = now;
            let calibrated = out.get(&status)
                == Some(kelvo_schema::MetricCode::value(
                    kelvo_schema::CpuPowerCalibration::Calibrated,
                ));
            match (out.get(&cpu), calibrated) {
                (Some(w), true) => smc_j = smc_j.map(|j| j + f64::from(w) * dt),
                _ => smc_j = None,
            }
            let mut out2 = SampleBuf::new();
            ior.sample(&tick, &mut out2).unwrap();
            let acc = &ior.active.as_ref().unwrap().acc;
            if acc.pmp_moved {
                let scale = source.calib().scale();
                let t = start.elapsed().as_secs();
                if let Some(j) = smc_j {
                    let err = j / acc.p_cluster_j - 1.0;
                    println!(
                        "{t:>5} s  PMP {:.1} J, calibrated SMC {j:.1} J, error {:+.1}%, scale now {scale:?}",
                        acc.p_cluster_j,
                        err * 100.0
                    );
                    errors.push(err);
                } else {
                    println!("{t:>5} s  PMP moved, scale {scale:?}");
                }
                smc_j = calibrated.then_some(0.0);
            }
        }
        println!("errors: {errors:?}");
        assert!(
            !errors.is_empty(),
            "no calibrated window closed in {minutes} min"
        );
        for e in errors {
            assert!(e.abs() <= 0.05, "{:+.1}%", e * 100.0);
        }
    }
}

//! What each channel feeds ([`Plan`], built once per probe) and the per-tick reduction
//! ([`reduce`]), testable without IOReport through [`Readings`].

use std::time::Duration;

use kelvo_schema::{Labels, MetricId, SeriesKey};

use super::channels::{
    ClusterId, ClusterKind, EnergyRole, energy_role, is_idle_state, parse_cluster_channel,
};
use super::{Groups, MAX_PMP_SPAN_TICKS};
use crate::macos::vendor::ioreport::{self as ior, Delta};
use crate::macos::vendor::soc;
use crate::{CollectError, SampleBuf};

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

pub(super) fn read_meta(delta: &Delta) -> Vec<ChannelMeta> {
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

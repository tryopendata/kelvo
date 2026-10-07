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

use kelvo_schema::{Entitlement, Module, UnsupportedReason};

use self::channels::keep_channel;
use self::plan::{Accum, Plan, read_meta, reduce};
use super::CpuPowerSource;
use super::vendor::ioreport::{self as ior, GroupSpec, Subscription};
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

mod channels;
mod plan;
#[cfg(test)]
mod tests;

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

//! macOS collectors. Public-API collectors (`host_processor_info`, `host_statistics64`,
//! routing-socket interface counters, IOKit registry reads, libproc, NSProcessInfo) live
//! here; private-API collectors (IOReport, SMC, HID, NetworkStatistics) live in their own
//! modules with their FFI confined there (.claude/rules/rust.md).

pub mod battery;
pub mod cpu;
pub mod disk;
pub mod gpu;
pub mod gpu_procs;
pub mod hid;
pub mod ifaddrs;
mod iokit;
pub mod ioreport;
mod libproc;
pub mod memory;
pub mod network;
pub mod nstat;
pub mod ports;
pub mod power_sources;
pub(crate) mod process_control;
pub mod processes;
pub mod self_cpu;
pub mod sensors;
pub mod smc;
pub mod sysctl;
pub mod thermal_state;
mod vendor;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub use iokit::platform_uuid;

use crate::Collector;
use crate::calib::{Calibrator, NoScaleStore, ScaleStore};

/// The GPU's DVFS operating points in MHz, for `HostInfo::gpu_dvfs_mhz` (D-092): the
/// IORegistry table `gpu.residency`'s states come from, in its order and without its
/// leading off entry. Empty when the table is not found.
pub fn gpu_dvfs_mhz() -> Vec<u32> {
    let chip = vendor::soc::chip_name().unwrap_or_default();
    vendor::soc::dvfs_tables(&chip)
        .map(|t| t.gpu_mhz.get(1..).unwrap_or_default().to_vec())
        .unwrap_or_default()
}

/// A `*_total` series being summed over one sample's parts (`net.rx_total`,
/// `disk.read_total`, D-092): the parts are the series the collector reports on that
/// sample, and the total is a gap when any of them is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PartSum {
    sum: f64,
    gap: bool,
}

impl PartSum {
    /// Adds one reported part's value for this sample; `None` is that part's gap.
    pub(crate) fn add(&mut self, part: Option<f32>) {
        match part {
            Some(v) => self.sum += f64::from(v),
            None => self.gap = true,
        }
    }

    /// The total, `None` when any part was a gap.
    pub(crate) fn total(self) -> Option<f32> {
        (!self.gap).then_some(self.sum as f32)
    }
}

/// Which collector supplies `power.cpu` and `cpu.cluster.power` (D-054). The SMC power
/// collector sets it when its probe finds this chip's CPU power keys. The IOReport
/// collector is probed after it and then leaves those series to the SMC, keeping the PMP
/// path as the fallback. If the build's entitlements deny the SMC collector, it never
/// probes, the flag stays clear and IOReport supplies them.
///
/// It also carries the [`Calibrator`] that scales the SMC reading to the PMP counters:
/// the SMC collector feeds it P-cluster watts every tick, IOReport feeds it P-cluster PMP
/// energy, and both run on the engine thread one after the other, so the lock is never
/// contended. The SMC probe seeds it from the [`ScaleStore`] (or the chip's default), and
/// each closed window is saved back to the store (D-065).
#[derive(Clone)]
pub struct CpuPowerSource(Arc<SourceState>);

struct SourceState {
    smc: AtomicBool,
    calib: Mutex<Calibrator>,
    /// The chip the SMC probe found a power map for; the store's key.
    chip: Mutex<Option<String>>,
    store: Arc<dyn ScaleStore>,
}

impl Default for CpuPowerSource {
    fn default() -> Self {
        Self::new(Arc::new(NoScaleStore))
    }
}

impl std::fmt::Debug for CpuPowerSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CpuPowerSource")
            .field("smc", &self.smc())
            .field("calib", &*self.calib())
            .finish_non_exhaustive()
    }
}

fn relock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A poisoned lock still holds usable state: plain values updated in single
    // assignments.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl CpuPowerSource {
    /// A source whose learned scales persist through `store`.
    pub fn new(store: Arc<dyn ScaleStore>) -> Self {
        Self(Arc::new(SourceState {
            smc: AtomicBool::new(false),
            calib: Mutex::new(Calibrator::default()),
            chip: Mutex::new(None),
            store,
        }))
    }

    /// Whether the SMC supplies CPU power.
    pub fn smc(&self) -> bool {
        self.0.smc.load(Ordering::Acquire)
    }

    pub(crate) fn set_smc(&self, on: bool) {
        self.0.smc.store(on, Ordering::Release);
    }

    /// The shared calibrator.
    pub(crate) fn calib(&self) -> MutexGuard<'_, Calibrator> {
        relock(&self.0.calib)
    }

    /// Records `chip` as the store key and, unless a window already closed this session,
    /// seeds the calibrator with the scale stored for it, else with `default`.
    pub(crate) fn seed(&self, chip: &str, default: Option<f64>) {
        *relock(&self.0.chip) = Some(chip.to_owned());
        let stored = self
            .0
            .store
            .load(chip)
            .filter(|s| crate::calib::plausible_scale(*s));
        if let Some(scale) = stored.or(default) {
            self.calib().seed(scale);
        }
    }

    /// Feeds one slow sample to the calibrator (see [`Calibrator::slow`]) and saves the
    /// new scale when a window closes. The save runs after the lock is released.
    pub(crate) fn slow(&self, joules: f64, moved: bool, period_ns: u64, t_ns: u64) {
        let scale = {
            let mut cal = self.calib();
            cal.slow(joules, moved, period_ns, t_ns)
                .and_then(|_| cal.scale())
        };
        let Some(scale) = scale else {
            return;
        };
        let chip = relock(&self.0.chip).clone();
        if let Some(chip) = chip {
            self.0.store.save(&chip, scale);
        }
    }
}

/// Every macOS collector, unfiltered. The SMC power collector comes before IOReport
/// because the engine probes in this order and IOReport's probe reads [`CpuPowerSource`].
/// The per-process network collector comes after the processes collector, which the
/// engine samples first within a tick, so its sampling can follow "processes sampled
/// this tick" (D-089). Learned CPU power scales persist through `scales`.
pub fn collectors(scales: Arc<dyn ScaleStore>) -> Vec<Box<dyn Collector>> {
    let cpu_power = CpuPowerSource::new(scales);
    vec![
        Box::new(cpu::Cpu::new()),
        Box::new(memory::Memory::new()),
        Box::new(network::Network::new()),
        Box::new(disk::DiskIo::new()),
        Box::new(disk::DiskCapacity::new()),
        Box::new(battery::Battery::new()),
        Box::new(processes::Processes::new()),
        Box::new(ports::ProcessPorts::new()),
        Box::new(nstat::NetPerProcess::new()),
        Box::new(self_cpu::SelfCpu::new()),
        Box::new(thermal_state::ThermalState::new()),
        Box::new(smc::Power::new(cpu_power.clone())),
        Box::new(ioreport::IoReport::new(ioreport::Groups::ALL).with_cpu_power(cpu_power)),
        Box::new(gpu::Gpu::new()),
        Box::new(gpu_procs::GpuPerProcess::new()),
        Box::new(smc::Fans::new()),
        Box::new(sensors::SmcSensors::new()),
        Box::new(hid::HidThermal::new()),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::calib::CalibState;

    const S: u64 = 1_000_000_000;
    const CHIP: &str = "Apple M3 Max";

    /// What the shell's file store does, in memory: the map outlives each source, as the
    /// file outlives a session.
    #[derive(Default)]
    struct MemStore(Mutex<BTreeMap<String, f64>>);

    impl ScaleStore for MemStore {
        fn load(&self, chip: &str) -> Option<f64> {
            relock(&self.0).get(chip).copied()
        }

        fn save(&self, chip: &str, scale: f64) {
            relock(&self.0).insert(chip.to_owned(), scale);
        }
    }

    /// Runs `secs` of 1 s steps where the SMC reads `fast_w` and PMP moves every 300 s
    /// with `slow_w` worth of energy, as the two collectors feed the source.
    fn run(src: &CpuPowerSource, from_s: u64, secs: u64, fast_w: f64, slow_w: f64) {
        for t in from_s + 1..=from_s + secs {
            src.calib().fast(Some(fast_w), t * S);
            let moved = t % 300 == 0;
            let j = if moved { slow_w * 300.0 } else { 0.0 };
            src.slow(j, moved, S, t * S);
        }
    }

    #[test]
    fn part_sum_is_a_gap_when_any_part_is() {
        let mut s = PartSum::default();
        s.add(Some(1.5));
        s.add(Some(2.5));
        assert_eq!(s.total(), Some(4.0));
        s.add(None);
        s.add(Some(1.0));
        assert_eq!(s.total(), None);
    }

    #[test]
    fn a_new_session_seeds_from_the_chip_default_then_the_stored_scale() {
        let store = Arc::new(MemStore::default());
        let first = CpuPowerSource::new(store.clone());
        first.seed(CHIP, Some(1.33));
        assert_eq!(first.calib().state(), CalibState::Seeded(1.33));

        // Two PMP moves close a window: 8 W against 6 W read is a ratio of 4/3, folded
        // into the 1.33 seed, and saved for the next session.
        run(&first, 0, 600, 6.0, 8.0);
        let CalibState::Calibrated(learned) = first.calib().state() else {
            panic!("no window closed");
        };
        let want = 1.33 + crate::calib::ALPHA * (8.0 / 6.0 - 1.33);
        assert!((learned - want).abs() < 1e-9, "{learned}");
        assert_eq!(store.load(CHIP), Some(learned));

        // A restart: the stored scale wins over the default, and is marked seeded.
        let second = CpuPowerSource::new(store.clone());
        second.seed(CHIP, Some(1.33));
        assert_eq!(second.calib().state(), CalibState::Seeded(learned));
    }

    #[test]
    fn every_closed_window_updates_the_stored_scale() {
        let store = Arc::new(MemStore::default());
        let src = CpuPowerSource::new(store.clone());
        src.seed(CHIP, None);
        assert_eq!(src.calib().state(), CalibState::Uncalibrated);
        run(&src, 0, 600, 6.0, 9.0);
        let after_one = store.load(CHIP).unwrap();
        assert!((after_one - 1.5).abs() < 1e-9, "{after_one}");
        run(&src, 600, 300, 6.0, 6.0);
        let after_two = store.load(CHIP).unwrap();
        assert!((after_two - 1.25).abs() < 1e-9, "{after_two}");
    }

    #[test]
    fn an_implausible_stored_scale_falls_back_to_the_default() {
        let store = Arc::new(MemStore::default());
        store.save(CHIP, 40.0);
        let src = CpuPowerSource::new(store.clone());
        src.seed(CHIP, Some(1.33));
        assert_eq!(src.calib().state(), CalibState::Seeded(1.33));
    }

    /// Only counter-derived collectors may drop to [`crate::IDLE_MS`] when nothing shows
    /// them: their next delta covers the skipped span, so S10 and M1 averages stay exact.
    /// A gauge sampled once per 10 s would turn its M1 average into a mean of six points
    /// (review #2).
    #[test]
    fn only_counter_collectors_idle_when_not_shown() {
        let idle: Vec<_> = collectors(Arc::new(NoScaleStore))
            .iter()
            .filter(|c| c.cadence() == crate::LIVE_OR_IDLE)
            .map(|c| c.id().as_str())
            .collect();
        assert_eq!(idle, ["cpu", "network", "disk_io"]);
    }

    #[test]
    fn nothing_is_saved_before_the_smc_probe_names_the_chip() {
        let store = Arc::new(MemStore::default());
        let src = CpuPowerSource::new(store.clone());
        run(&src, 0, 600, 6.0, 8.0);
        assert!(src.calib().scale().is_some());
        assert!(relock(&store.0).is_empty());
    }
}

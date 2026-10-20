//! Linux collectors (D-008, D-115): `/proc`, sysfs, hwmon and cgroups v2, read through a
//! [`SysRoot`] so every parser runs against captured fixtures in tests. Modules no
//! collector covers yet probe as `Unsupported` through [`Unsupported`].

pub mod battery;
mod blockdev;
mod counters;
pub mod diskstats;
pub mod fs;
pub mod hwmon;
pub mod ifaddrs;
mod mountinfo;
pub mod net;
pub mod rapl;
mod sysroot;

pub mod cpu;
pub mod memory;
pub mod processes;
pub(crate) mod procfs;
pub mod self_cpu;
#[cfg(test)]
pub(crate) mod testdata;
pub(crate) mod topology;

pub use sysroot::{SysRoot, clk_tck, page_size};

use kelvo_schema::{Entitlement, Module, UnsupportedReason};

use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

/// Reports `Unsupported { NoHardware }` for the modules no Linux collector covers yet,
/// and never emits values.
pub struct Unsupported {
    modules: &'static [Module],
}

impl Collector for Unsupported {
    fn id(&self) -> CollectorId {
        CollectorId("linux_unsupported")
    }

    fn cadence(&self) -> Cadence {
        Cadence::EveryTick
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        self.modules
    }

    fn probe(&mut self) -> Probe {
        Probe::Unsupported {
            reason: UnsupportedReason::NoHardware,
        }
    }

    fn sample(&mut self, _tick: &Tick, _out: &mut SampleBuf) -> Result<(), CollectError> {
        Err(CollectError::NotProbed)
    }
}

/// Modules no Linux collector covers yet. Shrinks as collectors land.
const UNCOVERED: &[Module] = &[Module::Gpu, Module::Containers];

pub fn collectors() -> Vec<Box<dyn Collector>> {
    let mut out: Vec<Box<dyn Collector>> = Vec::new();
    // Each collector group pushes here; keep them in probe order.
    out.extend([
        Box::new(cpu::Cpu::new()) as Box<dyn Collector>,
        Box::new(memory::Memory::new()),
    ]);
    out.push(Box::new(diskstats::DiskStats::new()));
    out.push(Box::new(fs::Filesystems::new()));
    out.push(Box::new(net::Network::new()));
    out.push(Box::new(processes::Processes::new()));
    out.push(Box::new(self_cpu::SelfCpu::new()));
    out.push(Box::new(hwmon::Hwmon::sensors()));
    out.push(Box::new(hwmon::Hwmon::power()));
    out.push(Box::new(rapl::Rapl::new()));
    out.push(Box::new(battery::Battery::new()));
    if !UNCOVERED.is_empty() {
        out.push(Box::new(Unsupported { modules: UNCOVERED }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncovered_modules_are_unsupported() {
        for mut c in collectors() {
            if c.id() == CollectorId("linux_unsupported") {
                assert!(matches!(c.probe(), Probe::Unsupported { .. }));
            }
        }
    }
}

//! Linux stub until v4.1: one collector that probes as `Unsupported` for every module. It
//! exists so the shared crates compile and test on Linux (the ubuntu CI job,
//! `make rust-linux-check`).

use kelvo_schema::{Entitlement, Module, UnsupportedReason};

use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

/// Reports `Unsupported { NoHardware }` for every module and never emits values.
pub struct Unsupported;

impl Collector for Unsupported {
    fn id(&self) -> CollectorId {
        CollectorId("linux_stub")
    }

    fn cadence(&self) -> Cadence {
        Cadence::EveryTick
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        &Module::ALL
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

pub fn collectors() -> Vec<Box<dyn Collector>> {
    vec![Box::new(Unsupported)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_is_unsupported_for_every_module() {
        for mut c in collectors() {
            assert!(matches!(c.probe(), Probe::Unsupported { .. }), "{}", c.id());
            assert_eq!(c.modules().len(), Module::ALL.len());
        }
    }
}

//! `thermal.state` from `NSProcessInfo.thermalState`: 0 nominal, 1 fair, 2 serious,
//! 3 critical (the same codes as `kelvo_schema::ThermalState`). Public API, no
//! entitlement, sampled every 2 s like the fans.

use kelvo_schema::{Entitlement, MetricCode, MetricId, Module, SeriesKey};
use objc2_foundation::NSProcessInfo;

use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

pub struct ThermalState {
    key: SeriesKey,
}

impl Default for ThermalState {
    fn default() -> Self {
        Self::new()
    }
}

impl ThermalState {
    pub fn new() -> Self {
        Self {
            key: SeriesKey::bare(MetricId::from_static("thermal.state")),
        }
    }
}

/// The current state, `None` for a value outside 0..=3 (a future macOS).
pub fn current() -> Option<kelvo_schema::ThermalState> {
    let raw = NSProcessInfo::processInfo().thermalState().0;
    u8::try_from(raw)
        .ok()
        .and_then(|v| kelvo_schema::ThermalState::from_value(f32::from(v)))
}

impl Collector for ThermalState {
    fn id(&self) -> CollectorId {
        CollectorId("thermal_state")
    }

    fn cadence(&self) -> Cadence {
        Cadence::Every(2_000)
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Sensors]
    }

    fn probe(&mut self) -> Probe {
        Probe::Supported(vec![self.key.clone()])
    }

    fn sample(&mut self, _tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        match current() {
            Some(v) => {
                out.push(&self.key, v.value());
                Ok(())
            }
            None => Err(CollectError::UnexpectedShape {
                source_name: "NSProcessInfo.thermalState",
                detail: "value outside 0..=3",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_valid_state() {
        let mut c = ThermalState::new();
        let mut buf = SampleBuf::new();
        let tick = Tick {
            n: 0,
            wall_ms: 0,
            continuous_ns: 0,
            interval_ms: 1_000,
        };
        c.sample(&tick, &mut buf).unwrap();
        let v = buf.get(&c.key).unwrap();
        println!("thermal.state = {v}");
        assert!((0.0..=3.0).contains(&v));
    }
}

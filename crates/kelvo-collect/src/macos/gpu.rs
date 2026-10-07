//! GPU utilization: `gpu.util`, `gpu.render`, `gpu.tiler`.
//!
//! Read from the `PerformanceStatistics` dictionary of the `IOAccelerator` service
//! (`AGXAcceleratorG15X` on an M3 Max): "Device Utilization %", "Renderer Utilization %"
//! and "Tiler Utilization %". This is a public IORegistry read, so it needs no
//! entitlement. All three keys were present and integer percentages on the development
//! M3 Max (macOS 27.0.1); Activity Monitor's GPU history reads the same dictionary.
//! Each series is offered only if its key exists at probe time.
//!
//! GPU frequency and residency come from IOReport (`ioreport.rs`).

use kelvo_schema::{Entitlement, MetricId, Module, SeriesKey};

use super::vendor::{cf, iokit};
use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

pub const ID: CollectorId = CollectorId("gpu");

const STATS: &str = "PerformanceStatistics";
const KEYS: [(&str, &str); 3] = [
    ("Device Utilization %", "gpu.util"),
    ("Renderer Utilization %", "gpu.render"),
    ("Tiler Utilization %", "gpu.tiler"),
];

pub struct Gpu {
    service: Option<iokit::IoObject>,
    /// `(PerformanceStatistics key, series)` for the keys found at probe time.
    series: Vec<(&'static str, SeriesKey)>,
}

impl Default for Gpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Gpu {
    pub fn new() -> Self {
        Self {
            service: None,
            series: Vec::new(),
        }
    }
}

/// Percent values from a `PerformanceStatistics` dictionary, clamped to 0..=100.
fn read_pct(stats: &cf::Dict, key: &str) -> Option<f32> {
    let v = cf::get_i64(stats, key)?;
    Some(v.clamp(0, 100) as f32)
}

impl Collector for Gpu {
    fn id(&self) -> CollectorId {
        ID
    }

    fn cadence(&self) -> Cadence {
        // Every tick, also with nothing on screen: Device Utilization % is an instantaneous gauge, so a
        // 10 s idle period would leave one point per S10 bucket and an inexact M1 average.
        // Only counter-derived collectors use LIVE_OR_IDLE (D-070).
        Cadence::EveryTick
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Gpu]
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn probe(&mut self) -> Probe {
        self.service = None;
        self.series.clear();
        // An eGPU or a second accelerator would be another service; v1 reports the first
        // one that has the statistics (the built-in GPU on Apple Silicon).
        for service in iokit::matching_services(c"IOAccelerator") {
            let Some(stats) = service.dict_property(STATS) else {
                continue;
            };
            let series: Vec<_> = KEYS
                .iter()
                .filter(|(k, _)| cf::get_i64(&stats, k).is_some())
                .map(|(k, id)| (*k, SeriesKey::bare(MetricId::from_static(id))))
                .collect();
            if !series.is_empty() {
                self.series = series;
                self.service = Some(service);
                break;
            }
        }
        if self.service.is_none() {
            return Probe::NotPresent;
        }
        Probe::Supported(self.series.iter().map(|(_, k)| k.clone()).collect())
    }

    fn sample(&mut self, _tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let service = self.service.as_ref().ok_or(CollectError::NotProbed)?;
        let stats = service
            .dict_property(STATS)
            .ok_or(CollectError::UnexpectedShape {
                source_name: "IOAccelerator",
                detail: "PerformanceStatistics missing",
            })?;
        for (key, series) in &self.series {
            if let Some(v) = read_pct(&stats, key) {
                out.push(series, v);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Samples the real GPU statistics twice.
    #[test]
    #[ignore = "needs Apple Silicon hardware; run by hand with --ignored --nocapture"]
    fn live_gpu() {
        let mut c = Gpu::new();
        let Probe::Supported(series) = c.probe() else {
            panic!("no IOAccelerator PerformanceStatistics");
        };
        println!("series: {series:?}");
        for n in 0..2 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let mut out = SampleBuf::new();
            let tick = Tick {
                n,
                wall_ms: 0,
                continuous_ns: 0,
                interval_ms: 1_000,
            };
            c.sample(&tick, &mut out).unwrap();
            for s in out.values() {
                println!("  {} = {}", s.key, s.value);
                assert!((0.0..=100.0).contains(&s.value));
            }
            assert_eq!(out.values().len(), series.len());
        }
    }
}

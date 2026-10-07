//! Raw HID thermal zones: `thermal.zone{sensor}` and `thermal.hottest`.
//!
//! Every temperature service the IOHID event system lists, labelled with its raw
//! "Product" name ("PMU tdie4", "NAND CH0 temp", "gas gauge battery"). No chip map is
//! needed, so this keeps working on chips the SMC sensor map does not know. Names repeat
//! (an M3 Max lists 45 services under 24 names: each PMU sensor twice, the battery gauge
//! six times); repeated names are reported as their max, which keeps one stable series
//! per name whatever order the services come back in.

use kelvo_schema::{Entitlement, Labels, MetricId, Module, SeriesKey};

use super::sensors::plausible_celsius;
use super::vendor::hid::HidSensors;
use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

pub const ID: CollectorId = crate::TEMPERATURE_COLLECTORS[1];

pub struct HidThermal {
    sensors: Option<HidSensors>,
    /// Per HID service, the index of its name in `zones`.
    zone_of: Vec<usize>,
    zones: Vec<SeriesKey>,
    hottest: SeriesKey,
    raw: Vec<Option<f32>>,
    per_zone: Vec<Option<f32>>,
}

impl Default for HidThermal {
    fn default() -> Self {
        Self::new()
    }
}

impl HidThermal {
    pub fn new() -> Self {
        Self {
            sensors: None,
            zone_of: Vec::new(),
            zones: Vec::new(),
            hottest: SeriesKey::bare(MetricId::from_static("thermal.hottest")),
            raw: Vec::new(),
            per_zone: Vec::new(),
        }
    }
}

/// Unique names sorted, and for each input name the index of its unique name.
pub(crate) fn dedup_names<'a>(names: impl Iterator<Item = &'a str>) -> (Vec<String>, Vec<usize>) {
    let names: Vec<&str> = names.collect();
    let mut uniq: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
    uniq.sort();
    uniq.dedup();
    let index = names
        .iter()
        .map(|n| uniq.binary_search_by(|u| u.as_str().cmp(n)).unwrap_or(0))
        .collect();
    (uniq, index)
}

/// Folds service readings into per-zone maxima; returns the overall max.
pub(crate) fn fold_zones(
    raw: &[Option<f32>],
    zone_of: &[usize],
    per_zone: &mut [Option<f32>],
) -> Option<f32> {
    per_zone.iter_mut().for_each(|z| *z = None);
    let mut hottest: Option<f32> = None;
    for (v, zone) in raw.iter().zip(zone_of) {
        let Some(t) = v.filter(|t| plausible_celsius(*t)) else {
            continue;
        };
        if let Some(z) = per_zone.get_mut(*zone) {
            *z = Some(z.map_or(t, |cur| cur.max(t)));
        }
        hottest = Some(hottest.map_or(t, |h| h.max(t)));
    }
    hottest
}

impl Collector for HidThermal {
    fn id(&self) -> CollectorId {
        ID
    }

    fn cadence(&self) -> Cadence {
        Cadence::Every(crate::TEMPERATURE_PERIOD_MS)
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Sensors]
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::HidSensors]
    }

    fn probe(&mut self) -> Probe {
        self.sensors = None;
        let Some(sensors) = HidSensors::open() else {
            return Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::NoHardware,
            };
        };
        let (uniq, zone_of) = dedup_names(sensors.names());
        if uniq.is_empty() {
            return Probe::NotPresent;
        }
        let m = MetricId::from_static;
        self.zones = uniq
            .iter()
            .map(|n| SeriesKey::new(m("thermal.zone"), Labels::single("sensor", n)))
            .collect();
        self.zone_of = zone_of;
        self.per_zone = vec![None; self.zones.len()];
        self.raw = Vec::with_capacity(self.zone_of.len());
        self.sensors = Some(sensors);
        let mut series = self.zones.clone();
        series.push(self.hottest.clone());
        Probe::Supported(series)
    }

    fn sample(&mut self, _tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let sensors = self.sensors.as_ref().ok_or(CollectError::NotProbed)?;
        sensors.read(&mut self.raw);
        let hottest = fold_zones(&self.raw, &self.zone_of, &mut self.per_zone);
        for (key, v) in self.zones.iter().zip(&self.per_zone) {
            if let Some(t) = v {
                out.push(key, *t);
            }
        }
        if let Some(t) = hottest {
            out.push(&self.hottest, t);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_names_share_a_zone() {
        let (uniq, idx) = dedup_names(["PMU tdie1", "gas gauge battery", "PMU tdie1"].into_iter());
        assert_eq!(uniq, ["PMU tdie1", "gas gauge battery"]);
        assert_eq!(idx, [0, 1, 0]);
    }

    #[test]
    fn folds_to_max_and_drops_bad_reads() {
        let zone_of = [0, 1, 0, 1];
        let mut per_zone = vec![None; 2];
        // A 0 °C read (no sensor) and a NaN must not win or count.
        let raw = [Some(43.4), Some(0.0), Some(44.3), Some(f32::NAN)];
        assert_eq!(fold_zones(&raw, &zone_of, &mut per_zone), Some(44.3));
        assert_eq!(per_zone, [Some(44.3), None]);
        // An empty read leaves everything missing rather than 0.
        let raw = [None, None, None, None];
        assert_eq!(fold_zones(&raw, &zone_of, &mut per_zone), None);
        assert_eq!(per_zone, [None, None]);
    }

    /// Samples the HID thermal zones twice on real hardware.
    #[test]
    #[ignore = "needs Apple Silicon hardware; run by hand with --ignored --nocapture"]
    fn live_hid_thermal() {
        let mut c = HidThermal::new();
        let Probe::Supported(series) = c.probe() else {
            panic!("no HID temperature sensors");
        };
        println!("{} services, {} series", c.zone_of.len(), series.len());
        for n in 0..2 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let tick = Tick {
                n,
                wall_ms: 0,
                continuous_ns: 0,
                interval_ms: 1_000,
            };
            let mut out = SampleBuf::new();
            c.sample(&tick, &mut out).unwrap();
            println!("--- sample {n}");
            for s in out.values() {
                println!("  {} = {:.1}", s.key, s.value);
                assert!((20.0..=110.0).contains(&s.value), "{} {}", s.key, s.value);
            }
            let zones = out.values().iter().filter(|s| s.key != c.hottest);
            let max = zones.map(|s| s.value).fold(f32::MIN, f32::max);
            assert_eq!(out.get(&c.hottest), Some(max));
        }
    }
}

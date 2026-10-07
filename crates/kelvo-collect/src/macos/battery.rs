//! Battery: charge, charging, external power and time remaining from IOPowerSources;
//! health, cycles, capacity, power and temperature from the `AppleSmartBattery` service.
//! Sampled at most every 10 s; the slow-moving health, cycles and capacity metrics at
//! most every 60 s (v1-local-monitor.md 6.1, D-061).
//!
//! Probes `NotPresent` on Macs without an internal battery.
//!
//! Field sources, checked on macOS 27 (M3 Max MacBook Pro):
//! - Design and nominal capacity (mAh) live inside the `BatteryData` dictionary; older
//!   releases also had top-level `DesignCapacity`, which is used as a fallback.
//! - `battery.health` is `NominalChargeCapacity / DesignCapacity`. On the dev machine
//!   that is 96.4% where System Settings shows "Maximum Capacity: 97%"; Apple's exact
//!   rounding or source is not public.
//! - `battery.capacity_wh` / `battery.design_wh` multiply mAh by the present pack
//!   voltage, so they move a little with charge level. An approximation.
//! - `battery.power` is `Voltage x Amperage` (mV, signed mA), negative while discharging.
//! - `battery.temp` reads the top-level `Temperature` (hundredths of a degree C). macOS 27
//!   no longer publishes it on the dev machine, so the series is simply not probed there;
//!   SMC `TB0T` is the other source (sensors collector).

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_foundation_sys::array::CFArrayRef;
use core_foundation_sys::base::CFTypeRef;
use core_foundation_sys::dictionary::CFDictionaryRef;
use kelvo_schema::{Entitlement, MetricId, Module, SeriesKey};

use super::iokit::{self, IoObject};
use crate::{Cadence, CollectError, Collector, CollectorId, Every, Probe, SampleBuf, Tick};

/// Health, cycles and capacities change over days: read at most every 60 s.
const SLOW_MS: u32 = 60_000;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
    fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
    fn IOPSGetPowerSourceDescription(blob: CFTypeRef, ps: CFTypeRef) -> CFDictionaryRef;
}

/// The internal battery's IOPowerSources description, if there is one.
fn internal_power_source() -> Option<CFDictionary<CFString, CFType>> {
    crate::calls::count(crate::calls::Api::IoKit);
    // SAFETY: Copy rule; we own the blob (or get null).
    let blob = unsafe { IOPSCopyPowerSourcesInfo() };
    if blob.is_null() {
        return None;
    }
    // SAFETY: non-null owned CF object.
    let blob = unsafe { CFType::wrap_under_create_rule(blob) };
    // SAFETY: `blob` is the power-sources blob; Copy rule for the list.
    let list = unsafe { IOPSCopyPowerSourcesList(blob.as_CFTypeRef()) };
    if list.is_null() {
        return None;
    }
    // SAFETY: non-null owned CFArray.
    let list: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(list) };
    let k_type = CFString::from_static_string("Type");
    for ps in list.iter() {
        // SAFETY: `ps` comes from the list for this blob; Get rule, so we retain it
        // with wrap_under_get_rule before `blob` is released.
        let desc = unsafe { IOPSGetPowerSourceDescription(blob.as_CFTypeRef(), ps.as_CFTypeRef()) };
        if desc.is_null() {
            continue;
        }
        // SAFETY: non-null borrowed dictionary with string keys.
        let desc: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(desc) };
        let ty = iokit::get(&desc, &k_type).and_then(|v| iokit::as_string(&v));
        if ty.as_deref() == Some("InternalBattery") {
            return Some(desc);
        }
    }
    None
}

/// Numbers read from one sample, before they become series values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Reading {
    charge: Option<f64>,
    charging: Option<bool>,
    external: Option<bool>,
    time_remaining_min: Option<f64>,
    cycles: Option<f64>,
    design_mah: Option<f64>,
    nominal_mah: Option<f64>,
    voltage_mv: Option<f64>,
    amperage_ma: Option<f64>,
    temp_centi_c: Option<f64>,
}

impl Reading {
    fn health_pct(&self) -> Option<f64> {
        match (self.nominal_mah, self.design_mah) {
            (Some(n), Some(d)) if d > 0.0 => Some(n * 100.0 / d),
            _ => None,
        }
    }

    fn wh(&self, mah: Option<f64>) -> Option<f64> {
        Some(mah? * self.voltage_mv? / 1e6)
    }

    fn power_w(&self) -> Option<f64> {
        Some(self.voltage_mv? * self.amperage_ma? / 1e6)
    }
}

struct Keys {
    charge: SeriesKey,
    charging: SeriesKey,
    external: SeriesKey,
    time_remaining: SeriesKey,
    health: SeriesKey,
    cycles: SeriesKey,
    capacity_wh: SeriesKey,
    design_wh: SeriesKey,
    power: SeriesKey,
    temp: SeriesKey,
}

impl Keys {
    fn new() -> Self {
        let k = |id| SeriesKey::bare(MetricId::from_static(id));
        Self {
            charge: k("battery.charge"),
            charging: k("battery.charging"),
            external: k("battery.external"),
            time_remaining: k("battery.time_remaining"),
            health: k("battery.health"),
            cycles: k("battery.cycles"),
            capacity_wh: k("battery.capacity_wh"),
            design_wh: k("battery.design_wh"),
            power: k("battery.power"),
            temp: k("battery.temp"),
        }
    }
}

pub struct Battery {
    keys: Keys,
    service: Option<IoObject>,
    has_temp: bool,
    slow: Every,
}

impl Default for Battery {
    fn default() -> Self {
        Self::new()
    }
}

impl Battery {
    pub fn new() -> Self {
        Self {
            keys: Keys::new(),
            service: None,
            has_temp: false,
            slow: Every::new(SLOW_MS),
        }
    }

    fn read(&self) -> Reading {
        let mut r = Reading::default();
        if let Some(ps) = internal_power_source() {
            let num = |k: &'static str| {
                iokit::get(&ps, &CFString::from_static_string(k)).and_then(|v| iokit::as_f64(&v))
            };
            let flag = |k: &'static str| {
                iokit::get(&ps, &CFString::from_static_string(k)).and_then(|v| iokit::as_bool(&v))
            };
            if let (Some(cur), Some(max)) = (num("Current Capacity"), num("Max Capacity"))
                && max > 0.0
            {
                r.charge = Some(cur * 100.0 / max);
            }
            r.charging = flag("Is Charging");
            r.external = iokit::get(&ps, &CFString::from_static_string("Power Source State"))
                .and_then(|v| iokit::as_string(&v))
                .map(|s| s == "AC Power");
            let minutes = if r.charging == Some(true) {
                num("Time to Full Charge")
            } else {
                num("Time to Empty")
            };
            // -1 while macOS is still estimating.
            r.time_remaining_min = minutes.filter(|m| *m >= 0.0);
        }
        if let Some(props) = self.service.as_ref().and_then(IoObject::properties) {
            let top = |k: &'static str| {
                iokit::get(&props, &CFString::from_static_string(k)).and_then(|v| iokit::as_f64(&v))
            };
            let data = iokit::get(&props, &CFString::from_static_string("BatteryData"))
                .and_then(|v| iokit::dict_of(&v));
            let nested = |k: &'static str| {
                data.as_ref()
                    .and_then(|d| iokit::get(d, &CFString::from_static_string(k)))
                    .and_then(|v| iokit::as_f64(&v))
            };
            r.cycles = top("CycleCount");
            r.design_mah = nested("DesignCapacity").or_else(|| top("DesignCapacity"));
            r.nominal_mah = nested("NominalChargeCapacity")
                .or_else(|| top("NominalChargeCapacity"))
                .or_else(|| top("AppleRawMaxCapacity"));
            r.voltage_mv = top("Voltage");
            r.amperage_ma = top("Amperage");
            r.temp_centi_c = top("Temperature");
            if r.external.is_none() {
                r.external = iokit::get(&props, &CFString::from_static_string("ExternalConnected"))
                    .and_then(|v| iokit::as_bool(&v));
            }
        }
        r
    }
}

impl Collector for Battery {
    fn id(&self) -> CollectorId {
        CollectorId("battery")
    }

    fn cadence(&self) -> Cadence {
        Cadence::Every(10_000)
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Battery]
    }

    fn probe(&mut self) -> Probe {
        self.service = IoObject::services(c"AppleSmartBattery").into_iter().next();
        if internal_power_source().is_none() && self.service.is_none() {
            return Probe::NotPresent;
        }
        let r = self.read();
        self.has_temp = r.temp_centi_c.is_some();
        self.slow.reset();
        let k = &self.keys;
        let mut series = vec![
            k.charge.clone(),
            k.charging.clone(),
            k.external.clone(),
            k.time_remaining.clone(),
        ];
        if self.service.is_some() {
            series.extend([
                k.health.clone(),
                k.cycles.clone(),
                k.capacity_wh.clone(),
                k.design_wh.clone(),
                k.power.clone(),
            ]);
        }
        if self.has_temp {
            series.push(k.temp.clone());
        }
        Probe::Supported(series)
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let r = self.read();
        let k = &self.keys;
        let mut put = |key: &SeriesKey, v: Option<f64>| {
            if let Some(v) = v {
                out.push(key, v as f32);
            }
        };
        let b = |v: Option<bool>| v.map(|b| if b { 1.0 } else { 0.0 });
        put(&k.charge, r.charge);
        put(&k.charging, b(r.charging));
        put(&k.external, b(r.external));
        put(&k.time_remaining, r.time_remaining_min);
        put(&k.power, r.power_w());
        if self.has_temp {
            put(&k.temp, r.temp_centi_c.map(|t| t / 100.0));
        }
        if self.slow.due(tick) {
            put(&k.health, r.health_pct());
            put(&k.cycles, r.cycles);
            put(&k.capacity_wh, r.wh(r.nominal_mah));
            put(&k.design_wh, r.wh(r.design_mah));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_values() {
        let r = Reading {
            design_mah: Some(8579.0),
            nominal_mah: Some(8267.0),
            voltage_mv: Some(12909.0),
            amperage_ma: Some(-61.0),
            ..Reading::default()
        };
        assert!((r.health_pct().unwrap() - 96.363).abs() < 0.01);
        assert!((r.wh(r.design_mah).unwrap() - 110.746).abs() < 0.01);
        let p = r.power_w().unwrap();
        assert!(p < 0.0 && (p + 0.787).abs() < 0.001, "{p}");
        assert_eq!(Reading::default().health_pct(), None);
        let zero_design = Reading {
            design_mah: Some(0.0),
            nominal_mah: Some(1.0),
            ..Reading::default()
        };
        assert_eq!(zero_design.health_pct(), None);
    }

    #[test]
    #[ignore = "reads the live battery; run by hand on a Mac laptop"]
    fn live_smoke() {
        let mut b = Battery::new();
        let probe = b.probe();
        println!("probe: {probe:?}");
        if probe == Probe::NotPresent {
            return;
        }
        println!("reading: {:?}", b.read());
        let mut buf = SampleBuf::new();
        for n in [0, 1] {
            buf.clear();
            let tick = Tick {
                n,
                wall_ms: 0,
                continuous_ns: 0,
                interval_ms: 1_000,
            };
            b.sample(&tick, &mut buf).unwrap();
        }
        for s in buf.values() {
            println!("{} = {:.2}", s.key, s.value);
        }
        assert!(buf.get(&b.keys.charge).is_some());
    }
}

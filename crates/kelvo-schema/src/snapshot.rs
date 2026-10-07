//! The typed `Snapshot` view, built from one frame of series values (architecture.md
//! infra 1).
//!
//! The Snapshot is a convenience for the tray and the UI (`snap.cpu.total` instead of a
//! lookup). It is never stored or synced; changing its shape changes no data on disk or on
//! the wire.
//!
//! The rule that matters: a missing series yields `None`, never `0.0`. A zero is a
//! measurement; a `None` is a gap. In a frame, `NaN` means "in the layout but not sampled
//! this tick" (a gauge sampled every 10 ticks is `NaN` on the other nine), and that also
//! becomes `None`.
//!
//! A module view is `Some` when the frame's layout has at least one series of that module,
//! even if every value is `NaN` this tick, and `None` when it has none. Labelled series
//! (cores, interfaces, zones) appear in layout order with `None` values when unsampled.
//! Series the catalog does not know, or whose labels do not match their definition, are
//! skipped.

use serde::Serialize;

use crate::alert::ThermalState;
use crate::catalog::{Catalog, Module};
use crate::host::HostId;
use crate::series::SeriesKey;

/// One frame of values: the layout's series keys and one `f32` per key, `NaN` for
/// "not sampled this tick".
#[derive(Clone, Copy, Debug)]
pub struct FrameView<'a> {
    ts_ms: i64,
    series: &'a [SeriesKey],
    values: &'a [f32],
}

impl<'a> FrameView<'a> {
    /// Fails when `series` and `values` have different lengths.
    pub fn new(
        ts_ms: i64,
        series: &'a [SeriesKey],
        values: &'a [f32],
    ) -> Result<Self, FrameLenMismatch> {
        if series.len() != values.len() {
            return Err(FrameLenMismatch {
                series: series.len(),
                values: values.len(),
            });
        }
        Ok(Self {
            ts_ms,
            series,
            values,
        })
    }

    pub fn ts_ms(&self) -> i64 {
        self.ts_ms
    }

    /// `(key, value)` pairs in layout order, `NaN` included.
    pub fn iter(&self) -> impl Iterator<Item = (&'a SeriesKey, f32)> + 'a {
        self.series.iter().zip(self.values.iter().copied())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("frame has {series} series but {values} values")]
pub struct FrameLenMismatch {
    pub series: usize,
    pub values: usize,
}

fn val(v: f32) -> Option<f32> {
    v.is_finite().then_some(v)
}

/// A labelled value, for series lists such as thermal zones or load averages.
#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct LabeledValue {
    pub label: String,
    pub value: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct Snapshot {
    pub host: HostId,
    /// Exported to TS as `number`; see [`crate::JS_SAFE_INT_MAX`].
    #[specta(type = crate::JsSafeInt)]
    pub ts_ms: i64,
    /// `None` means the frame has no series for the module.
    pub cpu: Option<CpuView>,
    pub gpu: Option<GpuView>,
    pub memory: Option<MemoryView>,
    pub power: Option<PowerView>,
    pub sensors: Option<SensorsView>,
    pub network: Option<NetworkView>,
    pub disk: Option<DiskView>,
    pub battery: Option<BatteryView>,
    /// `self.cpu`: Kelvo's own CPU across the app and its WebKit helpers.
    pub self_cpu: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct CpuView {
    pub total: Option<f32>,
    pub user: Option<f32>,
    pub system: Option<f32>,
    /// `cpu.loadavg`, labelled by window ("1", "5", "15").
    pub load_avg: Vec<LabeledValue>,
    pub clusters: Vec<ClusterView>,
    pub cores: Vec<CoreView>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct ClusterView {
    /// The `cluster` label, matching `ClusterInfo::name`.
    pub cluster: String,
    pub freq_hz: Option<f32>,
    pub active: Option<f32>,
    pub power_w: Option<f32>,
    /// `cpu.cluster.residency`, labelled by state (MHz or "idle").
    pub residency: Vec<LabeledValue>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct CoreView {
    /// The `core` label ("P0", "E3"), listed in `ClusterInfo::cores`.
    pub core: String,
    pub load: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct GpuView {
    pub util: Option<f32>,
    pub render: Option<f32>,
    pub tiler: Option<f32>,
    pub freq_hz: Option<f32>,
    pub residency: Vec<LabeledValue>,
}

/// Byte values are `f32`, exact to within 0.1% at any realistic size.
#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct MemoryView {
    pub used: Option<f32>,
    pub app: Option<f32>,
    pub wired: Option<f32>,
    pub compressed: Option<f32>,
    pub cached: Option<f32>,
    pub free: Option<f32>,
    pub pressure: Option<f32>,
    /// 0 normal, 1 warn, 2 critical.
    pub pressure_level: Option<f32>,
    pub swap_used: Option<f32>,
    pub swap_in: Option<f32>,
    pub swap_out: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct PowerView {
    pub cpu: Option<f32>,
    pub gpu: Option<f32>,
    pub ane: Option<f32>,
    pub dram: Option<f32>,
    pub package: Option<f32>,
    pub system: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct SensorsView {
    pub cpu_c: Option<f32>,
    pub gpu_c: Option<f32>,
    pub hottest_c: Option<f32>,
    /// `None` when unsampled or out of range.
    pub thermal_state: Option<ThermalState>,
    /// `thermal.zone`, labelled by raw HID sensor name.
    pub zones: Vec<LabeledValue>,
    /// `thermal.sensor`, labelled by name (battery, ssd, wifi).
    pub sensors: Vec<LabeledValue>,
    pub fans: Vec<FanView>,
    pub fan_mode: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct FanView {
    pub fan: String,
    pub rpm: Option<f32>,
    pub max_rpm: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct NetworkView {
    pub interfaces: Vec<InterfaceView>,
    /// `net.rx_total`: the sum over the reported interfaces, a gap when any is (D-092).
    pub rx_total_bps: Option<f32>,
    /// `net.tx_total`, gated like `rx_total_bps`.
    pub tx_total_bps: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct InterfaceView {
    pub iface: String,
    pub rx_bps: Option<f32>,
    pub tx_bps: Option<f32>,
    /// Bits per second.
    pub link_rate: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct DiskView {
    pub devices: Vec<DiskDeviceView>,
    pub volumes: Vec<VolumeView>,
    /// `disk.read_total`: the sum over the reported devices, a gap when any is (D-092).
    pub read_total_bps: Option<f32>,
    /// `disk.write_total`, gated like `read_total_bps`.
    pub write_total_bps: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct DiskDeviceView {
    pub dev: String,
    pub read_bps: Option<f32>,
    pub write_bps: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct VolumeView {
    pub vol: String,
    pub used: Option<f32>,
    pub free: Option<f32>,
    pub total: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
pub struct BatteryView {
    pub charge: Option<f32>,
    pub charging: Option<bool>,
    pub external: Option<bool>,
    pub time_remaining_min: Option<f32>,
    pub health: Option<f32>,
    pub cycles: Option<f32>,
    pub capacity_wh: Option<f32>,
    pub design_wh: Option<f32>,
    /// Signed watts, negative while discharging.
    pub power_w: Option<f32>,
    pub temp_c: Option<f32>,
}

/// Finds the entry with `label` or appends a new one.
fn entry<'v, T: Default>(
    list: &'v mut Vec<T>,
    label: &str,
    get: impl Fn(&T) -> &str,
    make: impl FnOnce(String) -> T,
) -> &'v mut T {
    let pos = match list.iter().position(|t| get(t) == label) {
        Some(p) => p,
        None => {
            list.push(make(label.to_owned()));
            list.len() - 1
        }
    };
    // `pos` is either a found index or the index of the element just pushed.
    &mut list[pos]
}

fn labeled(list: &mut Vec<LabeledValue>, label: &str, v: Option<f32>) {
    entry(
        list,
        label,
        |l| &l.label,
        |label| LabeledValue { label, value: None },
    )
    .value = v;
}

impl Snapshot {
    /// Builds the view from one frame. Missing series produce `None` fields, never zeros.
    pub fn from_frame(host: HostId, frame: &FrameView<'_>, catalog: &Catalog) -> Snapshot {
        let mut s = Snapshot {
            host,
            ts_ms: frame.ts_ms(),
            cpu: None,
            gpu: None,
            memory: None,
            power: None,
            sensors: None,
            network: None,
            disk: None,
            battery: None,
            self_cpu: None,
        };
        for (key, raw) in frame.iter() {
            let Ok(def) = catalog.validate(key) else {
                continue;
            };
            let v = val(raw);
            let label = key.labels.iter().next().map(|(_, v)| v).unwrap_or_default();
            let id = def.id.as_str();
            if id == "self.cpu" {
                s.self_cpu = v;
                continue;
            }
            match def.module {
                Module::Cpu => {
                    let c = s.cpu.get_or_insert_with(Default::default);
                    match id {
                        "cpu.total" => c.total = v,
                        "cpu.user" => c.user = v,
                        "cpu.system" => c.system = v,
                        "cpu.loadavg" => labeled(&mut c.load_avg, label, v),
                        "cpu.load" => {
                            let mk = |core| CoreView { core, load: None };
                            entry(&mut c.cores, label, |x| &x.core, mk).load = v;
                        }
                        "cpu.cluster.freq"
                        | "cpu.cluster.active"
                        | "cpu.cluster.power"
                        | "cpu.cluster.residency" => {
                            let name = key.labels.get("cluster").unwrap_or_default();
                            let mk = |cluster| ClusterView {
                                cluster,
                                ..Default::default()
                            };
                            let cl = entry(&mut c.clusters, name, |x| &x.cluster, mk);
                            match id {
                                "cpu.cluster.freq" => cl.freq_hz = v,
                                "cpu.cluster.active" => cl.active = v,
                                "cpu.cluster.power" => cl.power_w = v,
                                _ => {
                                    let state = key.labels.get("state").unwrap_or_default();
                                    labeled(&mut cl.residency, state, v);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Module::Gpu => {
                    let g = s.gpu.get_or_insert_with(Default::default);
                    match id {
                        "gpu.util" => g.util = v,
                        "gpu.render" => g.render = v,
                        "gpu.tiler" => g.tiler = v,
                        "gpu.freq" => g.freq_hz = v,
                        "gpu.residency" => labeled(&mut g.residency, label, v),
                        _ => {}
                    }
                }
                Module::Memory => {
                    let m = s.memory.get_or_insert_with(Default::default);
                    let slot = match id {
                        "mem.used" => &mut m.used,
                        "mem.app" => &mut m.app,
                        "mem.wired" => &mut m.wired,
                        "mem.compressed" => &mut m.compressed,
                        "mem.cached" => &mut m.cached,
                        "mem.free" => &mut m.free,
                        "mem.pressure" => &mut m.pressure,
                        "mem.pressure_level" => &mut m.pressure_level,
                        "mem.swap_used" => &mut m.swap_used,
                        "mem.swap_in" => &mut m.swap_in,
                        "mem.swap_out" => &mut m.swap_out,
                        _ => continue,
                    };
                    *slot = v;
                }
                Module::Power => {
                    let p = s.power.get_or_insert_with(Default::default);
                    let slot = match id {
                        "power.cpu" => &mut p.cpu,
                        "power.gpu" => &mut p.gpu,
                        "power.ane" => &mut p.ane,
                        "power.dram" => &mut p.dram,
                        "power.package" => &mut p.package,
                        "power.system" => &mut p.system,
                        _ => continue,
                    };
                    *slot = v;
                }
                Module::Sensors => {
                    let t = s.sensors.get_or_insert_with(Default::default);
                    match id {
                        "thermal.cpu" => t.cpu_c = v,
                        "thermal.gpu" => t.gpu_c = v,
                        "thermal.hottest" => t.hottest_c = v,
                        "thermal.state" => t.thermal_state = v.and_then(ThermalState::from_value),
                        "thermal.zone" => labeled(&mut t.zones, label, v),
                        "thermal.sensor" => labeled(&mut t.sensors, label, v),
                        "fan.mode" => t.fan_mode = v,
                        "fan.rpm" | "fan.max" => {
                            let mk = |fan| FanView {
                                fan,
                                ..Default::default()
                            };
                            let f = entry(&mut t.fans, label, |x| &x.fan, mk);
                            if id == "fan.rpm" {
                                f.rpm = v;
                            } else {
                                f.max_rpm = v;
                            }
                        }
                        _ => {}
                    }
                }
                Module::Network => {
                    let n = s.network.get_or_insert_with(Default::default);
                    match id {
                        "net.rx_total" => {
                            n.rx_total_bps = v;
                            continue;
                        }
                        "net.tx_total" => {
                            n.tx_total_bps = v;
                            continue;
                        }
                        _ => {}
                    }
                    let mk = |iface| InterfaceView {
                        iface,
                        ..Default::default()
                    };
                    let i = entry(&mut n.interfaces, label, |x| &x.iface, mk);
                    match id {
                        "net.rx" => i.rx_bps = v,
                        "net.tx" => i.tx_bps = v,
                        "net.link_rate" => i.link_rate = v,
                        _ => {}
                    }
                }
                Module::Disk => {
                    let d = s.disk.get_or_insert_with(Default::default);
                    match id {
                        "disk.read_total" => d.read_total_bps = v,
                        "disk.write_total" => d.write_total_bps = v,
                        "disk.read" | "disk.write" => {
                            let mk = |dev| DiskDeviceView {
                                dev,
                                ..Default::default()
                            };
                            let dv = entry(&mut d.devices, label, |x| &x.dev, mk);
                            if id == "disk.read" {
                                dv.read_bps = v;
                            } else {
                                dv.write_bps = v;
                            }
                        }
                        "disk.used" | "disk.free" | "disk.total" => {
                            let mk = |vol| VolumeView {
                                vol,
                                ..Default::default()
                            };
                            let vv = entry(&mut d.volumes, label, |x| &x.vol, mk);
                            match id {
                                "disk.used" => vv.used = v,
                                "disk.free" => vv.free = v,
                                _ => vv.total = v,
                            }
                        }
                        _ => {}
                    }
                }
                Module::Battery => {
                    let b = s.battery.get_or_insert_with(Default::default);
                    let flag = v.map(|x| x != 0.0);
                    match id {
                        "battery.charge" => b.charge = v,
                        "battery.charging" => b.charging = flag,
                        "battery.external" => b.external = flag,
                        "battery.time_remaining" => b.time_remaining_min = v,
                        "battery.health" => b.health = v,
                        "battery.cycles" => b.cycles = v,
                        "battery.capacity_wh" => b.capacity_wh = v,
                        "battery.design_wh" => b.design_wh = v,
                        "battery.power" => b.power_w = v,
                        "battery.temp" => b.temp_c = v,
                        _ => {}
                    }
                }
                // The catalog never files a metric under Unknown.
                Module::Unknown => {}
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn keys(ks: &[&str]) -> Vec<SeriesKey> {
        ks.iter().map(|k| SeriesKey::parse(k).unwrap()).collect()
    }

    fn snap(ks: &[&str], vs: &[f32]) -> Snapshot {
        let keys = keys(ks);
        let frame = FrameView::new(1_000, &keys, vs).unwrap();
        Snapshot::from_frame(HostId(Uuid::nil()), &frame, &Catalog::builtin())
    }

    #[test]
    fn missing_series_is_none_never_zero() {
        let s = snap(&["cpu.total"], &[12.5]);
        let cpu = s.cpu.unwrap();
        assert_eq!(cpu.total, Some(12.5));
        assert_eq!(cpu.user, None);
        assert_eq!(cpu.system, None);
        // Modules with no series in the frame are absent, not zero-filled.
        assert!(s.gpu.is_none() && s.memory.is_none() && s.power.is_none());
        assert!(s.sensors.is_none() && s.network.is_none() && s.disk.is_none());
        assert!(s.battery.is_none());
        assert_eq!(s.self_cpu, None);
    }

    #[test]
    fn nan_is_none_and_zero_is_some() {
        let s = snap(&["power.cpu", "power.gpu"], &[f32::NAN, 0.0]);
        let p = s.power.unwrap();
        assert_eq!(p.cpu, None);
        assert_eq!(p.gpu, Some(0.0));
    }

    #[test]
    fn module_present_when_all_values_unsampled() {
        let s = snap(&["disk.used{vol=/}"], &[f32::NAN]);
        let d = s.disk.unwrap();
        assert_eq!(d.volumes.len(), 1);
        assert_eq!(d.volumes[0].used, None);
    }

    #[test]
    fn fills_labelled_views_in_layout_order() {
        let s = snap(
            &[
                "cpu.load{core=P0}",
                "cpu.load{core=E0}",
                "cpu.cluster.freq{cluster=P0}",
                "cpu.cluster.residency{cluster=P0,state=idle}",
                "cpu.cluster.active{cluster=P0}",
                "fan.rpm{fan=0}",
                "fan.max{fan=0}",
                "net.rx{iface=en0}",
                "net.tx{iface=en0}",
                "thermal.state",
                "battery.charging",
                "self.cpu",
            ],
            &[
                40.0, 5.0, 3.2e9, 60.0, 40.0, 1800.0, 6000.0, 1e6, 2e5, 2.0, 1.0, 0.4,
            ],
        );
        let cpu = s.cpu.unwrap();
        let cores: Vec<_> = cpu
            .cores
            .iter()
            .map(|c| (c.core.as_str(), c.load))
            .collect();
        assert_eq!(cores, [("P0", Some(40.0)), ("E0", Some(5.0))]);
        assert_eq!(cpu.clusters.len(), 1);
        assert_eq!(cpu.clusters[0].freq_hz, Some(3.2e9));
        assert_eq!(cpu.clusters[0].active, Some(40.0));
        assert_eq!(cpu.clusters[0].power_w, None);
        assert_eq!(cpu.clusters[0].residency[0].label, "idle");
        let sensors = s.sensors.unwrap();
        assert_eq!(sensors.fans[0].rpm, Some(1800.0));
        assert_eq!(sensors.fans[0].max_rpm, Some(6000.0));
        assert_eq!(sensors.thermal_state, Some(ThermalState::Serious));
        let en0 = &s.network.unwrap().interfaces[0];
        assert_eq!(
            (en0.rx_bps, en0.tx_bps, en0.link_rate),
            (Some(1e6), Some(2e5), None)
        );
        assert_eq!(s.battery.unwrap().charging, Some(true));
        assert_eq!(s.self_cpu, Some(0.4));
    }

    #[test]
    fn totals_are_module_fields_not_unnamed_rows() {
        let s = snap(
            &[
                "net.rx{iface=en0}",
                "net.rx_total",
                "net.tx_total",
                "disk.read{dev=disk0}",
                "disk.read_total",
                "disk.write_total",
            ],
            &[1e6, 1e6, f32::NAN, 5.0, 5.0, 0.0],
        );
        let n = s.network.unwrap();
        assert_eq!(n.interfaces.len(), 1);
        assert_eq!((n.rx_total_bps, n.tx_total_bps), (Some(1e6), None));
        let d = s.disk.unwrap();
        assert_eq!(d.devices.len(), 1);
        assert_eq!(
            (d.read_total_bps, d.write_total_bps),
            (Some(5.0), Some(0.0))
        );
    }

    #[test]
    fn unknown_and_mislabelled_series_are_skipped() {
        let s = snap(
            &["gpu.quantum{x=1}", "cpu.load", "mem.used"],
            &[1.0, 2.0, 3.0],
        );
        assert!(
            s.gpu.is_none(),
            "unknown metric must not create a module view"
        );
        assert!(
            s.cpu.is_none(),
            "cpu.load without its core label is skipped"
        );
        assert_eq!(s.memory.unwrap().used, Some(3.0));
    }

    #[test]
    fn frame_lengths_must_match() {
        let k = keys(&["cpu.total"]);
        assert!(FrameView::new(0, &k, &[]).is_err());
    }
}

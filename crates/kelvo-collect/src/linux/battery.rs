//! Battery from `/sys/class/power_supply` (D-116): only supplies with `type` Battery and
//! a `scope` other than Device (a wireless mouse's battery is a Device), and `present`
//! not 0. The first by name is reported (`BAT0`); a second pack is not summed yet.
//! Sampled at most every 10 s; health, cycles and capacities at most every 60 s, as on
//! macOS.
//!
//! Fields, each probed only when it reads:
//! - `battery.charge`: `capacity` (%).
//! - `battery.charging`: `status` is Charging. Unknown status is no value.
//! - `battery.external`: any Mains or USB supply reports `online` 1; without such a
//!   supply, a status other than Discharging.
//! - `battery.power`: `power_now` (µW), else `current_now` (µA) x `voltage_now` (µV).
//!   Drivers disagree on the sign, so the magnitude is signed by `status`: negative
//!   while Discharging.
//! - `battery.temp`: `temp`, tenths of a degree (few laptops have it).
//! - `battery.health`, `battery.capacity_wh`, `battery.design_wh`: `energy_full` and
//!   `energy_full_design` (µWh), or the `charge_*` (µAh) pair times
//!   `voltage_min_design`.
//! - `battery.cycles`: `cycle_count`, when above 0 (drivers without a count report 0).
//!
//! `battery.time_remaining` is not derived: sysfs has no estimate, and computing one
//! from instantaneous power is a later decision. A host without a battery (the M1 mini
//! has only USB-PD supplies) probes `NotPresent`.

use std::path::{Path, PathBuf};

use kelvo_schema::{Entitlement, MetricId, Module, SeriesKey};

use super::SysRoot;
use super::hwmon::read_i64_at;
use crate::{Cadence, CollectError, Collector, CollectorId, Every, Probe, SampleBuf, Tick};

const SLOW_MS: u32 = 60_000;

/// Reads a short text attribute (`status`) into `buf` without allocating.
fn read_word<'a>(path: &Path, buf: &'a mut [u8; 32]) -> Option<&'a str> {
    use std::io::Read;
    let n = std::fs::File::open(path).ok()?.read(buf).ok()?;
    Some(std::str::from_utf8(buf.get(..n)?).ok()?.trim())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Charging,
    Discharging,
    /// Full or Not charging: on external power, not charging.
    Idle,
}

fn parse_status(s: &str) -> Option<Status> {
    match s {
        "Charging" => Some(Status::Charging),
        "Discharging" => Some(Status::Discharging),
        "Full" | "Not charging" => Some(Status::Idle),
        _ => None,
    }
}

/// The attribute paths of one battery, built at probe.
struct Files {
    capacity: PathBuf,
    status: PathBuf,
    power_now: PathBuf,
    current_now: PathBuf,
    voltage_now: PathBuf,
    temp: PathBuf,
    energy_full: PathBuf,
    energy_design: PathBuf,
    charge_full: PathBuf,
    charge_design: PathBuf,
    voltage_min: PathBuf,
    cycles: PathBuf,
    /// `online` of each Mains/USB supply.
    online: Vec<PathBuf>,
}

impl Files {
    fn at(dir: &Path, online: Vec<PathBuf>) -> Self {
        let f = |n: &str| dir.join(n);
        Self {
            capacity: f("capacity"),
            status: f("status"),
            power_now: f("power_now"),
            current_now: f("current_now"),
            voltage_now: f("voltage_now"),
            temp: f("temp"),
            energy_full: f("energy_full"),
            energy_design: f("energy_full_design"),
            charge_full: f("charge_full"),
            charge_design: f("charge_full_design"),
            voltage_min: f("voltage_min_design"),
            cycles: f("cycle_count"),
            online,
        }
    }

    fn num(p: &Path) -> Option<f64> {
        read_i64_at(p).map(|v| v as f64)
    }

    fn status(&self) -> Option<Status> {
        let mut buf = [0u8; 32];
        parse_status(read_word(&self.status, &mut buf)?)
    }

    fn charge(&self) -> Option<f64> {
        Self::num(&self.capacity).filter(|c| (0.0..=100.0).contains(c))
    }

    fn external(&self, status: Option<Status>) -> Option<bool> {
        let mut any = None;
        for p in &self.online {
            if let Some(v) = read_i64_at(p) {
                any = Some(any.unwrap_or(false) || v == 1);
            }
        }
        any.or_else(|| status.map(|s| s != Status::Discharging))
    }

    fn power(&self, status: Option<Status>) -> Option<f64> {
        let w = Self::num(&self.power_now)
            .map(|uw| uw.abs() / 1e6)
            .or_else(|| {
                let (a, v) = (Self::num(&self.current_now)?, Self::num(&self.voltage_now)?);
                Some((a * v).abs() / 1e12)
            })?;
        Some(match status? {
            Status::Discharging => -w,
            Status::Charging | Status::Idle => w,
        })
    }

    fn temp(&self) -> Option<f64> {
        Self::num(&self.temp)
            .map(|t| t / 10.0)
            .filter(|t| *t > 0.0 && *t <= 100.0)
    }

    /// Full and design capacity in Wh.
    fn capacities(&self) -> (Option<f64>, Option<f64>) {
        let energy = |p: &Path| Self::num(p).filter(|v| *v > 0.0).map(|v| v / 1e6);
        let volts = Self::num(&self.voltage_min).filter(|v| *v > 0.0);
        let charge = |p: &Path| Some(Self::num(p).filter(|v| *v > 0.0)? * volts? / 1e12);
        (
            energy(&self.energy_full).or_else(|| charge(&self.charge_full)),
            energy(&self.energy_design).or_else(|| charge(&self.charge_design)),
        )
    }

    fn cycles(&self) -> Option<f64> {
        Self::num(&self.cycles).filter(|c| *c > 0.0)
    }
}

struct Keys {
    charge: SeriesKey,
    charging: SeriesKey,
    external: SeriesKey,
    power: SeriesKey,
    temp: SeriesKey,
    health: SeriesKey,
    cycles: SeriesKey,
    capacity_wh: SeriesKey,
    design_wh: SeriesKey,
}

impl Keys {
    fn new() -> Self {
        let k = |id| SeriesKey::bare(MetricId::from_static(id));
        Self {
            charge: k("battery.charge"),
            charging: k("battery.charging"),
            external: k("battery.external"),
            power: k("battery.power"),
            temp: k("battery.temp"),
            health: k("battery.health"),
            cycles: k("battery.cycles"),
            capacity_wh: k("battery.capacity_wh"),
            design_wh: k("battery.design_wh"),
        }
    }
}

/// One sample's values, keyed, given the battery's `status`. `slow` adds the 60 s fields.
fn readings<'k>(
    f: &Files,
    k: &'k Keys,
    status: Option<Status>,
    slow: bool,
) -> [(&'k SeriesKey, Option<f64>); 9] {
    let bool_f = |b: bool| if b { 1.0 } else { 0.0 };
    let (full, design) = if slow { f.capacities() } else { (None, None) };
    let health = match (full, design) {
        (Some(a), Some(d)) => Some(a * 100.0 / d),
        _ => None,
    };
    [
        (&k.charge, f.charge()),
        (&k.charging, status.map(|s| bool_f(s == Status::Charging))),
        (&k.external, f.external(status).map(bool_f)),
        (&k.power, f.power(status)),
        (&k.temp, f.temp()),
        (&k.health, health),
        (&k.cycles, if slow { f.cycles() } else { None }),
        (&k.capacity_wh, full),
        (&k.design_wh, design),
    ]
}

/// The battery's directory and the external supplies' `online` files.
fn find(root: &SysRoot) -> Option<(PathBuf, Vec<PathBuf>)> {
    let base = root.path("/sys/class/power_supply");
    let mut names: Vec<String> = std::fs::read_dir(&base)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    names.sort();
    let attr = |n: &str, a: &str| root.read_trimmed(&format!("/sys/class/power_supply/{n}/{a}"));
    let battery = names.iter().find(|n| {
        attr(n, "type").as_deref() == Some("Battery")
            && attr(n, "scope").as_deref() != Some("Device")
            && attr(n, "present").as_deref() != Some("0")
    })?;
    let online = names
        .iter()
        .filter(|n| {
            attr(n, "type").is_some_and(|t| t == "Mains" || t.starts_with("USB"))
                && attr(n, "scope").as_deref() != Some("Device")
        })
        .map(|n| base.join(n).join("online"))
        .filter(|p| p.exists())
        .collect();
    Some((base.join(battery), online))
}

/// `battery.*` from the first system battery.
pub struct Battery {
    root: SysRoot,
    files: Option<Files>,
    keys: Keys,
    slow: Every,
}

impl Default for Battery {
    fn default() -> Self {
        Self::new()
    }
}

impl Battery {
    pub fn new() -> Self {
        Self::at(SysRoot::host())
    }

    fn at(root: SysRoot) -> Self {
        Self {
            root,
            files: None,
            keys: Keys::new(),
            slow: Every::new(SLOW_MS),
        }
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
        self.slow.reset();
        self.files = None;
        let Some((dir, online)) = find(&self.root) else {
            return Probe::NotPresent;
        };
        let files = Files::at(&dir, online);
        // A status the driver reports as Unknown right now still makes the
        // status-derived series available: they are gaps until it is known.
        let mut buf = [0u8; 32];
        let status = files.status().or_else(|| {
            read_word(&files.status, &mut buf)
                .filter(|s| !s.is_empty())
                .map(|_| Status::Idle)
        });
        let series: Vec<SeriesKey> = readings(&files, &self.keys, status, true)
            .into_iter()
            .filter(|(_, v)| v.is_some())
            .map(|(k, _)| k.clone())
            .collect();
        self.files = Some(files);
        Probe::Supported(series)
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let files = self.files.as_ref().ok_or(CollectError::NotProbed)?;
        let slow = self.slow.due(tick);
        for (key, v) in readings(files, &self.keys, files.status(), slow) {
            if let Some(v) = v {
                out.push(key, v as f32);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testdata::{asahi_t0, tree, write};
    use super::*;

    fn tick(secs: u64) -> Tick {
        Tick {
            n: secs,
            wall_ms: 0,
            continuous_ns: secs * 1_000_000_000,
            interval_ms: 1_000,
        }
    }

    #[test]
    fn usb_pd_supplies_are_not_a_battery() {
        assert_eq!(Battery::at(asahi_t0()).probe(), Probe::NotPresent);
    }

    fn laptop() -> tempfile::TempDir {
        tree(&[
            ("sys/class/power_supply/AC/type", "Mains\n"),
            ("sys/class/power_supply/AC/online", "0\n"),
            ("sys/class/power_supply/macsmc-battery/type", "Battery\n"),
            ("sys/class/power_supply/macsmc-battery/present", "1\n"),
            (
                "sys/class/power_supply/macsmc-battery/status",
                "Discharging\n",
            ),
            ("sys/class/power_supply/macsmc-battery/capacity", "81\n"),
            (
                "sys/class/power_supply/macsmc-battery/power_now",
                "7250000\n",
            ),
            (
                "sys/class/power_supply/macsmc-battery/energy_full",
                "45000000\n",
            ),
            (
                "sys/class/power_supply/macsmc-battery/energy_full_design",
                "50000000\n",
            ),
            ("sys/class/power_supply/macsmc-battery/cycle_count", "312\n"),
            ("sys/class/power_supply/hidpp_battery_0/type", "Battery\n"),
            ("sys/class/power_supply/hidpp_battery_0/scope", "Device\n"),
            ("sys/class/power_supply/hidpp_battery_0/capacity", "5\n"),
        ])
    }

    #[test]
    fn laptop_battery_values() {
        let dir = laptop();
        let mut c = Battery::at(SysRoot::at(dir.path()));
        let Probe::Supported(series) = c.probe() else {
            panic!("probe");
        };
        let k = Keys::new();
        assert_eq!(
            series,
            [
                k.charge.clone(),
                k.charging.clone(),
                k.external.clone(),
                k.power.clone(),
                k.health.clone(),
                k.cycles.clone(),
                k.capacity_wh.clone(),
                k.design_wh.clone(),
            ],
            "no temp file"
        );
        let mut buf = SampleBuf::new();
        c.sample(&tick(0), &mut buf).unwrap();
        assert_eq!(buf.get(&k.charge), Some(81.0), "not the mouse's 5%");
        assert_eq!(buf.get(&k.charging), Some(0.0));
        assert_eq!(buf.get(&k.external), Some(0.0));
        assert_eq!(buf.get(&k.power), Some(-7.25));
        assert_eq!(buf.get(&k.health), Some(90.0));
        assert_eq!(buf.get(&k.cycles), Some(312.0));
        assert_eq!(buf.get(&k.capacity_wh), Some(45.0));
        assert_eq!(buf.get(&k.design_wh), Some(50.0));

        // Plugged in: charging, external, positive power; the slow fields wait 60 s.
        write(&dir, "sys/class/power_supply/AC/online", "1\n");
        write(
            &dir,
            "sys/class/power_supply/macsmc-battery/status",
            "Charging\n",
        );
        buf.clear();
        c.sample(&tick(10), &mut buf).unwrap();
        assert_eq!(buf.get(&k.charging), Some(1.0));
        assert_eq!(buf.get(&k.external), Some(1.0));
        assert_eq!(buf.get(&k.power), Some(7.25));
        assert_eq!(buf.get(&k.health), None);
        assert_eq!(buf.get(&k.cycles), None);
    }

    /// The `charge_*` (µAh) drivers: capacity from charge x minimum design voltage, power
    /// from current x voltage; an Unknown status gives no charging value and no signed
    /// power, and a zero cycle count is no count.
    #[test]
    fn charge_based_battery() {
        let dir = tree(&[
            ("sys/class/power_supply/BAT1/type", "Battery\n"),
            ("sys/class/power_supply/BAT1/status", "Unknown\n"),
            ("sys/class/power_supply/BAT1/capacity", "50\n"),
            ("sys/class/power_supply/BAT1/current_now", "-1500000\n"),
            ("sys/class/power_supply/BAT1/voltage_now", "12000000\n"),
            ("sys/class/power_supply/BAT1/charge_full", "4000000\n"),
            (
                "sys/class/power_supply/BAT1/charge_full_design",
                "5000000\n",
            ),
            (
                "sys/class/power_supply/BAT1/voltage_min_design",
                "11100000\n",
            ),
            ("sys/class/power_supply/BAT1/cycle_count", "0\n"),
            ("sys/class/power_supply/BAT1/temp", "305\n"),
        ]);
        let mut c = Battery::at(SysRoot::at(dir.path()));
        c.probe();
        let k = Keys::new();
        let mut buf = SampleBuf::new();
        c.sample(&tick(0), &mut buf).unwrap();
        assert_eq!(buf.get(&k.charging), None);
        assert_eq!(buf.get(&k.power), None);
        assert_eq!(buf.get(&k.external), None);
        assert_eq!(buf.get(&k.cycles), None);
        assert_eq!(buf.get(&k.temp), Some(30.5));
        assert_eq!(buf.get(&k.health), Some(80.0));
        let wh = buf.get(&k.capacity_wh).unwrap();
        assert!((wh - 44.4).abs() < 1e-4, "{wh}");
        write(&dir, "sys/class/power_supply/BAT1/status", "Discharging\n");
        buf.clear();
        c.sample(&tick(10), &mut buf).unwrap();
        assert_eq!(buf.get(&k.power), Some(-18.0));
    }

    #[test]
    fn empty_bay_is_not_present() {
        let dir = tree(&[
            ("sys/class/power_supply/macsmc-battery/type", "Battery\n"),
            ("sys/class/power_supply/macsmc-battery/present", "0\n"),
        ]);
        assert_eq!(
            Battery::at(SysRoot::at(dir.path())).probe(),
            Probe::NotPresent
        );
    }
}

//! hwmon and thermal-zone sensors (D-115): `thermal.sensor{name}`, `thermal.hottest`,
//! `fan.rpm{fan}`/`fan.max{fan}` ([`Hwmon::sensors`], Sensors module) and
//! `power.sensor{name}`/`power.system` ([`Hwmon::power`], Power module). Two collectors
//! over one scan, so a host with temperatures but no power sensors reports Power as not
//! present instead of as available with no series.
//!
//! Inputs, from `/sys/class/hwmon/hwmon*`:
//! - `temp<N>_input`, millidegrees C. Kept when it reads in (0, 150] °C at probe and on
//!   each sample, the bounds the macOS sensors use: 0 and below means no sensor, above
//!   150 a bad read. The M1 mini's `tas2770` amplifier reads -92 °C and is left out.
//! - `fan<N>_input` (rpm, 0 is a stopped fan) and `fan<N>_max`, read at most every 60 s.
//! - `power<N>_input` (µW), or `power<N>_average` when the chip has no `_input` for that
//!   channel (older amdgpu). `curr*` and `in*` are current and voltage, never power.
//!
//! A temperature or power sensor is named by its kernel label alone ("NAND Flash
//! Temperature", "Total System Power"), as the macOS names read. It is qualified as
//! `<chip>/<label>` when the label names no part ([`generic_label`]: `Composite`,
//! `Sensor 2`, amdgpu's `edge`) or when another chip on the host carries the same label;
//! without a label it is `<chip>/temp<N>`. A chip name that more than one hwmon directory
//! carries is qualified by its parent device (`nvme@nvme0`, `jc42@1-0018`), which
//! survives a reboot where the `hwmon<N>` number does not. Anything still over the label
//! cap is bounded by [`bound_label_value`]. Fans are labelled `0`, `1`, ... (hwmon `fan1` is
//! `0`), the macOS scheme, when one chip carries every fan; otherwise `<chip>/<N-1>`.
//!
//! Thermal zones (`/sys/class/thermal/thermal_zone*`) add `thermal.sensor{name =
//! "zone/<type>"}`, `zone/<type>@<N>` when a type repeats (zone numbers have no better
//! stable name). A zone that registered its own hwmon device (a `hwmon*` child) is read
//! through hwmon only.
//!
//! `thermal.hottest` is the max over CPU/SoC-class sensors only ([`cpu_class_chip`],
//! [`cpu_class_zone`]); without one it is absent. The M1 mini has none: `macsmc_hwmon`
//! exposes NAND and Wi-Fi temperatures, not die sensors.
//!
//! The directories are rescanned every 30 s and the series revision bumps only when the
//! filtered set (kind and name) changed.

use std::path::{Path, PathBuf};

use kelvo_schema::{Entitlement, Labels, MetricId, Module, SeriesKey, bound_label_value};

use super::SysRoot;
use crate::{Cadence, CollectError, Collector, CollectorId, Every, Probe, SampleBuf, Tick};

const RESCAN_MS: u32 = 30_000;
const FAN_MAX_MS: u32 = 60_000;

/// The parent device's name of `/sys/class/hwmon/<dir>`. A seam so fixture trees, which
/// cannot hold symlinks, can say what each hwmon directory hangs off.
pub(super) type ParentDevice = Box<dyn Fn(&SysRoot, &str) -> Option<String> + Send>;

/// The live parent lookup: the `device` link's target, or for a virtual hwmon (a thermal
/// zone's) the directory its real path sits in.
pub(super) fn parent_device(root: &SysRoot, dir: &str) -> Option<String> {
    let hwmon = root.path(&format!("/sys/class/hwmon/{dir}"));
    if let Ok(target) = std::fs::read_link(hwmon.join("device")) {
        return basename(&target);
    }
    let real = std::fs::canonicalize(&hwmon).ok()?;
    let parent = real.parent()?;
    let parent = if parent.file_name()? == "hwmon" {
        parent.parent()?
    } else {
        parent
    };
    basename(parent)
}

fn basename(p: &Path) -> Option<String> {
    Some(p.file_name()?.to_str()?.to_owned())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Temp,
    Fan,
    Power,
}

impl Kind {
    /// The input's value in series units: °C, rpm, W.
    fn convert(self, raw: i64) -> f32 {
        match self {
            Kind::Temp => (raw as f64 / 1e3) as f32,
            Kind::Fan => raw as f32,
            Kind::Power => (raw as f64 / 1e6) as f32,
        }
    }

    fn plausible(self, v: f32) -> bool {
        v.is_finite()
            && match self {
                Kind::Temp => v > 0.0 && v <= 150.0,
                Kind::Fan => (0.0..=100_000.0).contains(&v),
                Kind::Power => (0.0..2_000.0).contains(&v),
            }
    }
}

/// hwmon chips whose temperatures are CPU or SoC die sensors.
pub(super) fn cpu_class_chip(chip: &str) -> bool {
    matches!(chip, "k10temp" | "zenpower" | "coretemp") || thermal_name(chip)
}

/// Thermal zone types that are CPU or SoC die sensors.
pub(super) fn cpu_class_zone(ty: &str) -> bool {
    ty == "x86_pkg_temp" || thermal_name(ty)
}

/// `cpu_thermal`, `cpu0-thermal`, `cpu-big0-thermal`, `soc_thermal`: device-tree CPU and
/// SoC zones, and the hwmon chips they register (hwmon turns `-` into `_`).
fn thermal_name(s: &str) -> bool {
    (s.starts_with("cpu") || s.starts_with("soc")) && s.ends_with("thermal")
}

/// One sensor a scan found.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Found {
    pub(super) kind: Kind,
    /// The label value: `name` for temperatures and power, `fan` for fans. Qualified by
    /// the chip until [`unqualify`] runs.
    pub(super) label: String,
    /// The kernel label, when it can stand alone ([`unqualify`]).
    bare: Option<String>,
    input: PathBuf,
    /// `fan<N>_max`, when it reads.
    max: Option<PathBuf>,
    /// Counts towards `thermal.hottest`.
    cpu_class: bool,
    /// The whole machine's draw: emitted as `power.system` instead of `power.sensor`.
    system: bool,
}

fn list(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    v.sort();
    v
}

/// Reads a small signed decimal attribute without allocating.
pub(super) fn read_i64_at(path: &Path) -> Option<i64> {
    use std::io::Read;
    let mut buf = [0u8; 32];
    let n = std::fs::File::open(path).ok()?.read(&mut buf).ok()?;
    std::str::from_utf8(buf.get(..n)?).ok()?.trim().parse().ok()
}

/// The input's current value in series units, if plausible.
fn read(kind: Kind, path: &Path) -> Option<f32> {
    Some(kind.convert(read_i64_at(path)?)).filter(|v| kind.plausible(*v))
}

/// `temp3_input` -> (Temp, 3, "input").
fn channel(file: &str) -> Option<(Kind, u32, &str)> {
    let (head, suffix) = file.split_once('_')?;
    let (kind, n) = [
        ("temp", Kind::Temp),
        ("fan", Kind::Fan),
        ("power", Kind::Power),
    ]
    .into_iter()
    .find_map(|(p, k)| Some((k, head.strip_prefix(p)?)))?;
    Some((kind, n.parse().ok()?, suffix))
}

/// Every plausible sensor on the host, sorted by kind and label.
pub(super) fn scan(root: &SysRoot, parent: &ParentDevice) -> Vec<Found> {
    let mut chips: Vec<(String, String)> = list(&root.path("/sys/class/hwmon"))
        .into_iter()
        .filter(|d| d.starts_with("hwmon"))
        .filter_map(|d| Some((root.read_trimmed(&format!("/sys/class/hwmon/{d}/name"))?, d)))
        .collect();
    chips.sort();
    let mut out = Vec::new();
    for (chip, dir) in &chips {
        let id = if chips.iter().filter(|(c, _)| c == chip).count() > 1 {
            // No parent (never seen) falls back to the directory: distinct, if unstable.
            let dev = parent(root, dir).unwrap_or_else(|| dir.clone());
            format!("{chip}@{dev}")
        } else {
            chip.clone()
        };
        chip_sensors(root, chip, &id, dir, &mut out);
    }
    fan_labels(&mut out);
    unqualify(&mut out);
    zones(root, &mut out);
    for f in &mut out {
        f.label = bound_label_value(&f.label).into_owned();
    }
    out.sort_by(|a, b| (a.kind, &a.label).cmp(&(b.kind, &b.label)));
    // Two channels of one chip with the same label: keep the first, one series per key.
    out.dedup_by(|b, a| a.kind == b.kind && a.label == b.label);
    out
}

fn chip_sensors(root: &SysRoot, chip: &str, id: &str, dir: &str, out: &mut Vec<Found>) {
    let base = root.path(&format!("/sys/class/hwmon/{dir}"));
    let files = list(&base);
    for file in &files {
        let Some((kind, n, suffix)) = channel(file) else {
            continue;
        };
        let has_input = || files.iter().any(|f| *f == format!("power{n}_input"));
        let wanted = suffix == "input" || (kind == Kind::Power && suffix == "average");
        if !wanted || (suffix == "average" && has_input()) {
            continue;
        }
        let input = base.join(file);
        if read(kind, &input).is_none() {
            continue;
        }
        let prefix = file.split('_').next().unwrap_or_default();
        let kernel = root.read_trimmed(&format!("/sys/class/hwmon/{dir}/{prefix}_label"));
        let label = kernel.as_deref().unwrap_or(prefix);
        let system = kind == Kind::Power && is_system_power(chip, label);
        let (label, max) = if kind == Kind::Fan {
            let max = base.join(format!("fan{n}_max"));
            let max = read(kind, &max).filter(|m| *m > 0.0).map(|_| max);
            (format!("{id}/{}", n.saturating_sub(1)), max)
        } else {
            (format!("{id}/{label}"), None)
        };
        let bare = kernel.filter(|l| kind != Kind::Fan && !generic_label(l));
        out.push(Found {
            kind,
            system,
            cpu_class: kind == Kind::Temp && cpu_class_chip(chip),
            label,
            bare,
            input,
            max,
        });
    }
}

/// Whether a power sensor is the whole machine's draw. Unverified: on the M1 mini the
/// SMC's "Total System Power" tracks the AC input (6.13 vs 6.23 W in the fixture), so it
/// is taken as the counterpart of the macOS `PSTR` key; no other chip is mapped.
fn is_system_power(chip: &str, label: &str) -> bool {
    chip == "macsmc_hwmon" && label == "Total System Power"
}

/// Labels that say which reading of a device this is but not which device: NVMe's
/// `Composite` and `Sensor <N>`, amdgpu's `edge`, `junction` and `mem`. Kept with their
/// chip.
fn generic_label(label: &str) -> bool {
    matches!(label, "Composite" | "edge" | "junction" | "mem") || label.starts_with("Sensor ")
}

/// Drops the chip from a sensor whose kernel label no other chip on the host carries for
/// the same kind. Two channels of one chip with one label share a qualified name and
/// count once; the later dedup keeps the first.
fn unqualify(out: &mut [Found]) {
    let shared = |f: &Found, b: &str| {
        out.iter()
            .any(|o| o.kind == f.kind && o.bare.as_deref() == Some(b) && o.label != f.label)
    };
    let bare: Vec<Option<String>> = out
        .iter()
        .map(|f| f.bare.clone().filter(|b| !shared(f, b)))
        .collect();
    for (f, b) in out.iter_mut().zip(bare) {
        if let Some(b) = b {
            f.label = b;
        }
    }
}

/// Fans carry `<chip>/<N-1>`; when they all share one chip, the bare `<N-1>`.
fn fan_labels(out: &mut [Found]) {
    let chip = |f: &Found| f.label.rsplit_once('/').map(|(c, _)| c.to_owned());
    let mut chips: Vec<String> = out
        .iter()
        .filter(|f| f.kind == Kind::Fan)
        .filter_map(chip)
        .collect();
    chips.dedup();
    if chips.len() != 1 {
        return;
    }
    for f in out.iter_mut().filter(|f| f.kind == Kind::Fan) {
        if let Some((_, n)) = f.label.rsplit_once('/') {
            f.label = n.to_owned();
        }
    }
}

fn zones(root: &SysRoot, out: &mut Vec<Found>) {
    let base = root.path("/sys/class/thermal");
    let mut found: Vec<(u32, String)> = list(&base)
        .into_iter()
        .filter_map(|d| {
            let n = d.strip_prefix("thermal_zone")?.parse().ok()?;
            let own_hwmon = list(&base.join(&d)).iter().any(|c| c.starts_with("hwmon"));
            let ty = root.read_trimmed(&format!("/sys/class/thermal/{d}/type"))?;
            (!own_hwmon).then_some((n, ty))
        })
        .collect();
    found.sort();
    for (n, ty) in &found {
        let input = base.join(format!("thermal_zone{n}/temp"));
        if read(Kind::Temp, &input).is_none() {
            continue;
        }
        let repeats = found.iter().filter(|(_, t)| t == ty).count() > 1;
        out.push(Found {
            kind: Kind::Temp,
            label: if repeats {
                format!("zone/{ty}@{n}")
            } else {
                format!("zone/{ty}")
            },
            input,
            max: None,
            bare: None,
            cpu_class: cpu_class_zone(ty),
            system: false,
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Sensors,
    Power,
}

impl Part {
    fn wants(self, k: Kind) -> bool {
        match self {
            Part::Sensors => k != Kind::Power,
            Part::Power => k == Kind::Power,
        }
    }
}

struct Slot {
    kind: Kind,
    input: PathBuf,
    key: SeriesKey,
    max: Option<(PathBuf, SeriesKey)>,
    cpu_class: bool,
}

/// hwmon and thermal-zone readings for one module ([`Hwmon::sensors`], [`Hwmon::power`]).
pub struct Hwmon {
    part: Part,
    root: SysRoot,
    parent: ParentDevice,
    slots: Vec<Slot>,
    /// Kind and label of each slot, what a rescan compares.
    ident: Vec<(Kind, String)>,
    hottest: Option<SeriesKey>,
    fan_max: Every,
    rescan: Every,
    /// Set by `probe`: the first rescan due after it is the probe's own scan.
    fresh: bool,
    revision: u64,
}

impl Hwmon {
    /// Temperatures and fans (Sensors).
    pub fn sensors() -> Self {
        Self::with(Part::Sensors, SysRoot::host(), Box::new(parent_device))
    }

    /// Board power sensors (Power).
    pub fn power() -> Self {
        Self::with(Part::Power, SysRoot::host(), Box::new(parent_device))
    }

    fn with(part: Part, root: SysRoot, parent: ParentDevice) -> Self {
        Self {
            part,
            root,
            parent,
            slots: Vec::new(),
            ident: Vec::new(),
            hottest: None,
            fan_max: Every::new(FAN_MAX_MS),
            rescan: Every::new(RESCAN_MS),
            fresh: true,
            revision: 0,
        }
    }

    fn found(&self) -> Vec<Found> {
        let mut v = scan(&self.root, &self.parent);
        v.retain(|f| self.part.wants(f.kind));
        v
    }

    fn set_changed(&self) -> bool {
        !self
            .found()
            .iter()
            .map(|f| (f.kind, f.label.as_str()))
            .eq(self.ident.iter().map(|(k, l)| (*k, l.as_str())))
    }
}

fn slot(f: Found) -> Slot {
    let m = MetricId::from_static;
    let (metric, label) = match f.kind {
        Kind::Temp => ("thermal.sensor", "name"),
        Kind::Fan => ("fan.rpm", "fan"),
        Kind::Power => ("power.sensor", "name"),
    };
    let labels = Labels::single(label, &f.label);
    // The system rail is `power.system` alone; as a `power.sensor` too it would list the
    // same reading twice.
    let key = if f.system {
        SeriesKey::bare(m("power.system"))
    } else {
        SeriesKey::new(m(metric), labels.clone())
    };
    Slot {
        kind: f.kind,
        max: f
            .max
            .map(|p| (p, SeriesKey::new(m("fan.max"), labels.clone()))),
        key,
        input: f.input,
        cpu_class: f.cpu_class,
    }
}

impl Collector for Hwmon {
    fn id(&self) -> CollectorId {
        match self.part {
            Part::Sensors => CollectorId("hwmon.sensors"),
            Part::Power => CollectorId("hwmon.power"),
        }
    }

    fn cadence(&self) -> Cadence {
        Cadence::Every(crate::TEMPERATURE_PERIOD_MS)
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        match self.part {
            Part::Sensors => &[Module::Sensors],
            Part::Power => &[Module::Power],
        }
    }

    fn probe(&mut self) -> Probe {
        self.fan_max.reset();
        self.rescan.reset();
        self.fresh = true;
        let found = self.found();
        self.ident = found.iter().map(|f| (f.kind, f.label.clone())).collect();
        let m = MetricId::from_static;
        self.hottest = found
            .iter()
            .any(|f| f.cpu_class)
            .then(|| SeriesKey::bare(m("thermal.hottest")));
        self.slots = found.into_iter().map(slot).collect();
        if self.slots.is_empty() {
            return Probe::NotPresent;
        }
        let mut series = Vec::new();
        for s in &self.slots {
            series.push(s.key.clone());
            series.extend(s.max.iter().map(|(_, k)| k.clone()));
        }
        series.extend(self.hottest.iter().cloned());
        Probe::Supported(series)
    }

    fn series_revision(&self) -> u64 {
        self.revision
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let max_due = self.fan_max.due(tick);
        let mut hottest: Option<f32> = None;
        for s in &self.slots {
            let Some(v) = read(s.kind, &s.input) else {
                continue;
            };
            out.push(&s.key, v);
            if s.cpu_class {
                hottest = Some(hottest.map_or(v, |h| h.max(v)));
            }
            if max_due
                && let Some((path, key)) = &s.max
                && let Some(m) = read(Kind::Fan, path).filter(|m| *m > 0.0)
            {
                out.push(key, m);
            }
        }
        if let (Some(k), Some(v)) = (&self.hottest, hottest) {
            out.push(k, v);
        }
        if self.rescan.due(tick) && !std::mem::take(&mut self.fresh) && self.set_changed() {
            self.revision += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testdata::{asahi_t0, tree, write};
    use super::*;

    /// The fixture's `readlink hwmonN/device` capture, as the parent seam.
    fn fixture_parents() -> ParentDevice {
        let text = asahi_t0().read("/hwmon_devices.txt").unwrap();
        let map: Vec<(String, String)> = text
            .lines()
            .filter_map(|l| {
                let (dir, path) = l.split_once(' ')?;
                Some((dir.to_owned(), path.rsplit('/').next()?.to_owned()))
            })
            .collect();
        Box::new(move |_, dir| map.iter().find(|(d, _)| d == dir).map(|(_, p)| p.clone()))
    }

    fn no_parents() -> ParentDevice {
        Box::new(|_, _| None)
    }

    fn tick(n: u64, secs: u64) -> Tick {
        Tick {
            n,
            wall_ms: 0,
            continuous_ns: secs * 1_000_000_000,
            interval_ms: 1_000,
        }
    }

    fn key(metric: &'static str, label: &str, value: &str) -> SeriesKey {
        SeriesKey::new(
            MetricId::from_static(metric),
            Labels::single(label, &bound_label_value(value)),
        )
    }

    fn temp(name: &str) -> SeriesKey {
        key("thermal.sensor", "name", name)
    }

    fn power(name: &str) -> SeriesKey {
        key("power.sensor", "name", name)
    }

    fn bare(metric: &'static str) -> SeriesKey {
        SeriesKey::bare(MetricId::from_static(metric))
    }

    fn sample_once(c: &mut Hwmon) -> SampleBuf {
        let mut buf = SampleBuf::new();
        c.sample(&tick(0, 0), &mut buf).unwrap();
        buf
    }

    fn sorted(v: Vec<SeriesKey>) -> Vec<String> {
        let mut s: Vec<String> = v.into_iter().map(|k| k.to_string()).collect();
        s.sort();
        s
    }

    /// linux-tailscale: three temperatures (the tas2770 reads -92.256 °C and is left
    /// out), one fan, two power sensors plus `power.system` (the third hwmon power input);
    /// no CPU-class sensor, so no `thermal.hottest`. Current, voltage and the USB-PD chips
    /// add nothing.
    #[test]
    fn asahi_fixture_values() {
        let mut s = Hwmon::with(Part::Sensors, asahi_t0(), fixture_parents());
        let Probe::Supported(series) = s.probe() else {
            panic!("sensors probe");
        };
        let nand = temp("NAND Flash Temperature");
        let wifi = temp("WiFi/BT Module Temp");
        let nvme = temp("nvme/Composite");
        let rpm = key("fan.rpm", "fan", "0");
        let max = key("fan.max", "fan", "0");
        assert_eq!(
            sorted(series),
            sorted(vec![
                nand.clone(),
                wifi.clone(),
                nvme.clone(),
                rpm.clone(),
                max.clone()
            ])
        );
        let buf = sample_once(&mut s);
        assert_eq!(buf.get(&nvme), Some(28.85));
        assert_eq!(buf.get(&nand), Some(30.321));
        assert_eq!(buf.get(&wifi), Some(32.66));
        assert_eq!(buf.get(&rpm), Some(1696.0));
        assert_eq!(buf.get(&max), Some(4499.0));
        assert_eq!(buf.get(&bare("thermal.hottest")), None);
        assert_eq!(buf.values().len(), 5);

        let mut p = Hwmon::with(Part::Power, asahi_t0(), fixture_parents());
        let Probe::Supported(series) = p.probe() else {
            panic!("power probe");
        };
        let system = bare("power.system");
        let ac = power("AC Input Power");
        let rail = power("3.8 V Rail Power");
        // The SMC's "Total System Power" is `power.system` only, not also a sensor row.
        assert_eq!(
            sorted(series),
            sorted(vec![system.clone(), ac.clone(), rail.clone()])
        );
        let buf = sample_once(&mut p);
        assert_eq!(buf.get(&system), Some(6.131752));
        assert_eq!(buf.get(&ac), Some(6.231627));
        assert_eq!(buf.get(&rail), Some(2.552584));
        assert_eq!(buf.get(&power("Total System Power")), None);
        assert_eq!(buf.values().len(), 3, "curr1 is not power");
    }

    /// The names linux-tailscale shows: kernel labels as they read, the NVMe's generic
    /// `Composite` with its chip.
    #[test]
    fn fixture_names_are_kernel_labels() {
        let found = scan(&asahi_t0(), &fixture_parents());
        let labels: Vec<(Kind, &str)> = found.iter().map(|f| (f.kind, f.label.as_str())).collect();
        assert_eq!(
            labels,
            [
                (Kind::Temp, "NAND Flash Temperature"),
                (Kind::Temp, "WiFi/BT Module Temp"),
                (Kind::Temp, "nvme/Composite"),
                (Kind::Fan, "0"),
                (Kind::Power, "3.8 V Rail Power"),
                (Kind::Power, "AC Input Power"),
                (Kind::Power, "Total System Power"),
            ]
        );
    }

    /// A label is qualified by its chip when it is generic, when another chip carries
    /// it, or when it is missing; one chip repeating its own label stays bare. A label
    /// over the cap is still bounded.
    #[test]
    fn labels_qualified_only_when_ambiguous() {
        let dir = tree(&[]);
        let long = "Southbridge Platform Controller Hub Temperature";
        chip(
            &dir,
            "hwmon0",
            "nct6775",
            &[
                ("temp1_input", "40000"),
                ("temp1_label", "CPUTIN"),
                ("temp2_input", "41000"),
                ("temp2_label", "SYSTIN"),
                ("temp3_input", "42000"),
                ("temp3_label", "AUXTIN"),
                ("temp4_input", "43000"),
                ("temp4_label", "AUXTIN"),
                ("temp5_input", "44000"),
                ("temp5_label", long),
                ("power1_input", "5000000"),
                ("power1_label", "SYSTIN"),
            ],
        );
        chip(
            &dir,
            "hwmon1",
            "it8792",
            &[
                ("temp1_input", "50000"),
                ("temp1_label", "SYSTIN"),
                ("temp2_input", "51000"),
            ],
        );
        chip(
            &dir,
            "hwmon2",
            "amdgpu",
            &[("temp1_input", "60000"), ("temp1_label", "edge")],
        );
        chip(
            &dir,
            "hwmon3",
            "nvme",
            &[("temp2_input", "45000"), ("temp2_label", "Sensor 1")],
        );
        let found = scan(&SysRoot::at(dir.path()), &no_parents());
        let labels: Vec<(Kind, &str)> = found.iter().map(|f| (f.kind, f.label.as_str())).collect();
        let bounded = bound_label_value(long);
        assert!(bounded.len() <= kelvo_schema::LABEL_VALUE_MAX);
        let mut want = vec![
            (Kind::Temp, "AUXTIN"),
            (Kind::Temp, "CPUTIN"),
            (Kind::Temp, &*bounded),
            (Kind::Temp, "amdgpu/edge"),
            (Kind::Temp, "it8792/SYSTIN"),
            (Kind::Temp, "it8792/temp2"),
            (Kind::Temp, "nct6775/SYSTIN"),
            (Kind::Temp, "nvme/Sensor 1"),
            // Another kind's label does not count as a repeat.
            (Kind::Power, "SYSTIN"),
        ];
        want.sort();
        assert_eq!(labels, want);
    }

    fn chip(dir: &tempfile::TempDir, hwmon: &str, name: &str, files: &[(&str, &str)]) {
        write(dir, &format!("sys/class/hwmon/{hwmon}/name"), name);
        for (f, v) in files {
            write(dir, &format!("sys/class/hwmon/{hwmon}/{f}"), v);
        }
    }

    /// Two NVMe drives share the chip name: each is qualified by its parent device, and
    /// swapping the `hwmonN` numbers (a different probe order on the next boot) keeps
    /// every name on the same drive.
    #[test]
    fn repeated_chips_are_qualified_by_parent_not_order() {
        let names = |swap: bool| {
            let dir = tree(&[]);
            let (a, b) = if swap {
                ("hwmon1", "hwmon0")
            } else {
                ("hwmon0", "hwmon1")
            };
            let composite = |t| [("temp1_input", t), ("temp1_label", "Composite")];
            chip(&dir, a, "nvme", &composite("40000"));
            chip(&dir, b, "nvme", &composite("50000"));
            chip(&dir, "hwmon2", "k10temp", &[("temp1_input", "60000")]);
            let parents: ParentDevice = Box::new(move |_, d| {
                let dev = match d {
                    _ if d == a => "nvme0",
                    _ if d == b => "nvme1",
                    _ => "0000:00:18.3",
                };
                Some(dev.into())
            });
            let mut c = Hwmon::with(Part::Sensors, SysRoot::at(dir.path()), parents);
            c.probe();
            let buf = sample_once(&mut c);
            (
                buf.get(&temp("nvme@nvme0/Composite")),
                buf.get(&temp("nvme@nvme1/Composite")),
                buf.get(&temp("k10temp/temp1")),
            )
        };
        let want = (Some(40.0), Some(50.0), Some(60.0));
        assert_eq!(names(false), want);
        assert_eq!(names(true), want);
    }

    /// `thermal.hottest` is the max of the CPU-class sensors only: the NVMe drive is
    /// hotter here and must not win.
    #[test]
    fn hottest_comes_from_cpu_class_sensors_only() {
        let dir = tree(&[]);
        chip(
            &dir,
            "hwmon0",
            "k10temp",
            &[
                ("temp1_input", "55500"),
                ("temp1_label", "Tctl"),
                ("temp3_input", "51000"),
                ("temp3_label", "Tccd1"),
            ],
        );
        chip(&dir, "hwmon1", "nvme", &[("temp1_input", "70000")]);
        let mut c = Hwmon::with(Part::Sensors, SysRoot::at(dir.path()), no_parents());
        let Probe::Supported(series) = c.probe() else {
            panic!("probe");
        };
        assert!(series.contains(&bare("thermal.hottest")));
        let buf = sample_once(&mut c);
        assert_eq!(buf.get(&bare("thermal.hottest")), Some(55.5));
        assert_eq!(buf.get(&temp("Tctl")), Some(55.5));
        assert_eq!(buf.get(&temp("nvme/temp1")), Some(70.0));

        assert!(cpu_class_chip("coretemp") && cpu_class_chip("cpu_thermal"));
        assert!(cpu_class_chip("soc_thermal") && cpu_class_zone("cpu-big0-thermal"));
        assert!(cpu_class_zone("x86_pkg_temp"));
        for not in [
            "nvme",
            "macsmc_hwmon",
            "tas2770",
            "acpitz",
            "amdgpu",
            "iwlwifi_1",
        ] {
            assert!(!cpu_class_chip(not) && !cpu_class_zone(not), "{not}");
        }
    }

    /// Bad reads never become values: 0 °C, -92 °C and 151 °C are dropped at probe; a
    /// sensor that later reads 0 is a gap, not a zero. Negative fan and power readings,
    /// absurd power, voltage and current never appear.
    #[test]
    fn implausible_values_are_dropped() {
        let dir = tree(&[]);
        chip(
            &dir,
            "hwmon0",
            "board",
            &[
                ("temp1_input", "0"),
                ("temp2_input", "-92256"),
                ("temp3_input", "151000"),
                ("temp4_input", "150000"),
                ("temp5_input", "45000"),
                ("fan1_input", "-5"),
                ("power1_input", "-1"),
                ("power2_input", "2000000000"),
                ("in0_input", "12000"),
                ("curr1_input", "511"),
            ],
        );
        let root = SysRoot::at(dir.path());
        let found = scan(&root, &no_parents());
        let labels: Vec<&str> = found.iter().map(|f| f.label.as_str()).collect();
        assert_eq!(labels, ["board/temp4", "board/temp5"]);
        let mut c = Hwmon::with(Part::Sensors, root, no_parents());
        c.probe();
        write(&dir, "sys/class/hwmon/hwmon0/temp5_input", "0");
        let buf = sample_once(&mut c);
        assert_eq!(buf.get(&temp("board/temp5")), None);
        assert_eq!(buf.get(&temp("board/temp4")), Some(150.0));
        let mut p = Hwmon::with(Part::Power, SysRoot::at(dir.path()), no_parents());
        assert_eq!(p.probe(), Probe::NotPresent, "no plausible power reading");
    }

    #[test]
    fn power_average_only_without_input() {
        let dir = tree(&[]);
        chip(
            &dir,
            "hwmon0",
            "amdgpu",
            &[
                ("power1_average", "35000000"),
                ("power2_input", "1000000"),
                ("power2_average", "9000000"),
            ],
        );
        let found = scan(&SysRoot::at(dir.path()), &no_parents());
        let got: Vec<(&str, f32)> = found
            .iter()
            .map(|f| (f.label.as_str(), read(f.kind, &f.input).unwrap()))
            .collect();
        assert_eq!(got, [("amdgpu/power1", 35.0), ("amdgpu/power2", 1.0)]);
    }

    /// Fans on two chips keep their chip; zones read only when they have no hwmon of
    /// their own, and a repeated type is numbered.
    #[test]
    fn fan_labels_and_zones() {
        let dir = tree(&[]);
        let nct = [("fan1_input", "900"), ("fan2_input", "0")];
        chip(&dir, "hwmon0", "nct6775", &nct);
        let gpu = [("fan1_input", "1200"), ("fan1_max", "3300")];
        chip(&dir, "hwmon1", "amdgpu", &gpu);
        let zones = [
            (0, "acpitz", "27800"),
            (1, "acpitz", "29800"),
            (2, "x86_pkg_temp", "48000"),
            (3, "iwlwifi_1", "41000"),
        ];
        for (n, ty, t) in zones {
            write(&dir, &format!("sys/class/thermal/thermal_zone{n}/type"), ty);
            write(&dir, &format!("sys/class/thermal/thermal_zone{n}/temp"), t);
        }
        write(
            &dir,
            "sys/class/thermal/thermal_zone3/hwmon5/name",
            "iwlwifi_1",
        );
        let mut c = Hwmon::with(Part::Sensors, SysRoot::at(dir.path()), no_parents());
        let Probe::Supported(series) = c.probe() else {
            panic!("probe");
        };
        assert_eq!(
            sorted(series),
            sorted(vec![
                key("fan.rpm", "fan", "amdgpu/0"),
                key("fan.max", "fan", "amdgpu/0"),
                key("fan.rpm", "fan", "nct6775/0"),
                key("fan.rpm", "fan", "nct6775/1"),
                temp("zone/acpitz@0"),
                temp("zone/acpitz@1"),
                temp("zone/x86_pkg_temp"),
                bare("thermal.hottest"),
            ])
        );
        let buf = sample_once(&mut c);
        let stopped = buf.get(&key("fan.rpm", "fan", "nct6775/1"));
        assert_eq!(stopped, Some(0.0), "a stopped fan is a reading");
        assert_eq!(buf.get(&bare("thermal.hottest")), Some(48.0));
        assert_eq!(buf.get(&temp("zone/acpitz@1")), Some(29.8));
    }

    #[test]
    fn rescan_bumps_revision_only_for_a_new_sensor() {
        let dir = tree(&[]);
        chip(&dir, "hwmon0", "nvme", &[("temp1_input", "40000")]);
        let mut c = Hwmon::with(Part::Sensors, SysRoot::at(dir.path()), no_parents());
        c.probe();
        let mut buf = SampleBuf::new();
        c.sample(&tick(0, 0), &mut buf).unwrap();
        // A sensor-less chip, an implausible one and a changed value change nothing.
        chip(&dir, "hwmon1", "tps6598x_source_psy_0_0038", &[]);
        chip(&dir, "hwmon2", "tas2770", &[("temp1_input", "-92256")]);
        write(&dir, "sys/class/hwmon/hwmon0/temp1_input", "41000");
        c.sample(&tick(1, 30), &mut buf).unwrap();
        assert_eq!(c.series_revision(), 0);
        chip(&dir, "hwmon3", "k10temp", &[("temp1_input", "50000")]);
        c.sample(&tick(2, 40), &mut buf).unwrap();
        assert_eq!(c.series_revision(), 0, "rescans every 30 s");
        c.sample(&tick(3, 60), &mut buf).unwrap();
        assert_eq!(c.series_revision(), 1);
        assert!(matches!(c.probe(), Probe::Supported(s) if s.len() == 3));
    }

    #[test]
    fn no_hwmon_is_not_present() {
        let dir = tree(&[]);
        for part in [Part::Sensors, Part::Power] {
            let mut c = Hwmon::with(part, SysRoot::at(dir.path()), no_parents());
            assert_eq!(c.probe(), Probe::NotPresent);
        }
    }

    /// The live parent lookup follows the `device` link, and for a thermal zone's
    /// virtual hwmon the directory it sits in.
    #[test]
    fn live_parent_device_from_symlinks() {
        use std::os::unix::fs::symlink;
        let dir = tree(&[]);
        let root = SysRoot::at(dir.path());
        let dev = dir
            .path()
            .join("sys/devices/platform/soc/27bcc0000.nvme/nvme/nvme0");
        std::fs::create_dir_all(dev.join("hwmon/hwmon0")).unwrap();
        let zone = dir
            .path()
            .join("sys/devices/virtual/thermal/thermal_zone0/hwmon1");
        std::fs::create_dir_all(&zone).unwrap();
        std::fs::create_dir_all(root.path("/sys/class/hwmon")).unwrap();
        symlink(
            dev.join("hwmon/hwmon0"),
            root.path("/sys/class/hwmon/hwmon0"),
        )
        .unwrap();
        symlink(&dev, dev.join("hwmon/hwmon0/device")).unwrap();
        symlink(&zone, root.path("/sys/class/hwmon/hwmon1")).unwrap();
        assert_eq!(parent_device(&root, "hwmon0").as_deref(), Some("nvme0"));
        assert_eq!(
            parent_device(&root, "hwmon1").as_deref(),
            Some("thermal_zone0")
        );
        assert_eq!(parent_device(&root, "hwmon9"), None);
    }
}

//! Disk I/O per physical device and capacity per mounted volume.
//!
//! I/O: every `IOBlockStorageDriver` whose child `IOMedia` has a BSD name (`disk0`, plus
//! attached externals and disk images, the same set `iostat` shows). Its `Statistics`
//! dictionary holds cumulative `Bytes (Read)` and `Bytes (Write)`; the collector turns
//! them into `disk.read{dev}` / `disk.write{dev}` rates. A decrease is a reset (device
//! re-attached) and yields no rate that tick. `disk.read_total` and `disk.write_total`
//! sum the devices' rates of the same sample, a gap when any device's own value is a gap
//! (D-092), each side on its own.
//!
//! Capacity: `getmntinfo` at most every 60 s, for local volumes the Finder would show
//! (`MNT_LOCAL` and not `MNT_DONTBROWSE`), labelled `vol` with the mount point. That
//! hides the boot container's helper volumes (VM, Preboot, Data and so on). On APFS every
//! volume in a container reports the container's size and free space, so
//! `disk.used = total - free` is the container's used space, which is what Finder shows
//! for "Macintosh HD". `free` is `f_bavail` (space available to a normal user).
//! [`DiskCapacity::volumes`] identifies the boot volume and its APFS container
//! (`disk3` for `/dev/disk3s1s1`).

use kelvo_schema::{Entitlement, Labels, MetricId, Module, SeriesKey};

use super::PartSum;
use super::iokit::{self, IoObject, Key, matching_services};
use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

const IO_METRICS: [&str; 8] = [
    "disk.read",
    "disk.write",
    "disk.read_ops",
    "disk.write_ops",
    "disk.read_timed_ops",
    "disk.write_timed_ops",
    "disk.read_time",
    "disk.write_time",
];
const IO_COUNTERS: [&str; 6] = [
    "Bytes (Read)",
    "Bytes (Write)",
    "Operations (Read)",
    "Operations (Write)",
    "Total Time (Read)",
    "Total Time (Write)",
];
type Counters = [Option<u64>; 6];
struct Device {
    driver: IoObject,
    keys: [SeriesKey; 8],
    prev: Counters,
}

/// Independent byte/operation rates and paired completed-request timing rates.
pub struct DiskIo {
    devices: Vec<Device>,
    prev_ns: u64,
    totals: [SeriesKey; 8],
    k_stats: Key,
    counters: [Key; 6],
    k_bsd: Key,
}

impl Default for DiskIo {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskIo {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            prev_ns: 0,
            totals: kelvo_schema::disk_io::TOTAL_METRICS
                .map(|id| SeriesKey::bare(MetricId::from_static(id))),
            k_stats: Key::new("Statistics"),
            counters: IO_COUNTERS.map(Key::new),
            k_bsd: Key::new("BSD Name"),
        }
    }

    /// The device names found by the last probe.
    pub fn devices(&self) -> impl Iterator<Item = &str> {
        self.devices
            .iter()
            .filter_map(|d| d.keys.first()?.labels.get("dev"))
    }

    fn read_counters(&self, driver: &IoObject) -> Counters {
        let Some(stats) = driver
            .property(&self.k_stats)
            .and_then(|v| iokit::dict_of(&v))
        else {
            return [None; 6];
        };
        self.counters.each_ref().map(|key| {
            iokit::get(&stats, key)
                .and_then(|v| iokit::as_i64(&v))
                .and_then(|v| u64::try_from(v).ok())
        })
    }
}

/// Bytes per second between two cumulative readings; `None` on a reset.
fn rate(prev: u64, cur: u64, secs: f64) -> Option<f32> {
    (cur >= prev && secs > 0.0).then(|| ((cur - prev) as f64 / secs) as f32)
}

/// One device's read and write rates from its previous and current `(read, write)` byte
/// counters; each side `None` without both readings or on a reset.
#[cfg(test)]
fn device_rates(prev: Option<(u64, u64)>, cur: Option<(u64, u64)>, secs: f64) -> [Option<f32>; 2] {
    match (prev, cur) {
        (Some((pr, pw)), Some((r, w))) => [rate(pr, r, secs), rate(pw, w, secs)],
        _ => [None, None],
    }
}

/// Read/write pairing is independent. An unsupported timing counter never masks
/// known operations/s, and an invalid timing pair never fabricates request time.
fn counter_rates(prev: Counters, cur: Counters, secs: f64) -> [Option<f32>; 8] {
    let values: [Option<f32>; 6] = std::array::from_fn(|i| rate(prev[i]?, cur[i]?, secs));
    let mut out = [
        values[0], values[1], values[2], values[3], None, None, None, None,
    ];
    for side in 0..2 {
        if let (Some(ops), Some(ns)) = (values[2 + side], values[4 + side])
            && (ops > 0. || ns == 0.)
        {
            out[4 + side] = Some(ops);
            out[6 + side] = Some(ns / 1_000_000.);
        }
    }
    out
}

fn fixed_internal(
    internal: Option<bool>,
    removable: Option<bool>,
    ejectable: Option<bool>,
) -> bool {
    internal == Some(true) && removable == Some(false) && ejectable == Some(false)
}

/// On-demand metadata only; the sampler retains no device names beyond its labels.
pub fn io_devices() -> kelvo_schema::disk_io::DiskIoDevices {
    use kelvo_schema::disk_io::{DiskIoDevice, DiskIoDevices};
    let bsd = Key::new("BSD Name");
    let protocol = Key::new("Protocol Characteristics");
    let location = Key::new("Physical Interconnect Location");
    let stats = Key::new("Statistics");
    let keys = IO_COUNTERS.map(Key::new);
    let mut devices = Vec::new();
    let mut internal_fixed = None;
    let removable = Key::new("Removable");
    let ejectable = Key::new("Ejectable");
    for driver in matching_services(c"IOBlockStorageDriver")
        .into_iter()
        .take(128)
    {
        let Some(media) = driver.first_child() else {
            continue;
        };
        let Some(dev) = media.property(&bsd).and_then(|v| iokit::as_string(&v)) else {
            continue;
        };
        let name = media.name().unwrap_or_else(|| dev.clone());
        let internal = driver
            .ancestor_property(&protocol)
            .and_then(|v| iokit::dict_of(&v))
            .and_then(|d| iokit::get(&d, &location))
            .and_then(|v| iokit::as_string(&v))
            .and_then(|v| match v.as_str() {
                "Internal" => Some(true),
                "External" => Some(false),
                _ => None,
            });
        let request_time_supported = driver
            .property(&stats)
            .and_then(|v| iokit::dict_of(&v))
            .is_some_and(|d| {
                keys[2..].iter().all(|k| {
                    iokit::get(&d, k)
                        .and_then(|v| iokit::as_i64(&v))
                        .is_some_and(|v| v >= 0)
                })
            });
        if fixed_internal(
            internal,
            media.property(&removable).and_then(|v| iokit::as_bool(&v)),
            media.property(&ejectable).and_then(|v| iokit::as_bool(&v)),
        ) {
            internal_fixed.get_or_insert_with(|| dev.clone());
        }
        devices.push(DiskIoDevice {
            dev,
            name,
            internal,
            request_time_supported,
        });
    }
    devices.sort_by(|a, b| a.dev.cmp(&b.dev));
    let default_device = internal_fixed.or_else(|| devices.first().map(|d| d.dev.clone()));
    DiskIoDevices {
        devices,
        default_device,
    }
}

impl Collector for DiskIo {
    fn id(&self) -> CollectorId {
        CollectorId("disk_io")
    }

    fn cadence(&self) -> Cadence {
        // D-067: every tick while a window or the menu bar shows it, else every 10 s.
        crate::LIVE_OR_IDLE
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Disk]
    }

    fn probe(&mut self) -> Probe {
        self.devices.clear();
        self.prev_ns = 0;
        let mut series = Vec::new();
        for driver in matching_services(c"IOBlockStorageDriver") {
            let Some(name) = driver
                .first_child()
                .and_then(|media| media.property(&self.k_bsd))
                .and_then(|v| iokit::as_string(&v))
            else {
                continue; // no media (empty card reader)
            };
            let key = |id| SeriesKey::new(MetricId::from_static(id), Labels::single("dev", &name));
            let dev = Device {
                driver,
                keys: IO_METRICS.map(key),
                prev: [None; 6],
            };
            series.extend(dev.keys.iter().cloned());
            self.devices.push(dev);
        }
        if self.devices.is_empty() {
            Probe::NotPresent
        } else {
            series.extend(self.totals.iter().cloned());
            Probe::Supported(series)
        }
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let now = tick.continuous_ns;
        let secs = if self.prev_ns > 0 && now > self.prev_ns {
            (now - self.prev_ns) as f64 / 1e9
        } else {
            0.0
        };
        let mut totals: [PartSum; 8] = std::array::from_fn(|_| PartSum::default());
        for i in 0..self.devices.len() {
            let Some(dev) = self.devices.get(i) else {
                continue;
            };
            let cur = self.read_counters(&dev.driver);
            let Some(dev) = self.devices.get_mut(i) else {
                continue;
            };
            let values = counter_rates(dev.prev, cur, secs);
            for (key, value) in dev.keys.iter().zip(values) {
                if let Some(value) = value {
                    out.push(key, value);
                }
            }
            for (total, value) in totals.iter_mut().zip(values) {
                total.add(value);
            }
            dev.prev = cur;
        }
        if !self.devices.is_empty() {
            for (key, total) in self.totals.iter().zip(totals) {
                if let Some(value) = total.total() {
                    out.push(key, value);
                }
            }
        }
        self.prev_ns = now;
        Ok(())
    }
}

/// A volume the capacity collector reports.
#[derive(Clone, Debug, PartialEq)]
pub struct VolumeInfo {
    /// Mount point, the `vol` label value.
    pub mount: String,
    /// BSD device, for example `disk3s1s1`.
    pub device: String,
    /// The APFS container (whole disk) the volume lives on, for example `disk3`.
    pub container: Option<String>,
    pub fs_type: String,
    pub is_boot: bool,
}

/// `disk3s1s1` -> `disk3`. `None` when the name is not a `diskN...` device.
fn whole_disk(dev: &str) -> Option<String> {
    let rest = dev.strip_prefix("disk")?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    (digits > 0).then(|| format!("disk{}", rest.get(..digits).unwrap_or_default()))
}

fn c_str(buf: &[libc::c_char]) -> std::borrow::Cow<'_, str> {
    // SAFETY: c_char is one byte and every bit pattern is a valid u8. The
    // borrowed byte slice has the same lifetime and bounds as the input.
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), buf.len()) };
    let bytes = bytes.split(|byte| *byte == 0).next().unwrap_or_default();
    String::from_utf8_lossy(bytes)
}

unsafe extern "C" {
    /// `getmntinfo` with a buffer of the caller's own, freed with `free` (macOS 10.13+).
    /// Plain `getmntinfo` reuses one process-wide buffer, which races when the shell reads
    /// `boot_mounts` while the engine's capacity collector samples. The symbol carries the
    /// `$INODE64` suffix on Intel, as `libc` declares `getmntinfo`.
    #[cfg_attr(target_arch = "x86_64", link_name = "getmntinfo_r_np$INODE64")]
    fn getmntinfo_r_np(mntbufp: *mut *mut libc::statfs, flags: libc::c_int) -> libc::c_int;
}

/// Owns the reentrant mount buffer for exactly the duration of a read.
struct MountBuffer(*mut libc::statfs);
impl Drop for MountBuffer {
    fn drop(&mut self) {
        // SAFETY: getmntinfo_r_np returns a malloc-owned buffer; free accepts null.
        unsafe { libc::free(self.0.cast()) };
    }
}
fn with_mounts<T>(read: impl FnOnce(&[libc::statfs]) -> T) -> T {
    let mut mounts = MountBuffer(std::ptr::null_mut());
    // SAFETY: mounts.0 is an out-pointer owned by the guard above.
    let n = unsafe { getmntinfo_r_np(&mut mounts.0, libc::MNT_NOWAIT) };
    if n <= 0 || mounts.0.is_null() {
        return read(&[]);
    }
    // SAFETY: the call returned n contiguous entries; the guard outlives read.
    let all = unsafe { std::slice::from_raw_parts(mounts.0, n as usize) };
    read(all)
}

/// Mounted local, browsable volumes. Metadata is owned only when probing.
fn mounted() -> Vec<(VolumeInfo, u64, u64)> {
    with_mounts(volumes_of)
}
fn browsable(fs: &libc::statfs) -> bool {
    fs.f_flags & libc::MNT_LOCAL as u32 != 0 && fs.f_flags & libc::MNT_DONTBROWSE as u32 == 0
}

fn volumes_of(all: &[libc::statfs]) -> Vec<(VolumeInfo, u64, u64)> {
    let mut out = Vec::new();
    for fs in all {
        if !browsable(fs) {
            continue;
        }
        let mount = c_str(&fs.f_mntonname).into_owned();
        let from = c_str(&fs.f_mntfromname);
        let device = from.strip_prefix("/dev/").unwrap_or(&from).to_owned();
        let bsize = u64::from(fs.f_bsize);
        let total = fs.f_blocks.saturating_mul(bsize);
        let free = fs.f_bavail.saturating_mul(bsize);
        out.push((
            VolumeInfo {
                is_boot: mount == "/",
                container: whole_disk(&device),
                device,
                fs_type: c_str(&fs.f_fstypename).into_owned(),
                mount,
            },
            total,
            free,
        ));
    }
    out
}

/// The mounts of the boot volume's APFS container among the reported volumes, `/` first
/// and then by mount point (`/`, `/System/Volumes/Data`), for `HostInfo::boot_mounts`
/// (D-092). Empty when no reported volume is the boot volume.
pub fn boot_mounts() -> Vec<String> {
    let vols: Vec<VolumeInfo> = mounted()
        .into_iter()
        .filter(|(_, total, _)| *total > 0)
        .map(|(v, _, _)| v)
        .collect();
    boot_mounts_of(&vols)
}

fn boot_mounts_of(vols: &[VolumeInfo]) -> Vec<String> {
    let Some(boot) = vols.iter().find(|v| v.is_boot) else {
        return Vec::new();
    };
    let mut out: Vec<&VolumeInfo> = vols
        .iter()
        .filter(|v| v.is_boot || (boot.container.is_some() && v.container == boot.container))
        .collect();
    out.sort_by(|a, b| b.is_boot.cmp(&a.is_boot).then(a.mount.cmp(&b.mount)));
    out.into_iter().map(|v| v.mount.clone()).collect()
}

struct Volume {
    info: VolumeInfo,
    used: SeriesKey,
    free: SeriesKey,
    total: SeriesKey,
}

/// `disk.used`, `disk.free`, `disk.total` per volume, at most every 60 s.
#[derive(Default)]
pub struct DiskCapacity {
    volumes: Vec<Volume>,
    infos: Vec<VolumeInfo>,
}

impl DiskCapacity {
    pub fn new() -> Self {
        Self::default()
    }

    /// The volumes found by the last probe; the boot volume has `is_boot`.
    pub fn volumes(&self) -> &[VolumeInfo] {
        &self.infos
    }

    fn sample_mounts(&self, mounts: &[libc::statfs], out: &mut SampleBuf) {
        for fs in mounts.iter().filter(|fs| browsable(fs)) {
            let mount = c_str(&fs.f_mntonname);
            let Some(v) = self.volumes.iter().find(|v| v.info.mount == mount) else {
                continue; // New mounts get series only when the collector is reprobed.
            };
            let bsize = u64::from(fs.f_bsize);
            let total = fs.f_blocks.saturating_mul(bsize);
            let free = fs.f_bavail.saturating_mul(bsize);
            out.push(&v.total, total as f32);
            out.push(&v.free, free as f32);
            out.push(&v.used, total.saturating_sub(free) as f32);
        }
    }
}

impl Collector for DiskCapacity {
    fn id(&self) -> CollectorId {
        CollectorId("disk_capacity")
    }

    fn cadence(&self) -> Cadence {
        Cadence::Every(60_000)
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Disk]
    }

    fn probe(&mut self) -> Probe {
        self.volumes.clear();
        let mut series = Vec::new();
        for (info, total, _) in mounted() {
            if total == 0 {
                continue;
            }
            let key = |id| {
                SeriesKey::new(
                    MetricId::from_static(id),
                    Labels::single("vol", &info.mount),
                )
            };
            let v = Volume {
                used: key("disk.used"),
                free: key("disk.free"),
                total: key("disk.total"),
                info,
            };
            series.extend([v.used.clone(), v.free.clone(), v.total.clone()]);
            self.volumes.push(v);
        }
        self.infos = self.volumes.iter().map(|v| v.info.clone()).collect();
        Probe::Supported(series)
    }

    fn sample(&mut self, _tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        // Capacity changes; mount/device/type metadata and series keys do not.
        // Borrow the mount table instead of allocating VolumeInfo per volume.
        with_mounts(|mounts| self.sample_mounts(mounts, out));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_prefers_fixed_internal_storage_over_a_builtin_card_reader() {
        assert!(fixed_internal(Some(true), Some(false), Some(false)));
        assert!(!fixed_internal(Some(true), Some(true), Some(true)));
        assert!(!fixed_internal(Some(false), Some(false), Some(false)));
        assert!(!fixed_internal(None, Some(false), Some(false)));
        assert!(!fixed_internal(Some(true), None, Some(false)));
    }

    #[test]
    fn borrowed_capacity_samples_keep_probed_series_and_mount_filtering() {
        fn mount(path: &str, flags: u32, available: u64) -> libc::statfs {
            // SAFETY: statfs contains only numeric fields and fixed numeric arrays.
            let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
            for (slot, byte) in fs.f_mntonname.iter_mut().zip(path.bytes()) {
                *slot = byte as libc::c_char;
            }
            fs.f_flags = flags;
            fs.f_bsize = 4096;
            fs.f_blocks = 100;
            fs.f_bavail = available;
            fs
        }
        let local = libc::MNT_LOCAL as u32;
        let initial = [mount("/", local, 40), mount("/Volumes/Removed", local, 20)];
        let volumes = volumes_of(&initial)
            .into_iter()
            .map(|(info, _, _)| {
                let key = |id| {
                    SeriesKey::new(
                        MetricId::from_static(id),
                        Labels::single("vol", &info.mount),
                    )
                };
                Volume {
                    used: key("disk.used"),
                    free: key("disk.free"),
                    total: key("disk.total"),
                    info,
                }
            })
            .collect();
        let capacity = DiskCapacity {
            volumes,
            infos: Vec::new(),
        };
        let changed = [
            mount("/", local, 30),
            mount("/Volumes/New", local, 10),
            mount("/", local | libc::MNT_DONTBROWSE as u32, 99),
            mount("/", 0, 99),
        ];
        let mut out = SampleBuf::new();
        capacity.sample_mounts(&changed, &mut out);
        let measured: Vec<_> = out
            .values()
            .iter()
            .map(|sample| (sample.key.to_string(), sample.value))
            .collect();
        assert_eq!(
            measured,
            [
                ("disk.total{vol=/}".into(), 409_600.),
                ("disk.free{vol=/}".into(), 122_880.),
                ("disk.used{vol=/}".into(), 286_720.),
            ]
        );
        out.clear();
        capacity.sample_mounts(&[], &mut out);
        assert!(out.values().is_empty());
        assert!(matches!(
            c_str(&initial[0].f_mntonname),
            std::borrow::Cow::Borrowed("/")
        ));
    }

    #[test]
    fn whole_disk_strips_slices() {
        assert_eq!(whole_disk("disk3s1s1").as_deref(), Some("disk3"));
        assert_eq!(whole_disk("disk12s2").as_deref(), Some("disk12"));
        assert_eq!(whole_disk("disk0").as_deref(), Some("disk0"));
        assert_eq!(whole_disk("map auto_home"), None);
        assert_eq!(whole_disk("disks"), None);
    }

    #[test]
    fn request_pairs_handle_idle_unsupported_reset_and_one_sided_failure() {
        let baseline = [Some(100); 6];
        let current = [
            Some(300),
            Some(200),
            Some(110),
            Some(100),
            Some(10_000_100),
            Some(100),
        ];
        let rates = counter_rates(baseline, current, 2.);
        assert_eq!(
            rates,
            [
                Some(100.),
                Some(50.),
                Some(5.),
                Some(0.),
                Some(5.),
                Some(0.),
                Some(5.),
                Some(0.)
            ]
        );
        assert_eq!(
            kelvo_schema::disk_io::request_ms(rates[6].map(f64::from), rates[4].map(f64::from)),
            Some(1.)
        );
        assert_eq!(
            kelvo_schema::disk_io::request_ms(rates[7].map(f64::from), rates[5].map(f64::from)),
            None
        );
        let mut missing = current;
        missing[4] = None;
        let missing_rates = counter_rates(baseline, missing, 2.);
        assert_eq!(
            (missing_rates[2], missing_rates[4], missing_rates[6]),
            (Some(5.), None, None)
        );
        assert_eq!(missing_rates[5], Some(0.));
        let mut reset = current;
        reset[4] = Some(1);
        assert_eq!(counter_rates(baseline, reset, 2.)[4], None);
        reset[2] = Some(1);
        reset[4] = current[4];
        assert_eq!(counter_rates(baseline, reset, 2.)[6], None);
        assert_eq!(counter_rates([None; 6], current, 2.), [None; 8]);
        assert_eq!(counter_rates(baseline, current, 0.), [None; 8]);
        assert_eq!(counter_rates(current, current, 1.)[4..], [Some(0.); 4]);
    }
    #[test]
    fn reset_yields_no_rate() {
        assert_eq!(rate(100, 300, 2.0), Some(100.0));
        assert_eq!(rate(300, 100, 1.0), None);
        assert_eq!(rate(100, 300, 0.0), None);
    }

    #[test]
    fn boot_mounts_are_the_boot_container_root_first() {
        let vol = |mount: &str, device: &str, is_boot| VolumeInfo {
            mount: mount.into(),
            device: device.into(),
            container: whole_disk(device),
            fs_type: "apfs".into(),
            is_boot,
        };
        let vols = [
            vol("/Volumes/Backup", "disk5s1", false),
            vol("/System/Volumes/Data", "disk3s5", false),
            vol("/", "disk3s1s1", true),
        ];
        assert_eq!(boot_mounts_of(&vols), ["/", "/System/Volumes/Data"]);
        assert_eq!(boot_mounts_of(&vols[..1]), Vec::<String>::new());
    }

    #[test]
    fn device_rates_gate_each_side() {
        assert_eq!(
            device_rates(Some((100, 50)), Some((300, 50)), 2.0),
            [Some(100.0), Some(0.0)]
        );
        assert_eq!(device_rates(None, Some((300, 50)), 2.0), [None, None]);
        assert_eq!(device_rates(Some((100, 50)), None, 2.0), [None, None]);
        assert_eq!(
            device_rates(Some((300, 50)), Some((100, 60)), 1.0),
            [None, Some(10.0)],
            "a read reset leaves the write rate"
        );
    }

    #[test]
    fn totals_follow_each_sides_parts() {
        // Two devices: disk0 measured on both sides, disk4 with a read reset.
        let mut read = PartSum::default();
        let mut write = PartSum::default();
        for [r, w] in [
            device_rates(Some((0, 0)), Some((400, 100)), 2.0),
            device_rates(Some((900, 0)), Some((10, 50)), 2.0),
        ] {
            read.add(r);
            write.add(w);
        }
        assert_eq!((read.total(), write.total()), (None, Some(75.0)));
    }

    #[test]
    #[ignore = "reads live IOKit disk statistics and mounts; run by hand on a Mac"]
    fn live_smoke() {
        eprintln!("disk metadata: {:?}", io_devices());
        let mut io = DiskIo::new();
        let mut cap = DiskCapacity::new();
        println!("io probe: {:?}", io.probe());
        println!("cap probe: {:?}", cap.probe());
        println!("volumes: {:?}", cap.volumes());
        let mut buf = SampleBuf::new();
        for n in 0..2 {
            buf.clear();
            let tick = Tick {
                n,
                wall_ms: 0,
                continuous_ns: super::super::sysctl::continuous_ns(),
                interval_ms: 1_000,
            };
            io.sample(&tick, &mut buf).unwrap();
            cap.sample(&tick, &mut buf).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        for s in buf.values() {
            println!("{} = {:.0}", s.key, s.value);
        }
        assert!(cap.volumes().iter().any(|v| v.is_boot));
        // Reprobe starts a new registry population: reused BSD names must never
        // compare a newly attached device against a previous device's counters.
        io.probe();
        buf.clear();
        io.sample(
            &Tick {
                n: 3,
                wall_ms: 0,
                continuous_ns: super::super::sysctl::continuous_ns(),
                interval_ms: 1000,
            },
            &mut buf,
        )
        .unwrap();
        assert!(buf.values().is_empty(), "reprobe requires a fresh baseline");
    }
}

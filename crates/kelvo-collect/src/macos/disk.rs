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

struct Device {
    driver: IoObject,
    read: SeriesKey,
    write: SeriesKey,
    prev: Option<(u64, u64)>,
}

/// `disk.read` / `disk.write` per physical device.
pub struct DiskIo {
    devices: Vec<Device>,
    prev_ns: u64,
    read_total: SeriesKey,
    write_total: SeriesKey,
    k_stats: Key,
    k_read: Key,
    k_write: Key,
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
            read_total: SeriesKey::bare(MetricId::from_static("disk.read_total")),
            write_total: SeriesKey::bare(MetricId::from_static("disk.write_total")),
            k_stats: Key::new("Statistics"),
            k_read: Key::new("Bytes (Read)"),
            k_write: Key::new("Bytes (Write)"),
            k_bsd: Key::new("BSD Name"),
        }
    }

    /// The device names found by the last probe.
    pub fn devices(&self) -> impl Iterator<Item = &str> {
        self.devices.iter().filter_map(|d| d.read.labels.get("dev"))
    }

    fn read_bytes(&self, driver: &IoObject) -> Option<(u64, u64)> {
        let stats = iokit::dict_of(&driver.property(&self.k_stats)?)?;
        let r = iokit::as_i64(&iokit::get(&stats, &self.k_read)?)?;
        let w = iokit::as_i64(&iokit::get(&stats, &self.k_write)?)?;
        Some((r.max(0) as u64, w.max(0) as u64))
    }
}

/// Bytes per second between two cumulative readings; `None` on a reset.
fn rate(prev: u64, cur: u64, secs: f64) -> Option<f32> {
    (cur >= prev && secs > 0.0).then(|| ((cur - prev) as f64 / secs) as f32)
}

/// One device's read and write rates from its previous and current `(read, write)` byte
/// counters; each side `None` without both readings or on a reset.
fn device_rates(prev: Option<(u64, u64)>, cur: Option<(u64, u64)>, secs: f64) -> [Option<f32>; 2] {
    match (prev, cur) {
        (Some((pr, pw)), Some((r, w))) => [rate(pr, r, secs), rate(pw, w, secs)],
        _ => [None, None],
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
                read: key("disk.read"),
                write: key("disk.write"),
                prev: None,
            };
            series.extend([dev.read.clone(), dev.write.clone()]);
            self.devices.push(dev);
        }
        if self.devices.is_empty() {
            Probe::NotPresent
        } else {
            series.extend([self.read_total.clone(), self.write_total.clone()]);
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
        let mut read_total = PartSum::default();
        let mut write_total = PartSum::default();
        for i in 0..self.devices.len() {
            let Some(dev) = self.devices.get(i) else {
                continue;
            };
            let cur = self.read_bytes(&dev.driver);
            let Some(dev) = self.devices.get_mut(i) else {
                continue;
            };
            let [read, write] = device_rates(dev.prev, cur, secs);
            if let Some(v) = read {
                out.push(&dev.read, v);
            }
            if let Some(v) = write {
                out.push(&dev.write, v);
            }
            read_total.add(read);
            write_total.add(write);
            dev.prev = cur;
        }
        if !self.devices.is_empty() {
            if let Some(v) = read_total.total() {
                out.push(&self.read_total, v);
            }
            if let Some(v) = write_total.total() {
                out.push(&self.write_total, v);
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

fn c_str(buf: &[libc::c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

unsafe extern "C" {
    /// `getmntinfo` with a buffer of the caller's own, freed with `free` (macOS 10.13+).
    /// Plain `getmntinfo` reuses one process-wide buffer, which races when the shell reads
    /// `boot_mounts` while the engine's capacity collector samples. The symbol carries the
    /// `$INODE64` suffix on Intel, as `libc` declares `getmntinfo`.
    #[cfg_attr(target_arch = "x86_64", link_name = "getmntinfo_r_np$INODE64")]
    fn getmntinfo_r_np(mntbufp: *mut *mut libc::statfs, flags: libc::c_int) -> libc::c_int;
}

/// Mounted local, browsable volumes.
fn mounted() -> Vec<(VolumeInfo, u64, u64)> {
    let mut mounts: *mut libc::statfs = std::ptr::null_mut();
    // SAFETY: `mounts` is an out-pointer; on success it points to `n` entries this call
    // allocated, which are freed below after reading.
    let n = unsafe { getmntinfo_r_np(&mut mounts, libc::MNT_NOWAIT) };
    if n <= 0 || mounts.is_null() {
        return Vec::new();
    }
    // SAFETY: getmntinfo_r_np returned `n` contiguous statfs entries at `mounts`.
    let all = unsafe { std::slice::from_raw_parts(mounts, n as usize) };
    let out = volumes_of(all);
    // SAFETY: the buffer was malloc'ed by getmntinfo_r_np and nothing borrows it now.
    unsafe { libc::free(mounts.cast()) };
    out
}

fn volumes_of(all: &[libc::statfs]) -> Vec<(VolumeInfo, u64, u64)> {
    let mut out = Vec::new();
    for fs in all {
        let flags = fs.f_flags;
        if flags & libc::MNT_LOCAL as u32 == 0 || flags & libc::MNT_DONTBROWSE as u32 != 0 {
            continue;
        }
        let mount = c_str(&fs.f_mntonname);
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
                fs_type: c_str(&fs.f_fstypename),
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
        // At most every 60 s, so the small allocation in `mounted` is not a steady-state cost.
        for (info, total, free) in mounted() {
            let Some(v) = self.volumes.iter().find(|v| v.info.mount == info.mount) else {
                continue;
            };
            out.push(&v.total, total as f32);
            out.push(&v.free, free as f32);
            out.push(&v.used, total.saturating_sub(free) as f32);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_disk_strips_slices() {
        assert_eq!(whole_disk("disk3s1s1").as_deref(), Some("disk3"));
        assert_eq!(whole_disk("disk12s2").as_deref(), Some("disk12"));
        assert_eq!(whole_disk("disk0").as_deref(), Some("disk0"));
        assert_eq!(whole_disk("map auto_home"), None);
        assert_eq!(whole_disk("disks"), None);
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
    }
}

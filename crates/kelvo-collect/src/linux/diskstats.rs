//! Disk I/O per whole disk from `/proc/diskstats`: the `disk.*` series the macOS
//! collector emits, labelled `dev` with the kernel name (`nvme0n1`, `sda`).
//!
//! Which disks: see [`super::blockdev`] (hardware-backed whole disks; partitions,
//! virtual and stacked devices left out so totals do not double count). Sectors in
//! `/proc/diskstats` are always 512 bytes, whatever the device's logical block size.
//!
//! Per disk and sample, from the deltas of cumulative counters (fields 4 to 11 of
//! diskstats): `disk.read`/`disk.write` (bytes/s), `disk.read_ops`/`disk.write_ops`
//! (completed requests/s), and the D-110 pairs `disk.*_timed_ops` / `disk.*_time`, the
//! completed requests and the milliseconds they took, from the same interval. The kernel
//! adds a request's time when it completes, so the pair describes the same requests;
//! it is omitted when the time moved with no completed request. A decrease is a reset
//! (a device re-attached under the same name) and yields no value for that counter: the
//! agent runs on 64-bit kernels, whose counters do not wrap in practice.
//!
//! `disk.*_total` sum the disks' values of the same sample, a gap when any disk's own
//! value is a gap (D-092). The disk set is rescanned every 30 s; when it changed (a USB
//! disk plugged in) the collector bumps its series revision and the re-probe keeps the
//! baselines of the disks that stayed.

use std::io::Read;
use std::path::PathBuf;

use kelvo_schema::{DiskDevice, Entitlement, Labels, MetricId, Module, SeriesKey};

use super::blockdev::{self, BlockLink};
use super::counters::{PartSum, span_secs};
use super::{SysRoot, mountinfo};
use crate::{Cadence, CollectError, Collector, CollectorId, Every, Probe, SampleBuf, Tick};

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

/// How often the disk set is re-checked for attached or removed disks.
const RESCAN_MS: u32 = 30_000;

/// Read bytes, write bytes, read ops, write ops, read ms, write ms.
type Counters = [u64; 6];

/// The counters on one diskstats line, with its device name.
fn parse_line(line: &str) -> Option<(&str, Counters)> {
    let mut it = line.split_ascii_whitespace();
    let _major = it.next()?;
    let _minor = it.next()?;
    let name = it.next()?;
    let mut f = [0u64; 8];
    for slot in &mut f {
        *slot = it.next()?.parse().ok()?;
    }
    // reads, reads merged, sectors read, ms reading, writes, writes merged, sectors
    // written, ms writing.
    let [reads, _, sectors_r, ms_r, writes, _, sectors_w, ms_w] = f;
    Some((
        name,
        [
            sectors_r.saturating_mul(512),
            sectors_w.saturating_mul(512),
            reads,
            writes,
            ms_r,
            ms_w,
        ],
    ))
}

fn rate(prev: u64, cur: u64, secs: f64) -> Option<f32> {
    (cur >= prev && secs > 0.0).then(|| ((cur - prev) as f64 / secs) as f32)
}

/// The eight per-disk values in [`IO_METRICS`] order. Read and write pair
/// independently; a timing pair needs both of its counters valid and, when time
/// moved, at least one completed request.
fn counter_rates(
    prev: Option<Counters>,
    cur: Option<Counters>,
    secs: Option<f64>,
) -> [Option<f32>; 8] {
    let (Some(prev), Some(cur), Some(secs)) = (prev, cur, secs) else {
        return [None; 8];
    };
    let v: [Option<f32>; 6] = std::array::from_fn(|i| rate(prev[i], cur[i], secs));
    let mut out = [v[0], v[1], v[2], v[3], None, None, None, None];
    for side in 0..2 {
        if let (Some(ops), Some(ms)) = (v[2 + side], v[4 + side])
            && (ops > 0. || ms == 0.)
        {
            out[4 + side] = Some(ops);
            out[6 + side] = Some(ms);
        }
    }
    out
}

struct Device {
    dev: String,
    keys: [SeriesKey; 8],
    prev: Option<Counters>,
    /// Found on this sample's diskstats read.
    cur: Option<Counters>,
}

/// `disk.read`/`write`/`*_ops`/`*_timed_ops`/`*_time` per disk, and their totals.
pub struct DiskStats {
    root: SysRoot,
    link: BlockLink,
    path: PathBuf,
    buf: String,
    devices: Vec<Device>,
    inventory: Vec<DiskDevice>,
    totals: [SeriesKey; 8],
    prev_ns: u64,
    rescan: Every,
    /// Set by `probe`: the first rescan due after it is the probe's own scan.
    fresh: bool,
    revision: u64,
    /// The next probe answers this collector's own revision bump: keep baselines.
    keep_baseline: bool,
}

impl Default for DiskStats {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskStats {
    pub fn new() -> Self {
        Self::with(SysRoot::host(), Box::new(blockdev::sys_block_link))
    }

    pub(super) fn with(root: SysRoot, link: BlockLink) -> Self {
        Self {
            path: root.path("/proc/diskstats"),
            root,
            link,
            buf: String::new(),
            devices: Vec::new(),
            inventory: Vec::new(),
            totals: kelvo_schema::disk_io::TOTAL_METRICS
                .map(|id| SeriesKey::bare(MetricId::from_static(id))),
            prev_ns: 0,
            rescan: Every::new(RESCAN_MS),
            fresh: true,
            revision: 0,
            keep_baseline: false,
        }
    }

    fn read_diskstats(&mut self) -> Result<(), CollectError> {
        self.buf.clear();
        std::fs::File::open(&self.path)
            .and_then(|mut f| f.read_to_string(&mut self.buf))
            .map(drop)
            .map_err(|e| CollectError::Os {
                call: "read(/proc/diskstats)",
                code: i64::from(e.raw_os_error().unwrap_or(0)),
            })
    }

    /// Fills each device's `cur` from `self.buf`.
    fn match_lines(&mut self) {
        for d in &mut self.devices {
            d.cur = None;
        }
        for line in self.buf.lines() {
            let Some((name, counters)) = parse_line(line) else {
                continue;
            };
            if let Some(d) = self.devices.iter_mut().find(|d| d.dev == name) {
                d.cur = Some(counters);
            }
        }
    }

    fn emit(&mut self, now: u64, out: &mut SampleBuf) {
        let secs = span_secs(self.prev_ns, now);
        let mut totals = [PartSum::default(); 8];
        for d in &mut self.devices {
            let values = counter_rates(d.prev, d.cur, secs);
            for ((key, value), total) in d.keys.iter().zip(values).zip(&mut totals) {
                if let Some(v) = value {
                    out.push(key, v);
                }
                total.add(value);
            }
            d.prev = d.cur;
        }
        if !self.devices.is_empty() {
            for (key, total) in self.totals.iter().zip(totals) {
                if let Some(v) = total.total() {
                    out.push(key, v);
                }
            }
        }
        self.prev_ns = now;
    }

    /// Whether the reported disk set differs from the probed one.
    fn set_changed(&self) -> bool {
        let names = blockdev::reported_names(&self.root, &self.link);
        !names.iter().eq(self.devices.iter().map(|d| &d.dev))
    }
}

impl Collector for DiskStats {
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
        let keep = std::mem::take(&mut self.keep_baseline);
        let old = std::mem::take(&mut self.devices);
        if !keep {
            self.prev_ns = 0;
        }
        // The probe is a scan: the next rescan is a full period from the next sample.
        self.rescan.reset();
        self.fresh = true;
        if self.read_diskstats().is_err() {
            self.inventory.clear();
            return Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::NoHardware,
            };
        }
        let listed: Vec<&str> = self
            .buf
            .lines()
            .filter_map(|l| parse_line(l).map(|(n, _)| n))
            .collect();
        let disks: Vec<_> = blockdev::scan(&self.root, &self.link)
            .into_iter()
            .filter(|d| listed.contains(&d.dev.as_str()))
            .collect();
        let mut series = Vec::new();
        for disk in &disks {
            let key =
                |id| SeriesKey::new(MetricId::from_static(id), Labels::single("dev", &disk.dev));
            let prev = old
                .iter()
                .find(|o| keep && o.dev == disk.dev)
                .and_then(|o| o.prev);
            let d = Device {
                dev: disk.dev.clone(),
                keys: IO_METRICS.map(key),
                prev,
                cur: None,
            };
            series.extend(d.keys.iter().cloned());
            self.devices.push(d);
        }
        if self.devices.is_empty() {
            self.inventory.clear();
            return Probe::NotPresent;
        }
        let mounts = self
            .root
            .read("/proc/self/mountinfo")
            .map(|t| mountinfo::parse(&t))
            .unwrap_or_default();
        let root_disk = blockdev::root_disk(&self.root, &disks, &mounts);
        self.inventory = blockdev::inventory(&disks, root_disk.as_deref());
        series.extend(self.totals.iter().cloned());
        Probe::Supported(series)
    }

    fn series_revision(&self) -> u64 {
        self.revision
    }

    fn fill_capabilities(&self, caps: &mut kelvo_schema::Capabilities) {
        caps.disk_devices.clone_from(&self.inventory);
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        self.read_diskstats()?;
        self.match_lines();
        self.emit(tick.continuous_ns, out);
        if self.rescan.due(tick) && !std::mem::take(&mut self.fresh) && self.set_changed() {
            self.revision += 1;
            self.keep_baseline = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::blockdev::tests::{fixture, fixture_links};
    use super::*;

    fn tick(n: u64, secs: u64) -> Tick {
        Tick {
            n,
            wall_ms: 0,
            continuous_ns: secs * 1_000_000_000,
            interval_ms: 1_000,
        }
    }

    fn values(buf: &SampleBuf) -> Vec<(String, f32)> {
        buf.values()
            .iter()
            .map(|s| (s.key.to_string(), s.value))
            .collect()
    }

    /// t0 -> t1 is one second apart. By hand, nvme0n1: reads 1378915816 -> ..824 (+8),
    /// sectors read +192 (98 304 B), read ms +1; writes +14, sectors written +288
    /// (147 456 B), write ms +1. sda, sdb and the Apple namespaces did not move.
    #[test]
    fn fixture_rates_match_hand_computed_deltas() {
        let mut c = DiskStats::with(fixture("t0"), fixture_links());
        let Probe::Supported(series) = c.probe() else {
            panic!("probe");
        };
        assert_eq!(series.len(), 5 * 8 + 8);
        let mut buf = SampleBuf::new();
        c.sample(&tick(0, 10), &mut buf).unwrap();
        assert!(buf.values().is_empty(), "the first sample is the baseline");
        c.path = fixture("t1").path("/proc/diskstats");
        c.sample(&tick(1, 11), &mut buf).unwrap();
        let get = |k: &str| {
            buf.values()
                .iter()
                .find(|s| s.key.to_string() == k)
                .map(|s| s.value)
        };
        assert_eq!(get("disk.read{dev=nvme0n1}"), Some(98_304.));
        assert_eq!(get("disk.write{dev=nvme0n1}"), Some(147_456.));
        assert_eq!(get("disk.read_ops{dev=nvme0n1}"), Some(8.));
        assert_eq!(get("disk.write_ops{dev=nvme0n1}"), Some(14.));
        assert_eq!(get("disk.read_timed_ops{dev=nvme0n1}"), Some(8.));
        assert_eq!(get("disk.read_time{dev=nvme0n1}"), Some(1.));
        assert_eq!(get("disk.write_time{dev=nvme0n1}"), Some(1.));
        assert_eq!(get("disk.read{dev=sda}"), Some(0.));
        assert_eq!(get("disk.read_total"), Some(98_304.));
        assert_eq!(get("disk.write_total"), Some(147_456.));
        assert_eq!(get("disk.write_ops_total"), Some(14.));
        assert_eq!(get("disk.read_time_total"), Some(1.));
        assert_eq!(
            get("disk.read{dev=nvme0n1p6}"),
            None,
            "partitions are not reported"
        );
        assert_eq!(buf.values().len(), 5 * 8 + 8);
        let mut caps = kelvo_schema::Capabilities::default();
        c.fill_capabilities(&mut caps);
        assert_eq!(caps.disk_devices.len(), 5);
        assert!(
            caps.disk_devices
                .iter()
                .any(|d| d.dev == "nvme0n1" && d.default)
        );
    }

    #[test]
    fn a_reset_disk_is_a_gap_in_its_series_and_the_totals() {
        let mut c = DiskStats::with(fixture("t0"), fixture_links());
        c.path = fixture("t1").path("/proc/diskstats");
        assert!(matches!(c.probe(), Probe::Supported(_)));
        let mut buf = SampleBuf::new();
        c.sample(&tick(0, 10), &mut buf).unwrap();
        // Back to t0: every nvme0n1 counter decreased; the others are unchanged.
        c.path = fixture("t0").path("/proc/diskstats");
        c.sample(&tick(1, 11), &mut buf).unwrap();
        let v = values(&buf);
        assert!(!v.iter().any(|(k, _)| k.contains("nvme0n1}")), "{v:?}");
        assert!(v.iter().any(|(k, v)| k == "disk.read{dev=sda}" && *v == 0.));
        assert!(!v.iter().any(|(k, _)| k.ends_with("_total")), "{v:?}");
    }

    #[test]
    fn timing_pairs_need_completed_requests() {
        let p = [0, 0, 10, 10, 100, 100];
        // Reads: 2 ops, 5 ms. Writes: time moved with no completion, so no pair.
        let c = [512, 0, 12, 10, 105, 103];
        assert_eq!(
            counter_rates(Some(p), Some(c), Some(1.)),
            [
                Some(512.),
                Some(0.),
                Some(2.),
                Some(0.),
                Some(2.),
                None,
                Some(5.),
                None
            ]
        );
        // Idle: zero ops and zero time is a valid zero pair.
        assert_eq!(
            counter_rates(Some(p), Some(p), Some(1.))[4..],
            [Some(0.); 4]
        );
        assert_eq!(counter_rates(None, Some(c), Some(1.)), [None; 8]);
        assert_eq!(counter_rates(Some(p), Some(c), None), [None; 8]);
        // A read-side reset leaves the write side.
        let reset = [0, 0, 1, 11, 1, 101];
        let r = counter_rates(Some(p), Some(reset), Some(1.));
        assert_eq!(
            (r[2], r[3], r[4], r[5], r[7]),
            (None, Some(1.), None, Some(1.), Some(1.))
        );
    }

    #[test]
    fn parse_uses_512_byte_sectors_and_skips_short_lines() {
        let (name, c) = parse_line(" 8 0 sda 1 0 4 7 2 0 8 9 0 10 16").unwrap();
        assert_eq!((name, c), ("sda", [2048, 4096, 1, 2, 7, 9]));
        assert_eq!(parse_line(" 8 0 sda 1 2 3"), None);
        assert_eq!(parse_line(" 8 0 sda 1 x 3 4 5 6 7 8"), None);
    }

    #[test]
    fn a_new_disk_bumps_the_revision_and_keeps_baselines() {
        let dir = tempfile::tempdir().unwrap();
        let root = SysRoot::at(dir.path());
        let write = |p: &str, s: &str| {
            let p = root.path(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, s).unwrap();
        };
        write("/sys/block/vda/size", "100\n");
        write(
            "/proc/diskstats",
            " 253 0 vda 1 0 2 3 4 0 5 6 0 0 0\n 7 0 loop0 9 0 9 9 9 0 9 9 0 0 0\n",
        );
        write("/sys/block/loop0/size", "100\n");
        let mut c = DiskStats::with(root.clone(), Box::new(blockdev::sys_block_link));
        assert!(matches!(c.probe(), Probe::Supported(s) if s.len() == 16));
        let mut buf = SampleBuf::new();
        c.sample(&tick(0, 10), &mut buf).unwrap();
        c.sample(&tick(1, 40), &mut buf).unwrap();
        assert_eq!(c.series_revision(), 0, "the loop device is not a new disk");
        write("/sys/block/vdb/size", "100\n");
        write(
            "/proc/diskstats",
            " 253 0 vda 2 0 2 3 4 0 5 6 0 0 0\n 253 16 vdb 1 0 1 1 1 0 1 1 0 0 0\n",
        );
        buf.clear();
        c.sample(&tick(2, 70), &mut buf).unwrap();
        assert_eq!(c.series_revision(), 1);
        assert!(matches!(c.probe(), Probe::Supported(s) if s.len() == 24));
        write(
            "/proc/diskstats",
            " 253 0 vda 5 0 2 3 4 0 5 6 0 0 0\n 253 16 vdb 1 0 1 1 1 0 1 1 0 0 0\n",
        );
        buf.clear();
        c.sample(&tick(3, 71), &mut buf).unwrap();
        let v = values(&buf);
        assert!(
            v.contains(&("disk.read_ops{dev=vda}".into(), 3.)),
            "kept baseline: {v:?}"
        );
        assert!(
            !v.iter().any(|(k, _)| k.contains("vdb")),
            "new disk starts from a baseline"
        );
        assert!(!v.iter().any(|(k, _)| k.ends_with("_total")));
    }
}

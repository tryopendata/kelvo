//! Filesystem capacity: `disk.used`, `disk.free`, `disk.total` per mounted filesystem,
//! labelled `vol` with the mount point, every 60 s. Same meaning as the macOS
//! collector: `total` is `f_blocks`, `free` is `f_bavail` (what a normal user can still
//! write, so `df`'s Avail), and `used = total - free`. On ext4 that counts the root
//! reserve as used, which `df`'s Used column does not.
//!
//! Which mounts (from `/proc/self/mountinfo`):
//! - backed by a block device: the source is a `/dev/` node (zfs datasets too, whose
//!   source is the dataset name). That leaves out tmpfs, proc, sysfs, cgroup, devpts,
//!   nsfs and the other kernel filesystems, overlay (container roots) and network
//!   filesystems (nfs, cifs), whose `statvfs` can block the engine on a dead server;
//! - not `squashfs` or `erofs` (snap and image mounts, read-only and always full);
//! - not `fuse.*` (sshfs, rclone, gvfs: remote or virtual, with the same blocking risk).
//!   `fuseblk` (ntfs-3g, exfat-fuse) is a local disk and stays;
//! - not under `/var/lib/docker`, `/var/lib/containers`, `/run`, `/proc`, `/sys`, `/dev`
//!   or `/snap`: runtime and container plumbing, often binds of a reported filesystem.
//!   `/run/media` stays: udisks mounts removable drives there.
//!
//! Mounts sharing a device number are one filesystem (btrfs subvolumes, bind mounts):
//! the shortest mount point stands for it, so `/`, `/var` and `/home` on one btrfs
//! report once as `/`. A filesystem with zero blocks is skipped.
//!
//! The mount table is re-read on every sample; when the filtered set changed the
//! collector bumps its series revision, so the engine re-probes it alone.

use std::ffi::CString;

use kelvo_schema::{Entitlement, Labels, MetricId, Module, SeriesKey};

use super::SysRoot;
use super::mountinfo::{self, Mount};
use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

const EXCLUDED_UNDER: [&str; 7] = [
    "/var/lib/docker",
    "/var/lib/containers",
    "/run",
    "/proc",
    "/sys",
    "/dev",
    "/snap",
];

/// Whether `path` is `dir` or below it.
pub(super) fn under(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

fn is_real(m: &Mount) -> bool {
    let block = m.source.starts_with("/dev/") || m.fstype == "zfs";
    block
        && !matches!(m.fstype.as_str(), "squashfs" | "erofs")
        && !m.fstype.starts_with("fuse.")
        && (under(&m.point, "/run/media") || !EXCLUDED_UNDER.iter().any(|d| under(&m.point, d)))
}

/// The reported filesystems in `mounts`, one per device number, sorted by mount point.
pub(super) fn real_mounts(mounts: &[Mount]) -> Vec<Mount> {
    let mut out: Vec<Mount> = Vec::new();
    for m in mounts.iter().filter(|m| is_real(m)) {
        match out.iter_mut().find(|o| o.dev == m.dev) {
            Some(o) => {
                if (m.point.len(), &m.point) < (o.point.len(), &o.point) {
                    *o = m.clone();
                }
            }
            None => out.push(m.clone()),
        }
    }
    out.sort_by(|a, b| a.point.cmp(&b.point));
    out
}

/// Total and available bytes of the filesystem at `path`.
pub(super) fn statvfs(path: &CString) -> Option<(u64, u64)> {
    let mut st = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is NUL-terminated and `st` is a writable statvfs the call fills.
    if unsafe { libc::statvfs(path.as_ptr(), st.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statvfs returned 0, so it initialised `st`.
    let st = unsafe { st.assume_init() };
    #[allow(
        clippy::useless_conversion,
        reason = "c_ulong is u32 on 32-bit targets"
    )]
    let frsize = u64::from(if st.f_frsize > 0 {
        st.f_frsize
    } else {
        st.f_bsize
    });
    #[allow(
        clippy::useless_conversion,
        reason = "fsblkcnt_t is u32 on some targets"
    )]
    let (blocks, avail) = (u64::from(st.f_blocks), u64::from(st.f_bavail));
    Some((blocks.saturating_mul(frsize), avail.saturating_mul(frsize)))
}

struct Volume {
    point: String,
    dev: (u32, u32),
    c_path: CString,
    used: SeriesKey,
    free: SeriesKey,
    total: SeriesKey,
}

/// `disk.used`/`free`/`total{vol}`.
pub struct Filesystems {
    root: SysRoot,
    volumes: Vec<Volume>,
    /// The filtered mount table the last probe saw, before the zero-size check, as
    /// (mount point, device): what a later read is compared against.
    probed: Vec<(String, (u32, u32))>,
    revision: u64,
}

impl Default for Filesystems {
    fn default() -> Self {
        Self::new()
    }
}

impl Filesystems {
    pub fn new() -> Self {
        Self::with(SysRoot::host())
    }

    pub(super) fn with(root: SysRoot) -> Self {
        Self {
            root,
            volumes: Vec::new(),
            probed: Vec::new(),
            revision: 0,
        }
    }

    fn mounts(&self) -> Result<Vec<Mount>, CollectError> {
        let text = self
            .root
            .read("/proc/self/mountinfo")
            .map_err(|e| CollectError::Os {
                call: "read(/proc/self/mountinfo)",
                code: i64::from(e.raw_os_error().unwrap_or(0)),
            })?;
        Ok(real_mounts(&mountinfo::parse(&text)))
    }

    /// `statvfs` path for a mount point: under the SysRoot, so fixtures resolve too.
    fn c_path(&self, point: &str) -> Option<CString> {
        CString::new(self.root.path(point).into_os_string().into_encoded_bytes()).ok()
    }
}

fn key_set(mounts: &[Mount]) -> Vec<(String, (u32, u32))> {
    mounts.iter().map(|m| (m.point.clone(), m.dev)).collect()
}

impl Collector for Filesystems {
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
        let Ok(mounts) = self.mounts() else {
            self.probed.clear();
            return Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::NoHardware,
            };
        };
        self.probed = key_set(&mounts);
        let mut series = Vec::new();
        for m in mounts {
            let Some(c_path) = self.c_path(&m.point) else {
                continue;
            };
            if statvfs(&c_path).is_none_or(|(total, _)| total == 0) {
                continue;
            }
            let key =
                |id| SeriesKey::new(MetricId::from_static(id), Labels::single("vol", &m.point));
            let v = Volume {
                used: key("disk.used"),
                free: key("disk.free"),
                total: key("disk.total"),
                point: m.point,
                dev: m.dev,
                c_path,
            };
            series.extend([v.used.clone(), v.free.clone(), v.total.clone()]);
            self.volumes.push(v);
        }
        Probe::Supported(series)
    }

    fn series_revision(&self) -> u64 {
        self.revision
    }

    fn sample(&mut self, _tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let current = key_set(&self.mounts()?);
        if current != self.probed {
            self.revision += 1;
            // Report it once; the re-probe resets `probed`.
            self.probed.clone_from(&current);
        }
        for v in &self.volumes {
            // A mount that went away would stat the directory beneath it instead.
            if !current.iter().any(|(p, d)| *p == v.point && *d == v.dev) {
                continue;
            }
            let Some((total, free)) = statvfs(&v.c_path) else {
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
    use super::super::blockdev::tests::fixture;
    use super::*;

    fn points(mounts: &[Mount]) -> Vec<&str> {
        mounts.iter().map(|m| m.point.as_str()).collect()
    }

    #[test]
    fn fixture_mounts_dedupe_btrfs_and_drop_plumbing() {
        let text = fixture("t0").read("/proc/self/mountinfo").unwrap();
        let all = mountinfo::parse(&text);
        assert!(all.len() > 50);
        let real = real_mounts(&all);
        // `/var`, `/home` and `/var/lib/docker/btrfs` share `/`'s 0:41; /tmp and the
        // firmware mount are tmpfs; the netns, credentials and cgroup mounts are not
        // block-backed.
        assert_eq!(
            points(&real),
            [
                "/",
                "/boot",
                "/boot/efi",
                "/media/usb-drive",
                "/mnt/opendata-cache"
            ]
        );
        assert_eq!(real[0].dev, (0, 41));
    }

    fn m(point: &str, dev: (u32, u32), fstype: &str, source: &str) -> Mount {
        Mount {
            dev,
            root: "/".into(),
            point: point.into(),
            fstype: fstype.into(),
            source: source.into(),
        }
    }

    #[test]
    fn filter_rules_each_bite() {
        let cases = [
            (m("/data", (8, 1), "ext4", "/dev/sda1"), true),
            (m("/tank", (0, 50), "zfs", "tank/data"), true),
            (m("/win", (8, 2), "fuseblk", "/dev/sda2"), true),
            (m("/srv/nfs", (0, 51), "nfs4", "nas:/export"), false),
            (m("/", (0, 52), "overlay", "overlay"), false),
            (m("/snap/core/1", (7, 0), "squashfs", "/dev/loop0"), false),
            (m("/media/image", (7, 1), "squashfs", "/dev/loop1"), false),
            (m("/mnt/x", (0, 53), "fuse.sshfs", "/dev/fake"), false),
            (
                m("/var/lib/docker/overlay2", (8, 1), "ext4", "/dev/sda1"),
                false,
            ),
            (m("/run/media/me/usb", (8, 17), "vfat", "/dev/sdb1"), true),
            (m("/run/foo", (8, 18), "ext4", "/dev/sdb2"), false),
            (m("/dev/shm", (0, 27), "tmpfs", "tmpfs"), false),
            (m("/runner", (8, 33), "ext4", "/dev/sdc1"), true),
        ];
        for (mount, want) in cases {
            assert_eq!(is_real(&mount), want, "{mount:?}");
        }
    }

    #[test]
    fn dedupe_keeps_the_shortest_point() {
        let real = real_mounts(&[
            m("/home", (0, 41), "btrfs", "/dev/nvme0n1p6"),
            m("/", (0, 41), "btrfs", "/dev/nvme0n1p6"),
            m("/var", (0, 41), "btrfs", "/dev/nvme0n1p6"),
            m("/mnt/b", (8, 1), "ext4", "/dev/sda1"),
            m("/mnt/a", (8, 1), "ext4", "/dev/sda1"),
        ]);
        assert_eq!(points(&real), ["/", "/mnt/a"]);
    }

    #[test]
    fn statvfs_capacity_and_mount_changes_bump_the_revision() {
        let dir = tempfile::tempdir().unwrap();
        let root = SysRoot::at(dir.path());
        std::fs::create_dir_all(root.path("/proc/self")).unwrap();
        // The temp dir's own filesystem stands in for both mounts.
        let table = |extra: &str| {
            std::fs::write(
                root.path("/proc/self/mountinfo"),
                format!("1 0 8:1 / / rw - ext4 /dev/sda1 rw\n{extra}"),
            )
            .unwrap();
        };
        table("");
        let mut c = Filesystems::with(root.clone());
        assert!(matches!(c.probe(), Probe::Supported(s) if s.len() == 3));
        let mut buf = SampleBuf::new();
        let t = Tick {
            n: 0,
            wall_ms: 0,
            continuous_ns: 1,
            interval_ms: 1_000,
        };
        c.sample(&t, &mut buf).unwrap();
        let get = |b: &SampleBuf, k: &str| {
            b.values()
                .iter()
                .find(|s| s.key.to_string() == k)
                .map(|s| s.value)
        };
        let (total, free, used) = (
            get(&buf, "disk.total{vol=/}").unwrap(),
            get(&buf, "disk.free{vol=/}").unwrap(),
            get(&buf, "disk.used{vol=/}").unwrap(),
        );
        assert!(total > 0. && free <= total);
        assert!((total - free - used).abs() <= total * 1e-6);
        assert_eq!(c.series_revision(), 0);
        // A container's overlay appearing changes nothing.
        table("2 1 0:90 / /var/lib/docker/overlay2/x/merged rw - overlay overlay rw\n");
        c.sample(&t, &mut buf).unwrap();
        assert_eq!(c.series_revision(), 0);
        // A USB disk does, once.
        table("3 1 8:17 / /media/usb rw - vfat /dev/sdb1 rw\n");
        c.sample(&t, &mut buf).unwrap();
        c.sample(&t, &mut buf).unwrap();
        assert_eq!(c.series_revision(), 1);
        std::fs::create_dir_all(root.path("/media/usb")).unwrap();
        assert!(matches!(c.probe(), Probe::Supported(s) if s.len() == 6));
        // The root mount went away: its series stop rather than stat what is beneath.
        std::fs::write(
            root.path("/proc/self/mountinfo"),
            "3 1 8:17 / /media/usb rw - vfat /dev/sdb1 rw\n",
        )
        .unwrap();
        buf.clear();
        c.sample(&t, &mut buf).unwrap();
        assert_eq!(get(&buf, "disk.total{vol=/}"), None);
        assert!(get(&buf, "disk.total{vol=/media/usb}").is_some());
    }
}

//! Which block devices the disk I/O collector reports, and their inventory for
//! `Capabilities::disk_devices` (D-116).
//!
//! A reported disk is a `/sys/block` entry (whole disks only; partitions live under
//! their disk) that is backed by hardware. Excluded:
//! - anything whose `/sys/block/<dev>` link resolves under `/sys/devices/virtual`
//!   (loop, ram, zram, nbd, dm-*, md*), and those names even when the link cannot be read;
//! - optical drives (`sr*`), which only show reads of the disc in the tray;
//! - devices with no media (`size` 0, an empty card reader).
//!
//! Device-mapper (LVM, LUKS) and md RAID arrays are left out on purpose: they are stacked
//! on reported disks, whose own counters already include every request that reaches the
//! hardware, so keeping them would count the same bytes twice in `disk.*_total`.
//!
//! `internal` is false for a USB-attached disk (the resolved link passes through a `usb`
//! bus node) or one the kernel marks removable, true for any other disk whose link
//! resolved, and unknown otherwise. The default disk is the one backing `/` (through
//! its partition, or a dm/md stack's first member), else the largest internal disk,
//! else the first.

use std::path::Path;

use kelvo_schema::DiskDevice;

use super::SysRoot;
use super::mountinfo::Mount;

/// How a `/sys/block/<dev>` symlink resolves: the link text (`../devices/platform/...`),
/// or `None` when unreadable. A seam so fixture trees, which cannot hold symlinks, can
/// supply the captured targets.
pub(super) type BlockLink = Box<dyn Fn(&SysRoot, &str) -> Option<String> + Send>;

/// The live link reader.
pub(super) fn sys_block_link(root: &SysRoot, dev: &str) -> Option<String> {
    let target = std::fs::read_link(root.path(&format!("/sys/block/{dev}"))).ok()?;
    Some(target.to_string_lossy().into_owned())
}

/// A disk the collector reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BlockDisk {
    pub(super) dev: String,
    /// Size in 512-byte sectors.
    pub(super) sectors: u64,
    pub(super) internal: Option<bool>,
    pub(super) name: String,
}

/// Names of virtual or stacked devices, excluded even when their link is unreadable.
fn excluded_name(dev: &str) -> bool {
    let numbered = |prefix: &str| {
        dev.strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
    };
    dev.starts_with("loop")
        || dev.starts_with("ram")
        || dev.starts_with("zram")
        || dev.starts_with("dm-")
        || numbered("md")
        || numbered("sr")
}

/// Whether a `/sys/block` entry is a reported disk, from its name and resolved link.
pub(super) fn is_reported(dev: &str, link: Option<&str>) -> bool {
    !excluded_name(dev) && !link.is_some_and(|l| l.contains("/devices/virtual/"))
}

fn internal(link: Option<&str>, removable: Option<u64>) -> Option<bool> {
    let link = link?;
    let usb = link.split('/').any(|part| part.starts_with("usb"));
    Some(!usb && removable != Some(1))
}

/// "Samsung Portable SSD T5" from `device/vendor` and `device/model`, skipping the
/// generic `ATA` vendor libata reports; the device name when neither is readable.
fn display_name(root: &SysRoot, dev: &str) -> String {
    let read = |attr: &str| root.read_trimmed(&format!("/sys/block/{dev}/device/{attr}"));
    let vendor = read("vendor").filter(|v| v != "ATA");
    match (vendor, read("model")) {
        (Some(v), Some(m)) => format!("{v} {m}"),
        (None, Some(m)) => m,
        (Some(v), None) => v,
        (None, None) => dev.to_owned(),
    }
}

/// The `/sys/block` entries, sorted. Empty when the directory is unreadable.
pub(super) fn block_names(root: &SysRoot) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.path("/sys/block"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

/// The reported disks' names, sorted: the cheap check the collector's periodic rescan
/// compares.
pub(super) fn reported_names(root: &SysRoot, link: &BlockLink) -> Vec<String> {
    block_names(root)
        .into_iter()
        .filter(|dev| is_reported(dev, link(root, dev).as_deref()))
        .filter(|dev| {
            root.read_u64(&format!("/sys/block/{dev}/size"))
                .unwrap_or(0)
                > 0
        })
        .collect()
}

/// The reported disks with their metadata, sorted by name.
pub(super) fn scan(root: &SysRoot, link: &BlockLink) -> Vec<BlockDisk> {
    let mut out = Vec::new();
    for dev in block_names(root) {
        let resolved = link(root, &dev);
        if !is_reported(&dev, resolved.as_deref()) {
            continue;
        }
        let sectors = root
            .read_u64(&format!("/sys/block/{dev}/size"))
            .unwrap_or(0);
        if sectors == 0 {
            continue;
        }
        let removable = root.read_u64(&format!("/sys/block/{dev}/removable"));
        out.push(BlockDisk {
            internal: internal(resolved.as_deref(), removable),
            name: display_name(root, &dev),
            sectors,
            dev,
        });
    }
    out
}

/// The kernel name of the block device a mount is on (`nvme0n1p6`, `dm-0`): through
/// `/sys/dev/block/<major>:<minor>` for a real device number, else the mount source.
/// btrfs reports an anonymous device number, so `/` on btrfs resolves by its source.
fn mount_block(root: &SysRoot, m: &Mount) -> Option<String> {
    let file_name = |p: &Path| p.file_name()?.to_str().map(str::to_owned);
    if m.dev.0 != 0
        && let Ok(t) =
            std::fs::read_link(root.path(&format!("/sys/dev/block/{}:{}", m.dev.0, m.dev.1)))
    {
        return file_name(&t);
    }
    if !m.source.starts_with("/dev/") {
        return None;
    }
    // `/dev/mapper/root` and `/dev/disk/by-uuid/...` are links to `../dm-0`.
    let src = root.path(&m.source);
    match std::fs::read_link(&src) {
        Ok(t) => file_name(&t),
        Err(_) => file_name(&src),
    }
}

/// The reported disk `name` (a disk, partition or stacked device) lives on.
fn whole_disk(root: &SysRoot, name: &str, disks: &[BlockDisk], depth: u8) -> Option<String> {
    if disks.iter().any(|d| d.dev == name) {
        return Some(name.to_owned());
    }
    if depth >= 4 {
        return None;
    }
    // dm and md: the first member device.
    let mut slaves: Vec<String> =
        std::fs::read_dir(root.path(&format!("/sys/block/{name}/slaves")))
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect();
    slaves.sort();
    if let Some(first) = slaves.first() {
        return whole_disk(root, first, disks, depth + 1);
    }
    // A partition is a directory under its disk.
    if let Some(d) = disks
        .iter()
        .find(|d| root.path(&format!("/sys/block/{}/{name}", d.dev)).is_dir())
    {
        return Some(d.dev.clone());
    }
    // Naming fallback: `nvme0n1p6` -> `nvme0n1`, `sda1` -> `sda`, `mmcblk0p2` -> `mmcblk0`.
    disks
        .iter()
        .filter(|d| {
            name.strip_prefix(d.dev.as_str()).is_some_and(|rest| {
                let digits = rest.strip_prefix('p').unwrap_or(rest);
                !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
            })
        })
        .max_by_key(|d| d.dev.len())
        .map(|d| d.dev.clone())
}

/// The reported disk backing `/` (the last `/` mount in the table is the visible one).
pub(super) fn root_disk(root: &SysRoot, disks: &[BlockDisk], mounts: &[Mount]) -> Option<String> {
    let m = mounts.iter().rev().find(|m| m.point == "/")?;
    whole_disk(root, &mount_block(root, m)?, disks, 0)
}

/// `Capabilities::disk_devices` for `disks`, the default flagged (see the module docs).
pub(super) fn inventory(disks: &[BlockDisk], root_disk: Option<&str>) -> Vec<DiskDevice> {
    let default = root_disk
        .filter(|r| disks.iter().any(|d| d.dev == *r))
        .or_else(|| {
            disks
                .iter()
                .filter(|d| d.internal == Some(true))
                .max_by_key(|d| d.sectors)
                .map(|d| d.dev.as_str())
        })
        .or_else(|| disks.first().map(|d| d.dev.as_str()));
    disks
        .iter()
        .map(|d| DiskDevice {
            dev: d.dev.clone(),
            name: d.name.clone(),
            internal: d.internal,
            default: default == Some(d.dev.as_str()),
        })
        .collect()
}

#[cfg(test)]
pub(super) mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::linux::mountinfo;

    pub(in crate::linux) fn fixture(t: &str) -> SysRoot {
        SysRoot::at(
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/linux/fixtures/asahi-m1").to_owned()
                + "/"
                + t,
        )
    }

    /// The captured `readlink /sys/block/*` targets (fixtures hold no symlinks).
    pub(in crate::linux) fn fixture_links() -> BlockLink {
        let text = fixture("t0")
            .read("/block_links.txt")
            .expect("fixture block_links.txt");
        let map: HashMap<String, String> = text
            .lines()
            .filter_map(|l| l.split_once(' '))
            .map(|(d, t)| (d.to_owned(), t.to_owned()))
            .collect();
        Box::new(move |_, dev| map.get(dev).cloned())
    }

    #[test]
    fn fixture_disks_and_default() {
        let root = fixture("t0");
        let disks = scan(&root, &fixture_links());
        let devs: Vec<(&str, Option<bool>, u64)> = disks
            .iter()
            .map(|d| (d.dev.as_str(), d.internal, d.sectors))
            .collect();
        assert_eq!(
            devs,
            [
                ("nvme0n1", Some(true), 490_234_752),
                ("nvme0n2", Some(true), 6144),
                ("nvme0n3", Some(true), 262_144),
                ("sda", Some(false), 243_404_800),
                ("sdb", Some(false), 976_773_168),
            ]
        );
        let mounts = mountinfo::parse(&root.read("/proc/self/mountinfo").expect("fixture"));
        // `/` is btrfs on 0:41, so the disk comes from the source `/dev/nvme0n1p6`.
        let r = root_disk(&root, &disks, &mounts);
        assert_eq!(r.as_deref(), Some("nvme0n1"));
        let inv = inventory(&disks, r.as_deref());
        let defaults: Vec<&str> = inv
            .iter()
            .filter(|d| d.default)
            .map(|d| d.dev.as_str())
            .collect();
        assert_eq!(defaults, ["nvme0n1"]);
        // No model files in the fixture: the name falls back to the device.
        assert_eq!(inv[0].name, "nvme0n1");
    }

    #[test]
    fn virtual_and_stacked_devices_are_excluded() {
        let v = Some("../devices/virtual/block/loop0");
        assert!(!is_reported("loop0", v));
        assert!(!is_reported("dm-0", Some("../devices/virtual/block/dm-0")));
        assert!(!is_reported(
            "md127",
            Some("../devices/virtual/block/md127")
        ));
        assert!(
            !is_reported("zram0", None),
            "by name when the link is unreadable"
        );
        assert!(!is_reported("nbd3", Some("../devices/virtual/block/nbd3")));
        assert!(!is_reported(
            "sr0",
            Some("../devices/pci0000:00/ata1/host0/block/sr0")
        ));
        assert!(is_reported(
            "sda",
            Some("../devices/pci0000:00/ata1/block/sda")
        ));
        assert!(is_reported("mmcblk0", None));
        assert!(
            !is_reported("md0", None),
            "by name when the link is unreadable"
        );
        assert!(is_reported("mdx", None), "md only with a number");
    }

    #[test]
    fn internal_follows_usb_and_removable() {
        let usb = Some("../devices/platform/soc/382280000.usb/xhci-hcd.0.auto/usb5/5-1/block/sda");
        assert_eq!(internal(usb, Some(0)), Some(false));
        assert_eq!(
            internal(Some("../devices/pci0000:00/nvme/nvme0/nvme0n1"), Some(0)),
            Some(true)
        );
        assert_eq!(
            internal(Some("../devices/pci0000:00/mmc0/block/mmcblk0"), Some(1)),
            Some(false)
        );
        assert_eq!(internal(None, Some(0)), None);
    }

    fn disk(dev: &str, sectors: u64, internal: Option<bool>) -> BlockDisk {
        BlockDisk {
            dev: dev.into(),
            sectors,
            internal,
            name: dev.into(),
        }
    }

    #[test]
    fn default_falls_back_to_largest_internal_then_first() {
        let disks = [
            disk("nvme0n1", 10, Some(true)),
            disk("sda", 99, Some(false)),
            disk("sdb", 50, Some(true)),
        ];
        let pick = |r| {
            inventory(&disks, r)
                .into_iter()
                .filter(|d| d.default)
                .map(|d| d.dev)
                .collect::<Vec<_>>()
        };
        assert_eq!(pick(Some("sda")), ["sda"]);
        assert_eq!(
            pick(Some("dm-0")),
            ["sdb"],
            "an unreported root disk is ignored"
        );
        assert_eq!(pick(None), ["sdb"]);
        let external = [disk("sda", 1, Some(false)), disk("sdb", 2, None)];
        let firsts: Vec<_> = inventory(&external, None)
            .into_iter()
            .filter(|d| d.default)
            .map(|d| d.dev)
            .collect();
        assert_eq!(firsts, ["sda"]);
        assert!(inventory(&[], None).is_empty());
    }

    #[test]
    fn root_disk_through_dev_numbers_partitions_and_slaves() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::at(dir.path());
        let mk = |p: &str| std::fs::create_dir_all(root.path(p)).expect("mkdir");
        mk("/sys/block/vda/vda3");
        mk("/sys/block/dm-0/slaves/vda3");
        mk("/sys/dev/block");
        std::os::unix::fs::symlink(
            "../../devices/virtual/block/dm-0",
            root.path("/sys/dev/block/253:0"),
        )
        .expect("symlink");
        let disks = [disk("vda", 100, Some(true))];
        let mount = |dev, source: &str| Mount {
            dev,
            root: "/".into(),
            point: "/".into(),
            fstype: "ext4".into(),
            source: source.into(),
        };
        // LVM root: 253:0 -> dm-0 -> slaves/vda3 -> vda.
        assert_eq!(
            root_disk(&root, &disks, &[mount((253, 0), "/dev/mapper/root")]).as_deref(),
            Some("vda")
        );
        // A partition directory under its disk.
        assert_eq!(
            root_disk(&root, &disks, &[mount((0, 30), "/dev/vda3")]).as_deref(),
            Some("vda")
        );
        // Not a block-backed root at all.
        assert_eq!(root_disk(&root, &disks, &[mount((0, 30), "rootfs")]), None);
        // Name fallback needs a numeric partition suffix.
        let disks = [disk("sda", 1, None), disk("sdab", 1, None)];
        assert_eq!(
            whole_disk(&root, "sdab2", &disks, 0).as_deref(),
            Some("sdab")
        );
        assert_eq!(whole_disk(&root, "sdx1", &disks, 0), None);
    }

    #[test]
    fn live_link_reader_reads_symlinks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::at(dir.path());
        std::fs::create_dir_all(root.path("/sys/block")).expect("mkdir");
        std::os::unix::fs::symlink(
            "../devices/virtual/block/loop0",
            root.path("/sys/block/loop0"),
        )
        .expect("symlink");
        assert_eq!(
            sys_block_link(&root, "loop0").as_deref(),
            Some("../devices/virtual/block/loop0")
        );
        assert_eq!(sys_block_link(&root, "sda"), None);
    }
}

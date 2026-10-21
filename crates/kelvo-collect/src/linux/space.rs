//! Space report volumes on Linux (D-117): the filesystems the capacity collector reports
//! (`fs::real_mounts`), each walked from its mount point with the portable lister.
//!
//! The walk covers one filesystem, which on btrfs is several device numbers: `/home`
//! and `/var` mounted from subvolumes have their own. From `/proc/self/mountinfo` the
//! plan enters those mounts' devices too, and skips the mount points of other
//! filesystems (`/proc`, `/boot`, a USB disk) and bind mounts showing a folder the walk
//! already reaches elsewhere. A subvolume nested in a folder without a mount (Docker's
//! image layers, snapshots) is not entered: snapshots share their extents, so their
//! files would count the same disk space many times over (on one host, 128 GB of
//! layers using 4.7 GB). What they hold stays in the report's unaccounted remainder.
//!
//! Linux has no privacy prompts, so nothing is protected; folders the agent cannot read
//! are counted as unreadable by the walk.
use std::collections::HashSet;
use std::ffi::CString;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use kelvo_schema::disk_space::SpaceVolume;

use super::SysRoot;
use super::fs::{real_mounts, statvfs, under};
use super::mountinfo::{self, Mount};
use crate::disk_space::{ScanError, ScanPlan};

/// The host's scannable filesystems, `/` first. Same used and total as the Disk page.
pub fn volumes() -> Vec<SpaceVolume> {
    volumes_in(&SysRoot::host())
}

/// Resolves a volume from [`volumes`] by its mount point.
pub fn plan(mount: &str, _include: &[String]) -> Result<ScanPlan, ScanError> {
    plan_in(&SysRoot::host(), mount)
}

fn volumes_in(root: &SysRoot) -> Vec<SpaceVolume> {
    let Ok(text) = root.read("/proc/self/mountinfo") else {
        return Vec::new();
    };
    let mut out: Vec<SpaceVolume> = real_mounts(&mountinfo::parse(&text))
        .iter()
        .filter_map(|m| {
            let path = CString::new(root.path(&m.point).into_os_string().into_encoded_bytes());
            let (total, free) = statvfs(&path.ok()?)?;
            (total > 0).then(|| SpaceVolume {
                mount: m.point.clone(),
                name: m.point.clone(),
                is_boot: m.point == "/",
                used_bytes: total.saturating_sub(free) as f64,
                total_bytes: total as f64,
                protected: Vec::new(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.is_boot.cmp(&a.is_boot).then(a.mount.cmp(&b.mount)));
    out
}

fn plan_in(root: &SysRoot, mount: &str) -> Result<ScanPlan, ScanError> {
    let unknown = || ScanError::UnknownVolume(mount.to_owned());
    let volume = volumes_in(root)
        .into_iter()
        .find(|v| v.mount == mount)
        .ok_or_else(unknown)?;
    let all = mountinfo::parse(&root.read("/proc/self/mountinfo").map_err(|_| unknown())?);
    // The last mount at a point is the one visible there.
    let top = all
        .iter()
        .rev()
        .find(|m| m.point == mount)
        .ok_or_else(unknown)?;
    let (skip, walked) = walk_mounts(&all, top);
    let devices = walked
        .iter()
        .filter_map(|m| std::fs::metadata(root.path(&m.point)).ok())
        .map(|meta| meta.dev())
        .collect();
    Ok(ScanPlan {
        root: root.path(&volume.mount),
        used_bytes: volume.used_bytes as u64,
        skip,
        devices,
        skipped: Vec::new(),
        siblings: Vec::new(),
        firmlinked: Vec::<PathBuf>::new(),
        threads: 2,
        volume,
    })
}

/// The mount points below `top`, relative to it, that the walk does not enter (another
/// filesystem's, or a view of `top`'s filesystem the walk reaches at another path), and
/// the mounts it walks: `top` and those of the same filesystem showing a part nothing
/// else shows (a btrfs subvolume).
fn walk_mounts<'a>(all: &'a [Mount], top: &'a Mount) -> (HashSet<PathBuf>, Vec<&'a Mount>) {
    let mut below: Vec<&Mount> = all
        .iter()
        .filter(|m| m.point != top.point && within(&m.point, &top.point))
        .collect();
    below.sort_by_key(|m| m.point.len());
    let mut walked = vec![top];
    let mut skip = HashSet::new();
    for m in below {
        let rel = Path::new(&m.point)
            .strip_prefix(&top.point)
            .unwrap_or(Path::new(&m.point))
            .to_path_buf();
        if skip.iter().any(|s| rel.starts_with(s)) {
            continue; // Inside a mount already skipped.
        }
        if m.dev == top.dev && !walked.iter().any(|w| shown_elsewhere(w, m)) {
            walked.push(m);
        } else {
            skip.insert(rel);
        }
    }
    (skip, walked)
}

/// Whether `path` is `dir` or below it, `/` included.
fn within(path: &str, dir: &str) -> bool {
    dir == "/" || under(path, dir)
}

/// Whether walked mount `w` shows `m`'s folder at a path other than `m`'s mount point.
fn shown_elsewhere(w: &Mount, m: &Mount) -> bool {
    let Some(inside) = m
        .root
        .strip_prefix(&w.root)
        .filter(|_| within(&m.root, &w.root))
    else {
        return false;
    };
    Path::new(&w.point).join(inside.trim_start_matches('/')) != Path::new(&m.point)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table with the temp dir's filesystem at `/` and `/data`, plus mounts the report
    /// leaves out: one volume per device, `/` first, and only listed volumes plan.
    #[test]
    fn volumes_are_the_reported_filesystems_and_plan_walks_their_mount() {
        let dir = tempfile::tempdir().unwrap();
        let root = SysRoot::at(dir.path());
        std::fs::create_dir_all(root.path("/proc/self")).unwrap();
        std::fs::create_dir_all(root.path("/data")).unwrap();
        std::fs::write(
            root.path("/proc/self/mountinfo"),
            "1 0 8:1 / / rw - ext4 /dev/sda1 rw\n\
             2 1 8:2 / /data rw - xfs /dev/sdb1 rw\n\
             3 1 8:1 /home /home rw - ext4 /dev/sda1 rw\n\
             4 1 0:22 / /proc rw - proc proc rw\n\
             5 1 0:30 / /run rw - tmpfs tmpfs rw\n\
             6 1 0:50 / /var/lib/docker/overlay2/x/merged rw - overlay overlay rw\n",
        )
        .unwrap();

        let volumes = volumes_in(&root);
        let mounts: Vec<&str> = volumes.iter().map(|v| v.mount.as_str()).collect();
        assert_eq!(mounts, ["/", "/data"]);
        assert!(volumes[0].is_boot && !volumes[1].is_boot);
        assert!(volumes[0].total_bytes > 0.0 && volumes[0].used_bytes <= volumes[0].total_bytes);
        assert!(volumes.iter().all(|v| v.protected.is_empty()));

        let plan = plan_in(&root, "/data").unwrap();
        assert_eq!(plan.root, root.path("/data"));
        assert_eq!(plan.volume, volumes[1]);
        assert!(plan.skip.is_empty() && plan.siblings.is_empty());
        assert_eq!(plan.devices.len(), 1);
        assert!(matches!(
            plan_in(&root, "/proc"),
            Err(ScanError::UnknownVolume(m)) if m == "/proc"
        ));
    }

    fn mount(dev: (u32, u32), root: &str, point: &str) -> Mount {
        Mount {
            dev,
            root: root.into(),
            point: point.into(),
            fstype: "btrfs".into(),
            source: "/dev/nvme0n1p3".into(),
        }
    }

    /// A Fedora-style btrfs root: `/home` and `/var` are subvolume mounts of the same
    /// filesystem and are walked, as is a folder bind-mounted onto itself; other
    /// filesystems and a bind mount of an already walked folder are skipped.
    #[test]
    fn btrfs_subvolumes_are_walked_and_other_mounts_skipped() {
        let fs = (0, 41);
        let all = [
            mount(fs, "/root", "/"),
            mount((259, 5), "/", "/boot"),
            mount((259, 4), "/", "/boot/efi"),
            mount(fs, "/var", "/var"),
            mount(fs, "/home", "/home"),
            mount((0, 22), "/", "/proc"),
            mount((0, 55), "/", "/mnt/cache"),
            mount(fs, "/var/lib/docker/btrfs", "/var/lib/docker/btrfs"),
            mount((0, 60), "/", "/var/lib/docker/overlay2/x/merged"),
            mount(fs, "/home/a/shared", "/srv/shared"),
        ];
        let (skip, walked) = walk_mounts(&all, &all[0]);
        let walked: Vec<&str> = walked.iter().map(|m| m.point.as_str()).collect();
        assert_eq!(walked, ["/", "/var", "/home", "/var/lib/docker/btrfs"]);
        let mut skip: Vec<&str> = skip.iter().map(|p| p.to_str().unwrap()).collect();
        skip.sort_unstable();
        assert_eq!(
            skip,
            [
                "boot",
                "mnt/cache",
                "proc",
                "srv/shared",
                "var/lib/docker/overlay2/x/merged"
            ]
        );

        let data = mount((8, 2), "/", "/data");
        assert!(walk_mounts(&all, &data).0.is_empty());
    }
}

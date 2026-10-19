//! macOS side of disk space scans: a `getattrlistbulk` lister (one call returns many
//! entries' metadata, several times faster than `readdir` plus `lstat`), worker thread
//! setup, and resolving a Finder volume to a scan plan.
//!
//! Measured on the development Mac (M-series, macOS 27, warm cache): 1.7 M files in 3.3 s
//! on 4 workers, against 18.6 s for `du -x -k -d 1`; per-folder totals within 0.1% of `du`.
//! `ATTR_VOL_SPACEUSED` gives each APFS volume's own usage, where `statfs` reports the
//! container's (the System, Data, Preboot, Recovery and VM volumes summed to the container
//! within 0.2 GB).
use std::collections::HashSet;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use kelvo_schema::disk_space::{SpaceProtected, SpaceVolume};

use super::disk::{browsable, c_str, whole_disk, with_mounts};
use crate::disk_space::{ListError, ListedFile, Lister, Listing, ScanError, ScanPlan, Sibling};

const DATA_MOUNT: &str = "/System/Volumes/Data";
const VDIR: u32 = 2;

/// Locations in the home folder macOS guards with privacy prompts or refuses without Full
/// Disk Access. A default scan does not enter them (plan: Permissions).
const PROTECTED: [(&str, &str); 11] = [
    ("Desktop", "Desktop"),
    ("Documents", "Documents"),
    ("Downloads", "Downloads"),
    ("Library/Containers", "App containers"),
    ("Library/Group Containers", "Shared app containers"),
    ("Library/Mobile Documents", "iCloud Drive"),
    ("Library/CloudStorage", "Cloud storage"),
    ("Pictures/Photos Library.photoslibrary", "Photos library"),
    ("Library/Mail", "Mail"),
    ("Library/Messages", "Messages"),
    ("Library/Safari", "Safari"),
];

unsafe extern "C" {
    fn setiopolicy_np(iotype: i32, scope: i32, policy: i32) -> i32;
    fn pthread_set_qos_class_self_np(qos: u32, relative_priority: i32) -> i32;
}
const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: i32 = 3;
const IOPOL_SCOPE_THREAD: i32 = 1;
const IOPOL_MATERIALIZE_DATALESS_FILES_OFF: i32 = 1;
const QOS_CLASS_UTILITY: u32 = 0x11;

/// A lister for one worker thread. Creating it sets the thread to utility QoS and stops
/// File Provider (iCloud Drive and other cloud folders) from downloading anything the walk
/// touches, contents or directory listings.
pub struct BulkLister {
    buf: Vec<u8>,
}

impl BulkLister {
    pub fn for_this_thread() -> Self {
        // SAFETY: both calls only change the calling thread's own scheduling and I/O policy;
        // failures leave the defaults, which are still correct, only less polite.
        unsafe {
            pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0);
            setiopolicy_np(
                IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
                IOPOL_SCOPE_THREAD,
                IOPOL_MATERIALIZE_DATALESS_FILES_OFF,
            );
        }
        Self {
            buf: vec![0; 256 * 1024],
        }
    }
}

struct Fd(i32);
impl Drop for Fd {
    fn drop(&mut self) {
        // SAFETY: the descriptor was opened by `list` and is closed exactly once.
        unsafe { libc::close(self.0) };
    }
}

impl Lister for BulkLister {
    fn list(&mut self, dir: &Path, dev: u64, out: &mut Listing) -> Result<(), ListError> {
        let path = CString::new(dir.as_os_str().as_bytes()).map_err(|_| ListError::Unreadable)?;
        // SAFETY: path is NUL-terminated; O_NOFOLLOW never follows a symlinked folder.
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(ListError::Unreadable);
        }
        let fd = Fd(fd);
        // SAFETY: zeroed stat is a valid out-parameter; fd is open.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(fd.0, &mut st) } != 0 {
            return Err(ListError::Unreadable);
        }
        if st.st_dev as u64 != dev {
            return Err(ListError::OtherDevice);
        }
        // SAFETY: an all-zero attrlist is valid; the fields set below select attributes.
        let mut attrs: libc::attrlist = unsafe { std::mem::zeroed() };
        attrs.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
        attrs.commonattr = libc::ATTR_CMN_RETURNED_ATTRS
            | libc::ATTR_CMN_NAME
            | libc::ATTR_CMN_OBJTYPE
            | libc::ATTR_CMN_FILEID;
        attrs.fileattr = libc::ATTR_FILE_LINKCOUNT | libc::ATTR_FILE_ALLOCSIZE;
        loop {
            // SAFETY: attrs and buf outlive the call; buf.len() is its capacity.
            let n = unsafe {
                libc::getattrlistbulk(
                    fd.0,
                    (&raw mut attrs).cast(),
                    self.buf.as_mut_ptr().cast(),
                    self.buf.len(),
                    u64::from(libc::FSOPT_PACK_INVAL_ATTRS),
                )
            };
            match n {
                0 => return Ok(()),
                n if n < 0 => return Err(ListError::Unreadable),
                n => parse_entries(&self.buf, n as usize, out),
            }
        }
    }
}

fn read<const N: usize>(buf: &[u8], at: usize) -> Option<[u8; N]> {
    buf.get(at..at + N)?.try_into().ok()
}

/// Parses `count` packed entries: length (u32), returned attribute set (5 × u32), name
/// reference (i32 offset from the reference, u32 length with NUL), object type (u32),
/// file id (u64), then for non-folders link count (u32) and allocated size (i64). Folder
/// entries end after the common attributes (observed on macOS 27, APFS). Values are
/// 4-byte aligned, so 8-byte fields are read unaligned. The name follows the fixed part.
fn parse_entries(buf: &[u8], count: usize, out: &mut Listing) {
    let mut at = 0;
    for _ in 0..count {
        let Some(len) = read::<4>(buf, at).map(u32::from_ne_bytes) else {
            return;
        };
        let field = at + 24;
        let end = at + len as usize;
        at = end;
        let (Some(off), Some(name_len), Some(kind)) = (
            read::<4>(buf, field).map(i32::from_ne_bytes),
            read::<4>(buf, field + 4).map(u32::from_ne_bytes),
            read::<4>(buf, field + 8).map(u32::from_ne_bytes),
        ) else {
            return;
        };
        let start = (field as i64 + i64::from(off)) as usize;
        let Some(raw) = buf
            .get(start..start + (name_len as usize).saturating_sub(1))
            .filter(|_| start + name_len as usize <= end)
        else {
            continue;
        };
        let name: Box<str> = String::from_utf8_lossy(raw).into();
        if kind == VDIR {
            out.dirs.push(name);
            continue;
        }
        let (Some(fileid), Some(links), Some(alloc)) = (
            read::<8>(buf, field + 12).map(u64::from_ne_bytes),
            read::<4>(buf, field + 20).map(u32::from_ne_bytes),
            read::<8>(buf, field + 24).map(i64::from_ne_bytes),
        ) else {
            return;
        };
        out.files.push(ListedFile {
            name,
            bytes: alloc.max(0) as u64,
            linked: (links > 1).then_some(fileid),
        });
    }
}

struct Mount {
    on: String,
    device: String,
    browsable: bool,
    total: u64,
    used: u64,
}

fn mounts() -> Vec<Mount> {
    with_mounts(|all| {
        all.iter()
            .filter(|fs| fs.f_flags & libc::MNT_LOCAL as u32 != 0)
            .map(|fs| {
                let from = c_str(&fs.f_mntfromname);
                let bsize = u64::from(fs.f_bsize);
                let total = fs.f_blocks.saturating_mul(bsize);
                Mount {
                    on: c_str(&fs.f_mntonname).into_owned(),
                    device: from.strip_prefix("/dev/").unwrap_or(&from).to_owned(),
                    browsable: browsable(fs),
                    total,
                    used: total.saturating_sub(fs.f_bavail.saturating_mul(bsize)),
                }
            })
            .collect()
    })
}

/// One volume attribute through `getattrlist`; the buffer starts with its u32 length.
fn volume_attr(mount: &str, attr: u32) -> Option<[u8; 512]> {
    let path = CString::new(mount).ok()?;
    // SAFETY: an all-zero attrlist is valid; volattr selects one attribute.
    let mut attrs: libc::attrlist = unsafe { std::mem::zeroed() };
    attrs.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
    attrs.volattr = libc::ATTR_VOL_INFO | attr;
    let mut buf = [0u8; 512];
    // SAFETY: path is NUL-terminated; attrs and buf outlive the call.
    let rc = unsafe {
        libc::getattrlist(
            path.as_ptr(),
            (&raw mut attrs).cast(),
            buf.as_mut_ptr().cast(),
            buf.len(),
            0,
        )
    };
    (rc == 0).then_some(buf)
}

fn volume_name(mount: &str) -> Option<String> {
    let buf = volume_attr(mount, libc::ATTR_VOL_NAME)?;
    let off = i32::from_ne_bytes(read::<4>(&buf, 4)?);
    let len = u32::from_ne_bytes(read::<4>(&buf, 8)?) as usize;
    let start = (4 + off) as usize;
    let raw = buf.get(start..start + len.saturating_sub(1))?;
    Some(String::from_utf8_lossy(raw).into_owned())
}

fn volume_used(mount: &str) -> Option<u64> {
    let buf = volume_attr(mount, libc::ATTR_VOL_SPACEUSED)?;
    u64::try_from(i64::from_ne_bytes(read::<8>(&buf, 4)?)).ok()
}

fn home_rel() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    home.strip_prefix("/").ok().map(Path::to_path_buf)
}

fn protected(root: &Path) -> Vec<SpaceProtected> {
    let Some(home) = home_rel() else {
        return Vec::new();
    };
    PROTECTED
        .iter()
        .map(|(rel, label)| (home.join(rel), label))
        .filter(|(rel, _)| std::fs::symlink_metadata(root.join(rel)).is_ok_and(|m| m.is_dir()))
        .map(|(rel, label)| SpaceProtected {
            key: rel.to_string_lossy().into_owned(),
            label: (*label).to_owned(),
        })
        .collect()
}

fn scan_root(mount: &Mount, all: &[Mount]) -> PathBuf {
    if mount.on == "/" {
        let container = whole_disk(&mount.device);
        if all
            .iter()
            .any(|m| m.on == DATA_MOUNT && whole_disk(&m.device) == container)
        {
            return PathBuf::from(DATA_MOUNT);
        }
    }
    PathBuf::from(&mount.on)
}

fn space_volume(mount: &Mount, all: &[Mount]) -> SpaceVolume {
    let root = scan_root(mount, all);
    SpaceVolume {
        name: volume_name(&mount.on).unwrap_or_else(|| mount.on.clone()),
        is_boot: mount.on == "/",
        used_bytes: mount.used as f64,
        total_bytes: mount.total as f64,
        protected: if mount.on == "/" {
            protected(&root)
        } else {
            Vec::new()
        },
        mount: mount.on.clone(),
    }
}

/// Volumes Finder shows that can be scanned, the boot volume first.
pub fn volumes() -> Vec<SpaceVolume> {
    let all = mounts();
    let mut out: Vec<SpaceVolume> = all
        .iter()
        .filter(|m| m.browsable && m.total > 0)
        .map(|m| space_volume(m, &all))
        .collect();
    out.sort_by(|a, b| b.is_boot.cmp(&a.is_boot).then(a.name.cmp(&b.name)));
    out
}

fn sibling_label(mount: &Mount) -> String {
    match (mount.on.as_str(), volume_name(&mount.on).as_deref()) {
        ("/", _) => "macOS system".into(),
        (_, Some("VM")) => "Swap and sleep image".into(),
        (_, Some(name)) => format!("{name} volume"),
        (on, None) => on.to_owned(),
    }
}

/// Resolves a Finder volume and the protected locations the user included.
pub fn plan(mount: &str, include: &[String]) -> Result<ScanPlan, ScanError> {
    let all = mounts();
    let target = all
        .iter()
        .find(|m| m.on == mount && m.browsable)
        .ok_or_else(|| ScanError::UnknownVolume(mount.to_owned()))?;
    let volume = space_volume(target, &all);
    let root = scan_root(target, &all);
    let root_device = all
        .iter()
        .find(|m| Path::new(&m.on) == root)
        .map(|m| m.device.clone());
    let container = whole_disk(&target.device);
    let siblings = all
        .iter()
        .filter(|m| container.is_some() && whole_disk(&m.device) == container)
        .filter(|m| Path::new(&m.on) != root && Some(&m.device) != root_device.as_ref())
        .filter_map(|m| {
            let bytes = volume_used(&m.on)?;
            (bytes >= 1 << 20).then(|| Sibling {
                label: sibling_label(m),
                bytes,
            })
        })
        .collect();
    let include: HashSet<&str> = include.iter().map(String::as_str).collect();
    let (skip, skipped) = volume
        .protected
        .iter()
        .filter(|p| !include.contains(p.key.as_str()))
        .map(|p| (PathBuf::from(&p.key), p.label.clone()))
        .unzip();
    Ok(ScanPlan {
        firmlinked: if volume.is_boot {
            firmlinks()
        } else {
            Vec::new()
        },
        threads: if volume.is_boot { 4 } else { 2 },
        used_bytes: target.used,
        siblings,
        skip,
        skipped,
        root,
        volume,
    })
}

/// The Data volume's paths that "/" shows through firmlinks.
fn firmlinks() -> Vec<PathBuf> {
    std::fs::read_to_string("/usr/share/firmlinks")
        .map(|text| parse_firmlinks(&text))
        .unwrap_or_default()
}

fn parse_firmlinks(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| line.split('\t').nth(1))
        .map(str::trim)
        .filter(|rel| !rel.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: u32, fileid: u64, links: u32, alloc: i64) -> Vec<u8> {
        // Folders stop after the file id; files add link count and allocated size.
        let fixed = if kind == VDIR { 44 } else { 56 };
        let mut e = Vec::new();
        let name_bytes = [name.as_bytes(), &[0]].concat();
        let padded = name_bytes.len().div_ceil(4) * 4;
        let len = (fixed + padded) as u32;
        e.extend(len.to_ne_bytes());
        e.extend([0u8; 20]);
        // Offset from the reference (at byte 24) to the name (after the fixed part).
        e.extend(((fixed - 24) as i32).to_ne_bytes());
        e.extend((name_bytes.len() as u32).to_ne_bytes());
        e.extend(kind.to_ne_bytes());
        e.extend(fileid.to_ne_bytes());
        if kind != VDIR {
            e.extend(links.to_ne_bytes());
            e.extend(alloc.to_ne_bytes());
        }
        e.extend(&name_bytes);
        e.resize(len as usize, 0);
        e
    }

    #[test]
    fn parses_packed_entries() {
        let buf = [
            entry("Library", VDIR, 9, 0, 0),
            entry("big.bin", 1, 42, 1, 4096),
            entry("linked", 1, 43, 2, 8192),
        ]
        .concat();
        let mut out = Listing::default();
        parse_entries(&buf, 3, &mut out);
        assert_eq!(out.dirs, vec![Box::<str>::from("Library")]);
        assert_eq!(out.files.len(), 2);
        assert_eq!(
            (&*out.files[0].name, out.files[0].bytes, out.files[0].linked),
            ("big.bin", 4096, None)
        );
        assert_eq!(out.files[1].linked, Some(43));
    }

    #[test]
    fn a_truncated_buffer_stops_parsing() {
        let mut buf = entry("a", 1, 1, 1, 512);
        buf.truncate(30);
        let mut out = Listing::default();
        parse_entries(&buf, 2, &mut out);
        assert!(out.files.is_empty());
    }

    #[test]
    fn firmlinks_are_the_data_side_paths() {
        let text = "/Applications\tApplications\n/System/Library/Caches\tSystem/Library/Caches\n\n";
        assert_eq!(
            parse_firmlinks(text),
            [
                PathBuf::from("Applications"),
                PathBuf::from("System/Library/Caches")
            ]
        );
    }

    #[test]
    fn the_bulk_lister_reads_a_real_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("f"), vec![1u8; 10_000]).unwrap();
        let dev = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(dir.path()).unwrap().dev()
        };
        let mut out = Listing::default();
        BulkLister::for_this_thread()
            .list(dir.path(), dev, &mut out)
            .unwrap();
        assert_eq!(out.dirs, vec![Box::<str>::from("sub")]);
        assert_eq!(&*out.files[0].name, "f");
        assert!(out.files[0].bytes >= 10_000);
        assert_eq!(
            BulkLister::for_this_thread().list(dir.path(), dev + 1, &mut Listing::default()),
            Err(ListError::OtherDevice)
        );
    }

    #[test]
    fn the_boot_volume_resolves_to_the_data_volume() {
        let vols = volumes();
        let Some(boot) = vols.iter().find(|v| v.is_boot) else {
            return; // Not every CI runner has a browsable boot volume.
        };
        let plan = plan(&boot.mount, &[]).unwrap();
        if Path::new(DATA_MOUNT).exists() {
            assert_eq!(plan.root, PathBuf::from(DATA_MOUNT));
            assert!(plan.siblings.iter().any(|s| s.label == "macOS system"));
        }
        assert!(plan.siblings.iter().map(|s| s.bytes).sum::<u64>() < plan.used_bytes);
    }

    /// Hand check against `du`: `KELVO_SPACE_ROOT=~/Projects cargo test -p kelvo-collect
    /// --release real_tree -- --ignored --nocapture`.
    #[test]
    #[ignore = "walks a real folder named by KELVO_SPACE_ROOT"]
    fn real_tree_matches_du() {
        use crate::disk_space::Scan;
        let root = PathBuf::from(std::env::var_os("KELVO_SPACE_ROOT").expect("KELVO_SPACE_ROOT"));
        let vol = volumes().into_iter().next().expect("a volume");
        let plan = ScanPlan {
            volume: vol,
            root: root.clone(),
            skip: HashSet::new(),
            skipped: Vec::new(),
            siblings: Vec::new(),
            // As the boot volume would: KELVO_SPACE_USED in bytes sets the merge floor.
            used_bytes: std::env::var("KELVO_SPACE_USED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            firmlinked: Vec::new(),
            threads: 4,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let scan = Scan::start(plan, BulkLister::for_this_thread, move || {
            let _ = tx.send(());
        })
        .unwrap();
        rx.recv().unwrap();
        let p = scan.progress();
        let du = std::process::Command::new("/usr/bin/du")
            .args(["-skx"])
            .arg(&root)
            .output()
            .unwrap();
        let du_kb: u64 = String::from_utf8_lossy(&du.stdout)
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let ours_kb = p.measured / 1024;
        eprintln!(
            "scanned {} files in {:.2}s; retained nodes {}; ours {} KB, du {} KB ({:+.3}%)",
            p.items,
            p.elapsed_ms / 1000.0,
            scan.retained_nodes(),
            ours_kb,
            du_kb,
            (ours_kb as f64 - du_kb as f64) / du_kb as f64 * 100.0
        );
        assert!((ours_kb as f64 - du_kb as f64).abs() / (du_kb as f64) < 0.01);
    }
}

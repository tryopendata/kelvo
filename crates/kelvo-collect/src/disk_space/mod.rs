//! On-demand disk space scans (plan/disk-space-report.md). Not a `Collector`: no tick,
//! no series. A scan walks one volume on its own threads and keeps a size tree that the
//! UI reads while it runs and after it finishes.
mod portable;
mod tree;
mod walk;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use kelvo_schema::disk_space::{
    SpaceCrumb, SpaceFile, SpaceKind, SpaceNode, SpacePhase, SpaceReport, SpaceSubtree,
    SpaceTarget, SpaceVolume,
};
use kelvo_schema::lock::LockExt;

pub use portable::StdLister;
pub use walk::{ListError, ListedFile, Lister, Listing};

#[cfg(test)]
mod tests;

/// Entries under this fraction of the volume's used space (1 MB per TB) are merged into
/// their folder's smaller items, wherever they are.
pub const MIN_KEEP_DIVISOR: u64 = 1_000_000;

/// What a finished root's remainder holds, as the platform's filesystem has it.
#[cfg(target_os = "macos")]
pub const OTHER_NAME: &str = "Other: snapshots, APFS metadata, unreadable and skipped folders";
#[cfg(not(target_os = "macos"))]
pub const OTHER_NAME: &str =
    "Other: snapshots, subvolumes, filesystem metadata, unreadable and skipped folders";

/// Another volume sharing the scanned volume's container, shown as one entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Sibling {
    pub label: String,
    pub bytes: u64,
}

/// Everything a scan needs, resolved before it starts.
#[derive(Clone, Debug)]
pub struct ScanPlan {
    pub volume: SpaceVolume,
    /// Where the walk starts (the Data volume for the boot volume).
    pub root: PathBuf,
    /// Root-relative paths not entered.
    pub skip: HashSet<PathBuf>,
    /// Devices the walk enters besides the root's: on Linux, the btrfs subvolumes
    /// mounted below it (`linux::space`). A folder on any other device is not entered.
    pub devices: Vec<u64>,
    /// Labels of the skipped locations, for the report.
    pub skipped: Vec<String>,
    pub siblings: Vec<Sibling>,
    /// The total the root is drawn against: the volume's (container's) used bytes.
    pub used_bytes: u64,
    /// Root-relative paths macOS firmlinks into "/" (boot Data volume, `/usr/share/firmlinks`),
    /// so Reveal shows "/Users/..." rather than "/System/Volumes/Data/Users/...".
    pub firmlinked: Vec<PathBuf>,
    pub threads: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("{0} is not a volume Kelvo can scan")]
    UnknownVolume(String),
    #[error("couldn't start scanning: {0}")]
    Start(std::io::Error),
}

/// Live counters for the status line.
pub struct Progress {
    pub phase: SpacePhase,
    pub elapsed_ms: f64,
    pub items: u64,
    pub measured: u64,
    pub current: Option<String>,
}

/// One running or finished scan.
pub struct Scan {
    plan: ScanPlan,
    walk: Arc<walk::Walk>,
    started: Instant,
    finished: std::sync::Mutex<Option<(SpacePhase, Instant)>>,
}

impl Scan {
    /// Starts the walk on `plan.threads` threads. `lister` builds one lister per worker and
    /// runs once on each worker thread first (QoS, I/O policy).
    pub fn start<L: Lister + 'static>(
        plan: ScanPlan,
        lister: impl Fn() -> L + Send + Sync + 'static,
        on_done: impl FnOnce() + Send + 'static,
    ) -> Result<Arc<Self>, ScanError> {
        let dev = device_of(&plan.root).map_err(ScanError::Start)?;
        let walk = Arc::new(walk::Walk::new(
            plan.root.clone(),
            dev,
            plan.skip.clone(),
            &plan.volume.name,
            plan.used_bytes / MIN_KEEP_DIVISOR,
        ));
        let scan = Arc::new(Self {
            walk: Arc::clone(&walk),
            started: Instant::now(),
            finished: std::sync::Mutex::new(None),
            plan,
        });
        let threads = scan.plan.threads.max(1);
        let lister = Arc::new(lister);
        let coordinator = Arc::clone(&scan);
        std::thread::Builder::new()
            .name("disk-space".into())
            .spawn(move || {
                std::thread::scope(|s| {
                    for _ in 0..threads {
                        let (walk, lister) = (&walk, Arc::clone(&lister));
                        s.spawn(move || walk.work(&mut lister()));
                    }
                });
                let phase = if coordinator.walk.cancelled.load(Ordering::Relaxed) {
                    SpacePhase::Cancelled
                } else {
                    SpacePhase::Done
                };
                *coordinator.finished.lock_ok() = Some((phase, Instant::now()));
                on_done();
            })
            .map_err(ScanError::Start)?;
        Ok(scan)
    }

    /// Nodes the tree holds; for memory checks.
    pub fn retained_nodes(&self) -> usize {
        self.walk.tree.lock_ok().len()
    }

    pub fn plan(&self) -> &ScanPlan {
        &self.plan
    }

    pub fn cancel(&self) {
        self.walk.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn phase(&self) -> SpacePhase {
        self.finished
            .lock_ok()
            .map_or(SpacePhase::Scanning, |(phase, _)| phase)
    }

    pub fn progress(&self) -> Progress {
        let finished = *self.finished.lock_ok();
        let end = finished.map_or_else(Instant::now, |(_, at)| at);
        Progress {
            phase: finished.map_or(SpacePhase::Scanning, |(p, _)| p),
            elapsed_ms: end.duration_since(self.started).as_secs_f64() * 1000.0,
            items: self.walk.items.load(Ordering::Relaxed),
            measured: self.walk.measured.load(Ordering::Relaxed),
            current: finished.is_none().then(|| {
                let cur = self.walk.current.lock_ok();
                display(cur.strip_prefix(&self.plan.root).unwrap_or(&cur))
            }),
        }
    }

    /// A folder's breakdown; `None` for the root. At the root, other volumes in the
    /// container and (once finished) the unattributed remainder are entries of their own.
    pub fn subtree(&self, id: Option<u32>) -> Option<SpaceSubtree> {
        let tree = self.walk.tree.lock_ok();
        let id = id.unwrap_or(0);
        let node = tree.get(id)?;
        let mut folder = node.view(id);
        let mut children = tree.breakdown(id);
        let path = tree
            .crumbs(id)
            .into_iter()
            .map(|(id, name)| SpaceCrumb { id, name })
            .collect();
        let at_root = id == 0;
        let total = if at_root {
            self.plan.used_bytes as f64
        } else {
            folder.bytes
        };
        if at_root {
            children.extend(self.sibling_nodes());
            if node.done
                && let Some(other) = self.other_bytes(node.bytes)
            {
                children.push(synthetic(OTHER_NAME, SpaceKind::Other, other as f64));
            }
            children.sort_by(|a, b| b.bytes.total_cmp(&a.bytes));
            folder.bytes = total;
        }
        let unmeasured = (!node.done).then(|| {
            let attributed: f64 = children.iter().filter(|c| c.done).map(|c| c.bytes).sum();
            (total - attributed).max(0.0)
        });
        Some(SpaceSubtree {
            folder,
            path,
            children,
            unmeasured_bytes: unmeasured,
            total_bytes: total,
        })
    }

    fn sibling_nodes(&self) -> impl Iterator<Item = SpaceNode> + '_ {
        self.plan
            .siblings
            .iter()
            .map(|s| synthetic(&s.label, SpaceKind::Volume, s.bytes as f64))
    }

    fn siblings_bytes(&self) -> u64 {
        self.plan.siblings.iter().map(|s| s.bytes).sum()
    }

    /// Used bytes not attributed to files or sibling volumes; `None` when files exceed
    /// them (clones counted once per copy).
    fn other_bytes(&self, files: u64) -> Option<u64> {
        self.plan
            .used_bytes
            .checked_sub(files.saturating_add(self.siblings_bytes()))
    }

    pub fn largest_files(&self) -> Vec<SpaceFile> {
        self.walk
            .largest()
            .into_iter()
            .enumerate()
            .map(|(i, (bytes, path))| {
                let rel = path.strip_prefix(&self.plan.root).unwrap_or(&path);
                SpaceFile {
                    index: i as u32,
                    name: rel
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    parent: rel.parent().map(display).unwrap_or_default(),
                    bytes: bytes as f64,
                }
            })
            .collect()
    }

    /// The path to show in Finder, firmlinked where macOS shows it that way.
    pub fn reveal_path(&self, target: SpaceTarget) -> Option<PathBuf> {
        let rel: PathBuf = match target {
            SpaceTarget::Node { id } => self.walk.tree.lock_ok().rel_path(id)?.iter().collect(),
            SpaceTarget::LargestFile { index } => {
                let (_, path) = self.walk.largest().into_iter().nth(index as usize)?;
                path.strip_prefix(&self.plan.root).ok()?.to_path_buf()
            }
        };
        if self
            .plan
            .firmlinked
            .iter()
            .any(|prefix| rel.starts_with(prefix))
        {
            Some(PathBuf::from("/").join(rel))
        } else {
            Some(self.plan.root.join(rel))
        }
    }

    /// The summary of a finished scan.
    pub fn report(&self, id: u32, finished_ms: f64) -> Option<SpaceReport> {
        let (phase, at) = (*self.finished.lock_ok())?;
        if phase != SpacePhase::Done {
            return None;
        }
        let (files, truncated) = {
            let tree = self.walk.tree.lock_ok();
            (tree.get(0).map_or(0, |n| n.bytes), tree.truncated)
        };
        let (unreadable, paths) = self.walk.unreadable.lock_ok().clone();
        Some(SpaceReport {
            id,
            volume: self.plan.volume.clone(),
            finished_ms,
            duration_ms: at.duration_since(self.started).as_secs_f64() * 1000.0,
            items: self.walk.items.load(Ordering::Relaxed) as f64,
            used_bytes: self.plan.used_bytes as f64,
            files_bytes: files as f64,
            volumes_bytes: self.siblings_bytes() as f64,
            other_bytes: self.other_bytes(files).map(|b| b as f64),
            unreadable,
            unreadable_paths: paths.iter().map(|p| display(p)).collect(),
            skipped: self.plan.skipped.clone(),
            truncated,
        })
    }
}

fn synthetic(name: &str, kind: SpaceKind, bytes: f64) -> SpaceNode {
    SpaceNode {
        id: None,
        name: name.to_owned(),
        kind,
        bytes,
        items: 0.0,
        done: true,
        openable: false,
        children: Vec::new(),
    }
}

fn display(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(unix)]
fn device_of(path: &std::path::Path) -> std::io::Result<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(std::fs::symlink_metadata(path)?.dev())
}

//! The parallel walk: a shared stack of folders that any worker takes from, so one
//! dominant folder (a home folder) spreads across every worker. Metadata only: no file is
//! opened, symlinks are not followed, and the walk stays on the root's device.
use std::collections::{BinaryHeap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};

use kelvo_schema::lock::LockExt;

use super::tree::{FileEntry, NodeId, Tree};

/// Largest files a report lists.
pub const LARGEST_FILES: usize = 50;
/// Unreadable folders a report names.
pub const UNREADABLE_PATHS: usize = 50;

/// One folder's entries. Reused across listings to keep allocations down.
#[derive(Default)]
pub struct Listing {
    pub dirs: Vec<Box<str>>,
    pub files: Vec<ListedFile>,
}

pub struct ListedFile {
    pub name: Box<str>,
    /// Allocated bytes.
    pub bytes: u64,
    /// The inode, for a file with more than one hard link (counted once).
    pub linked: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ListError {
    /// Permission denied, a privacy refusal, or a folder that vanished.
    Unreadable,
    /// A mount point: another volume, not walked.
    OtherDevice,
}

/// Reads one folder. Implementations are per platform; `dev` is the root's device.
pub trait Lister: Send {
    fn list(&mut self, dir: &Path, dev: u64, out: &mut Listing) -> Result<(), ListError>;
}

struct Job {
    id: NodeId,
    path: PathBuf,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Largest {
    pub bytes: u64,
    pub path: PathBuf,
}

pub(crate) struct Walk {
    pub root: PathBuf,
    pub dev: u64,
    /// Root-relative paths not entered (privacy-protected locations).
    pub skip: HashSet<PathBuf>,
    pub tree: Mutex<Tree>,
    queue: Mutex<(Vec<Job>, usize)>,
    wake: Condvar,
    pub cancelled: AtomicBool,
    pub items: AtomicU64,
    pub measured: AtomicU64,
    pub current: Mutex<PathBuf>,
    links: Mutex<HashSet<u64>>,
    /// Min-heap of the largest files, by `Reverse`.
    largest: Mutex<BinaryHeap<std::cmp::Reverse<Largest>>>,
    largest_floor: AtomicU64,
    pub unreadable: Mutex<(u32, Vec<PathBuf>)>,
}

impl Walk {
    pub fn new(
        root: PathBuf,
        dev: u64,
        skip: HashSet<PathBuf>,
        name: &str,
        min_bytes: u64,
    ) -> Self {
        let (mut tree, root_id) = Tree::with_root(name);
        tree.min_bytes = min_bytes;
        Self {
            queue: Mutex::new((
                vec![Job {
                    id: root_id,
                    path: root.clone(),
                }],
                0,
            )),
            root,
            dev,
            skip,
            tree: Mutex::new(tree),
            wake: Condvar::new(),
            cancelled: AtomicBool::new(false),
            items: AtomicU64::new(0),
            measured: AtomicU64::new(0),
            current: Mutex::new(PathBuf::new()),
            links: Mutex::new(HashSet::new()),
            largest: Mutex::new(BinaryHeap::new()),
            largest_floor: AtomicU64::new(0),
            unreadable: Mutex::new((0, Vec::new())),
        }
    }

    /// Runs one worker until the stack is empty and no worker holds a job, or until
    /// cancelled.
    pub fn work(&self, lister: &mut dyn Lister) {
        let mut listing = Listing::default();
        while let Some(job) = self.take() {
            if !self.cancelled.load(Ordering::Relaxed) {
                self.visit(lister, &job, &mut listing);
            }
            self.release();
        }
    }

    fn take(&self) -> Option<Job> {
        let mut q = self.queue.lock_ok();
        loop {
            if self.cancelled.load(Ordering::Relaxed) {
                q.0.clear();
            }
            if let Some(job) = q.0.pop() {
                q.1 += 1;
                return Some(job);
            }
            if q.1 == 0 {
                self.wake.notify_all();
                return None;
            }
            q = self
                .wake
                .wait(q)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn release(&self) {
        let mut q = self.queue.lock_ok();
        q.1 = q.1.saturating_sub(1);
        self.wake.notify_all();
    }

    fn rel<'a>(&self, path: &'a Path) -> &'a Path {
        path.strip_prefix(&self.root).unwrap_or(path)
    }

    fn visit(&self, lister: &mut dyn Lister, job: &Job, listing: &mut Listing) {
        listing.dirs.clear();
        listing.files.clear();
        self.current.lock_ok().clone_from(&job.path);
        match lister.list(&job.path, self.dev, listing) {
            Ok(()) => {}
            Err(e) => {
                if e == ListError::Unreadable {
                    let mut u = self.unreadable.lock_ok();
                    u.0 += 1;
                    if u.1.len() < UNREADABLE_PATHS {
                        u.1.push(self.rel(&job.path).to_path_buf());
                    }
                }
                self.tree.lock_ok().finish_empty(job.id);
                return;
            }
        }
        let dirs: Vec<Box<str>> = listing
            .dirs
            .drain(..)
            .filter(|d| !self.skip.contains(&self.rel(&job.path).join(&**d)))
            .collect();
        let files = self.count_files(&job.path, listing);
        let paths: Vec<PathBuf> = dirs.iter().map(|d| job.path.join(&**d)).collect();
        let ids = self.tree.lock_ok().record_listing(job.id, dirs, files);
        if !ids.is_empty() {
            let mut q = self.queue.lock_ok();
            q.0.extend(
                ids.into_iter()
                    .zip(paths)
                    .map(|(id, path)| Job { id, path }),
            );
            self.wake.notify_all();
        }
    }

    /// Hard links count once; tracks the largest files; returns entries for the tree.
    fn count_files(&self, dir: &Path, listing: &mut Listing) -> Vec<FileEntry> {
        let mut out = Vec::with_capacity(listing.files.len());
        for f in listing.files.drain(..) {
            if let Some(inode) = f.linked
                && !self.links.lock_ok().insert(inode)
            {
                continue;
            }
            self.items.fetch_add(1, Ordering::Relaxed);
            self.measured.fetch_add(f.bytes, Ordering::Relaxed);
            if f.bytes > self.largest_floor.load(Ordering::Relaxed) {
                self.offer_largest(f.bytes, dir.join(&*f.name));
            }
            out.push(FileEntry {
                name: f.name,
                bytes: f.bytes,
            });
        }
        out
    }

    fn offer_largest(&self, bytes: u64, path: PathBuf) {
        let mut heap = self.largest.lock_ok();
        heap.push(std::cmp::Reverse(Largest { bytes, path }));
        if heap.len() > LARGEST_FILES {
            heap.pop();
        }
        if heap.len() == LARGEST_FILES
            && let Some(min) = heap.peek()
        {
            self.largest_floor.store(min.0.bytes, Ordering::Relaxed);
        }
    }

    /// The largest files, largest first.
    pub fn largest(&self) -> Vec<(u64, PathBuf)> {
        let mut out: Vec<_> = self
            .largest
            .lock_ok()
            .iter()
            .map(|r| (r.0.bytes, r.0.path.clone()))
            .collect();
        out.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        out
    }
}

//! Disk space report lifecycle (plan/disk-space-report.md): one scan at a time, one
//! finished report kept, both in memory only. The shell cancels the scan when the dashboard
//! hides and drops the report when the dashboard is destroyed.
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use kelvo_collect::disk_space::{Scan, ScanError, ScanPlan};
use kelvo_schema::disk_space::{
    SpaceFile, SpacePhase, SpaceReport, SpaceScanStatus, SpaceSubtree, SpaceTarget, SpaceVolume,
};
use kelvo_schema::lock::LockExt;

#[derive(Debug, thiserror::Error)]
pub enum SpaceError {
    #[error("a scan is already running")]
    Busy,
    #[error("this report is no longer available; scan again")]
    NotFound,
    #[error("{0}")]
    Scan(#[from] ScanError),
}

struct Entry {
    id: u32,
    scan: Arc<Scan>,
    finished_ms: f64,
}

#[derive(Default)]
struct Inner {
    next_id: u32,
    running: Option<Entry>,
    report: Option<Entry>,
    /// How the last scan that did not produce a report ended.
    ended: Option<(SpacePhase, Option<String>, String)>,
}

#[derive(Default)]
pub struct SpaceService {
    inner: Mutex<Inner>,
    /// Set by [`SpaceService::folder`]: the one folder this service offers as a volume.
    folder: Option<PathBuf>,
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

impl SpaceService {
    pub fn new() -> Self {
        Self::default()
    }

    /// A service whose only volume is `root`, scanned as a volume of its own: for tests
    /// of callers outside this crate, so they never walk the host's disks.
    pub fn folder(root: PathBuf) -> Self {
        Self {
            folder: Some(root),
            ..Self::default()
        }
    }

    /// Volumes the host can scan: what Finder shows on a Mac, local filesystems on Linux.
    pub fn volumes(&self) -> Vec<SpaceVolume> {
        match &self.folder {
            Some(root) => vec![folder_volume(root)],
            None => platform::volumes(),
        }
    }

    pub fn start(&self, mount: &str, include: &[String]) -> Result<SpaceScanStatus, SpaceError> {
        let plan = match &self.folder {
            Some(root) if folder_volume(root).mount == mount => folder_plan(root),
            Some(_) => return Err(ScanError::UnknownVolume(mount.to_owned()).into()),
            None => platform::plan(mount, include)?,
        };
        self.start_plan(plan)
    }

    fn start_plan(&self, plan: ScanPlan) -> Result<SpaceScanStatus, SpaceError> {
        let mut inner = self.inner.lock_ok();
        settle(&mut inner);
        if inner.running.is_some() {
            return Err(SpaceError::Busy);
        }
        let volume = plan.volume.mount.clone();
        let scan = match platform::start(plan) {
            Ok(scan) => scan,
            Err(e) => {
                inner.ended = Some((SpacePhase::Failed, Some(e.to_string()), volume));
                return Err(e.into());
            }
        };
        inner.next_id = inner.next_id.wrapping_add(1);
        inner.running = Some(Entry {
            id: inner.next_id,
            scan,
            finished_ms: 0.0,
        });
        inner.ended = None;
        Ok(status_of(&inner))
    }

    pub fn status(&self) -> SpaceScanStatus {
        let mut inner = self.inner.lock_ok();
        settle(&mut inner);
        status_of(&inner)
    }

    pub fn cancel(&self) {
        if let Some(entry) = &self.inner.lock_ok().running {
            entry.scan.cancel();
        }
    }

    /// Cancels any scan and forgets the report (the dashboard was destroyed).
    pub fn clear(&self) {
        let mut inner = self.inner.lock_ok();
        if let Some(entry) = inner.running.take() {
            entry.scan.cancel();
        }
        inner.report = None;
        inner.ended = None;
    }

    pub fn report(&self) -> Option<SpaceReport> {
        let mut inner = self.inner.lock_ok();
        settle(&mut inner);
        let entry = inner.report.as_ref()?;
        entry.scan.report(entry.id, entry.finished_ms)
    }

    fn with_scan<T>(&self, id: u32, f: impl FnOnce(&Scan) -> Option<T>) -> Result<T, SpaceError> {
        let mut inner = self.inner.lock_ok();
        settle(&mut inner);
        let scan = [&inner.running, &inner.report]
            .into_iter()
            .flatten()
            .find(|e| e.id == id)
            .map(|e| Arc::clone(&e.scan))
            .ok_or(SpaceError::NotFound)?;
        drop(inner);
        f(&scan).ok_or(SpaceError::NotFound)
    }

    /// A folder's breakdown in the running scan or the report; `None` is the volume root.
    pub fn subtree(&self, report: u32, node: Option<u32>) -> Result<SpaceSubtree, SpaceError> {
        self.with_scan(report, |scan| scan.subtree(node))
    }

    pub fn largest_files(&self, report: u32) -> Result<Vec<SpaceFile>, SpaceError> {
        self.with_scan(report, |scan| Some(scan.largest_files()))
    }

    pub fn reveal_path(&self, report: u32, target: SpaceTarget) -> Result<PathBuf, SpaceError> {
        self.with_scan(report, |scan| scan.reveal_path(target))
    }
}

/// Moves a scan that has stopped to the report slot (done) or records how it ended.
fn settle(inner: &mut Inner) {
    let Some(entry) = inner.running.as_ref() else {
        return;
    };
    let phase = entry.scan.phase();
    if phase == SpacePhase::Scanning {
        return;
    }
    let Some(mut entry) = inner.running.take() else {
        return;
    };
    let volume = entry.scan.plan().volume.mount.clone();
    if phase == SpacePhase::Done && entry.scan.plan().root.exists() {
        entry.finished_ms = now_ms();
        inner.report = Some(entry);
        inner.ended = None;
    } else if phase == SpacePhase::Done {
        let name = &entry.scan.plan().volume.name;
        let message = format!("Couldn't finish scanning {name}: the volume was disconnected.");
        inner.ended = Some((SpacePhase::Failed, Some(message), volume));
    } else {
        inner.ended = Some((phase, None, volume));
    }
}

fn status_of(inner: &Inner) -> SpaceScanStatus {
    let idle = SpaceScanStatus {
        phase: SpacePhase::Idle,
        report_id: None,
        volume: None,
        elapsed_ms: 0.0,
        items: 0.0,
        measured_bytes: 0.0,
        used_bytes: 0.0,
        current: None,
        error: None,
    };
    let from = |entry: &Entry| {
        let p = entry.scan.progress();
        SpaceScanStatus {
            phase: p.phase,
            report_id: Some(entry.id),
            volume: Some(entry.scan.plan().volume.mount.clone()),
            elapsed_ms: p.elapsed_ms,
            items: p.items as f64,
            measured_bytes: p.measured as f64,
            used_bytes: entry.scan.plan().used_bytes as f64,
            current: p.current,
            error: None,
        }
    };
    if let Some(entry) = &inner.running {
        return from(entry);
    }
    if let Some((phase, error, volume)) = &inner.ended {
        return SpaceScanStatus {
            phase: *phase,
            volume: Some(volume.clone()),
            error: error.clone(),
            ..idle
        };
    }
    inner.report.as_ref().map_or(idle, from)
}

fn folder_volume(root: &std::path::Path) -> SpaceVolume {
    let mount = root.to_string_lossy().into_owned();
    SpaceVolume {
        name: mount.clone(),
        mount,
        is_boot: false,
        used_bytes: 0.0,
        total_bytes: 0.0,
        protected: Vec::new(),
    }
}

fn folder_plan(root: &std::path::Path) -> ScanPlan {
    ScanPlan {
        volume: folder_volume(root),
        root: root.to_path_buf(),
        skip: Default::default(),
        devices: Vec::new(),
        skipped: Vec::new(),
        siblings: Vec::new(),
        used_bytes: 0,
        firmlinked: Vec::new(),
        threads: 1,
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use kelvo_collect::macos::space::{self, BulkLister};

    pub fn volumes() -> Vec<SpaceVolume> {
        space::volumes()
    }
    pub fn plan(mount: &str, include: &[String]) -> Result<ScanPlan, ScanError> {
        space::plan(mount, include)
    }
    pub fn start(plan: ScanPlan) -> Result<Arc<Scan>, ScanError> {
        Scan::start(plan, BulkLister::for_this_thread, || {})
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use kelvo_collect::disk_space::StdLister;
    use kelvo_collect::linux::space;

    pub fn volumes() -> Vec<SpaceVolume> {
        space::volumes()
    }
    pub fn plan(mount: &str, include: &[String]) -> Result<ScanPlan, ScanError> {
        space::plan(mount, include)
    }
    /// Enters the btrfs subvolumes mounted below the root (`linux::space`).
    pub fn start(plan: ScanPlan) -> Result<Arc<Scan>, ScanError> {
        let devices = plan.devices.clone();
        let lister = move || StdLister {
            devices: devices.clone(),
        };
        Scan::start(plan, lister, || {})
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;
    use kelvo_collect::disk_space::StdLister;

    pub fn volumes() -> Vec<SpaceVolume> {
        Vec::new()
    }
    pub fn plan(mount: &str, _include: &[String]) -> Result<ScanPlan, ScanError> {
        Err(ScanError::UnknownVolume(mount.to_owned()))
    }
    pub fn start(plan: ScanPlan) -> Result<Arc<Scan>, ScanError> {
        Scan::start(plan, StdLister::default, || {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    fn plan(root: &std::path::Path) -> ScanPlan {
        ScanPlan {
            volume: SpaceVolume {
                mount: "/test".into(),
                name: "Test".into(),
                is_boot: false,
                used_bytes: 0.0,
                total_bytes: 0.0,
                protected: Vec::new(),
            },
            root: root.to_path_buf(),
            skip: HashSet::new(),
            devices: Vec::new(),
            skipped: Vec::new(),
            siblings: Vec::new(),
            used_bytes: 1 << 30,
            firmlinked: Vec::new(),
            threads: 2,
        }
    }

    fn wait(service: &SpaceService, phase: SpacePhase) -> SpaceScanStatus {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let status = service.status();
            if status.phase == phase || Instant::now() > deadline {
                return status;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn tree(files: usize) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..files {
            let path = dir.path().join(format!("d{}/e{i}/f.bin", i % 7));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, vec![1u8; 5000]).unwrap();
        }
        dir
    }

    #[test]
    fn a_finished_scan_becomes_the_report() {
        let dir = tree(20);
        let service = SpaceService::new();
        let started = service.start_plan(plan(dir.path())).unwrap();
        let id = started.report_id.unwrap();
        let done = wait(&service, SpacePhase::Done);
        assert_eq!(done.phase, SpacePhase::Done);
        assert_eq!(done.items, 20.0);
        let report = service.report().unwrap();
        assert_eq!(report.id, id);
        assert!(report.finished_ms > 0.0);
        let root = service.subtree(id, None).unwrap();
        assert_eq!(
            root.children
                .iter()
                .filter(|c| c.name.starts_with('d'))
                .count(),
            7
        );
        assert!(matches!(
            service.subtree(id + 1, None),
            Err(SpaceError::NotFound)
        ));
    }

    #[test]
    fn one_scan_at_a_time_and_cancel_keeps_the_previous_report() {
        let dir = tree(20);
        let service = SpaceService::new();
        let first = service.start_plan(plan(dir.path())).unwrap().report_id;
        wait(&service, SpacePhase::Done);
        let big = tree(3000);
        let second = service.start_plan(plan(big.path())).unwrap();
        assert!(matches!(
            service.start_plan(plan(dir.path())),
            Err(SpaceError::Busy)
        ));
        service.cancel();
        let ended = wait(&service, SpacePhase::Cancelled);
        assert_eq!(ended.phase, SpacePhase::Cancelled);
        assert_ne!(second.report_id, first);
        assert_eq!(service.report().map(|r| Some(r.id)), Some(first));
        service.clear();
        assert!(service.report().is_none());
        assert_eq!(service.status().phase, SpacePhase::Idle);
    }

    #[test]
    fn a_vanished_root_fails_the_scan() {
        let dir = tree(5);
        let root = dir.path().join("gone");
        std::fs::create_dir(&root).unwrap();
        let service = SpaceService::new();
        service.start_plan(plan(&root)).unwrap();
        std::fs::remove_dir(&root).unwrap();
        let status = wait(&service, SpacePhase::Failed);
        assert_eq!(status.phase, SpacePhase::Failed);
        assert!(status.error.unwrap().contains("disconnected"));
        assert!(service.report().is_none());
    }
}

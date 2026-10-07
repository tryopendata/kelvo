//! Local history in SQLite: series and layout interning, tiered rollups, explicit gaps,
//! sync cursors, pruning, and the single writer.
//!
//! Depends on `kelvo-schema` only. Knows nothing about collectors, the webview or the
//! proto: the cursor API speaks [`CursorPage`], which the sync driver maps to and from the
//! proto `SyncPage` one field at a time.
//!
//! # Shape
//!
//! - [`Store::open`] takes an exclusive lock on `<file>.lock` (a second opener gets
//!   [`StoreError::Locked`]), creates or migrates the file and starts the writer thread,
//!   the only holder of a write connection (architecture.md infra 8). The database, its
//!   WAL and shared-memory files and the lock are owner-only (0600, D-074); the
//!   [`create_private_dir`] and [`write_private`] helpers do the same for callers' files.
//! - [`Writer`] is a cloneable handle to that thread. Per-tick writes are queued; the
//!   batch commits every 5 minutes and on [`Writer::flush`] (D-070).
//! - [`Reader`] is a read-only connection for queries and the cursor read API. Open one
//!   per thread.
//!
//! Pruning keeps the file under a byte cap as well as retention, and [`LowDiskGuard`]
//! stops the 10 s tier when the volume runs low (D-057).
//!
//! Every persisted row carries a `seq` from `meta.next_seq`; cursors are
//! `(db_instance_uuid, seq)`. Gaps are explicit rows and are never inferred from missing
//! buckets. Store-specific choices beyond architecture.md are in D-041.

mod blob;
mod db;
mod disk;
mod error;
mod lock;
mod perms;
mod reader;
mod rolldown;
mod types;
mod writer;

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use uuid::Uuid;

pub use db::SCHEMA_VERSION;
pub use disk::{
    FreeSpace, LOW_DISK_BYTES, LOW_DISK_PERCENT, LowDiskGuard, StatVfs, VolumeSpace,
    low_disk_threshold, resume_threshold,
};
pub use error::{Result, StoreError};
pub use lock::lock_path;
pub use perms::{
    PRIVATE_DIR_MODE, PRIVATE_FILE_MODE, create_private_dir, open_private, restrict, write_private,
};
pub use reader::{
    BATTERY_CHARGE, BATTERY_CHARGING, BatteryCell, ExportQuery, ExportSummary, HistoryGrowth,
    MIN_MEASURED_MS, MetricStats, RangeStats, Reader, fill_battery_hours, history_recent,
    net_by_app_recent, range_stats,
};
pub use rolldown::NET_TOP_APPS;
/// The `rusqlite` behind [`StoreError::Sqlite`], so callers can match its error codes
/// without depending on a version of their own.
pub use rusqlite;
pub use types::{
    BucketRow, CapTrim, CursorPage, CursorRead, FILL_MINUTES, FILL_ROLLED, FillMeasurement,
    HistoryQuery, HistoryResult, IngestReport, NetApp, NetBucket, NetByApp, NetSpan, PageEvent,
    PageGap, PageLayout, PageRow, Point, ProcResolution, ProcRow, ProcessesAt, PruneReport,
    Retention, SeriesPoints, SessionStart, TierChoice,
};
pub use writer::NET_NEW_NAMES_PER_HOUR;
#[cfg(feature = "write-stats")]
pub use writer::WriteStats;
pub use writer::Writer;

/// How often the writer commits its batch when nobody asks it to flush. A crash loses
/// at most this much persisted history; the engine also flushes before sleep, on wake,
/// on a store swap and at shutdown. Five minutes trades that window for fewer WAL
/// commits and disk writes (D-070).
pub const DEFAULT_COMMIT_INTERVAL: Duration = Duration::from_secs(300);

/// Rows deleted per pruning transaction (architecture.md, Store).
pub const DEFAULT_PRUNE_BATCH: u64 = 5_000;

#[derive(Clone, Debug)]
pub struct StoreConfig {
    /// The database file. `-wal` and `-shm` files sit next to it.
    pub path: PathBuf,
    pub commit_interval: Duration,
    /// Rows deleted per pruning transaction; the writer handles queued work between
    /// batches (D-064). Tests set it small to get many batches from few rows.
    pub prune_batch: u64,
}

impl StoreConfig {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            commit_interval: DEFAULT_COMMIT_INTERVAL,
            prune_batch: DEFAULT_PRUNE_BATCH,
        }
    }
}

/// An open history database: its writer thread plus the means to open readers.
///
/// Exactly one `Store` may be open on a file (architecture.md infra 8). [`Store::open`]
/// enforces it across processes with an exclusive lock on `<file>.lock`, held until the
/// writer has stopped. Dropping it commits, checkpoints the WAL and stops the writer;
/// [`Store::close`] does the same and reports errors.
pub struct Store {
    path: PathBuf,
    epoch: Uuid,
    writer: Writer,
    thread: Option<JoinHandle<()>>,
    /// Declared last so it is released after `Drop` has stopped the writer.
    _lock: lock::WriterLock,
}

impl Store {
    /// Opens or creates the database. [`StoreError::Locked`] when another `Store` (in this
    /// or another process) has it open; [`StoreError::TooNew`] when a newer build wrote it.
    pub fn open(config: StoreConfig) -> Result<Store> {
        let lock = lock::WriterLock::acquire(&config.path)?;
        private_files(&config.path)?;
        let (conn, epoch) = db::open_writer(&config.path, wall_ms())?;
        let (writer, thread) = writer::spawn(
            conn,
            config.path.clone(),
            config.commit_interval,
            config.prune_batch.max(1),
        )?;
        Ok(Store {
            path: config.path,
            epoch,
            writer,
            thread: Some(thread),
            _lock: lock,
        })
    }

    /// The `db_instance_uuid`, created with the file: the epoch of every cursor into it.
    pub fn epoch(&self) -> Uuid {
        self.epoch
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn writer(&self) -> Writer {
        self.writer.clone()
    }

    /// A new read-only connection.
    pub fn reader(&self) -> Result<Reader> {
        let (conn, epoch) = db::open_reader(&self.path)?;
        Ok(Reader::new(conn, epoch))
    }

    /// Bytes on disk: the database plus its WAL and shared-memory files.
    pub fn size_on_disk(&self) -> Result<u64> {
        size_on_disk(&self.path)
    }

    /// Commits, checkpoints and stops the writer.
    pub fn close(mut self) -> Result<()> {
        self.stop()
    }

    fn stop(&mut self) -> Result<()> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        let res = self.writer.shutdown();
        if thread.join().is_err() {
            tracing::error!("store writer thread panicked");
        }
        res
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Err(e) = self.stop() {
            tracing::error!("store shutdown failed: {e}");
        }
    }
}

/// Moves the database at `path` and its `-wal` and `-shm` files aside, to
/// `<stem>-reset-<now_ms>.<ext>` beside it, so the next [`Store::open`] starts a fresh
/// file with a new epoch. The moved copy stays readable for diagnosis. Takes the writer
/// lock while it moves, so it fails with [`StoreError::Locked`] while a `Store` is open on
/// the file. Returns where the database went, or `None` when there was none.
pub fn move_aside(path: &Path, now_ms: i64) -> Result<Option<PathBuf>> {
    let _lock = lock::WriterLock::acquire(path)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "history".into());
    let ext = path
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sqlite".into());
    let moved = path.with_file_name(format!("{stem}-reset-{now_ms}.{ext}"));
    let mut moved_db = None;
    for suffix in ["", "-wal", "-shm"] {
        let mut from = path.as_os_str().to_owned();
        from.push(suffix);
        let mut to = moved.as_os_str().to_owned();
        to.push(suffix);
        match std::fs::rename(&from, &to) {
            Ok(()) if suffix.is_empty() => moved_db = Some(moved.clone()),
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(moved_db)
}

/// Makes the database owner-only before SQLite opens it (D-074): creates it 0600 when
/// it is missing (SQLite takes an empty file as a new database and gives the `-wal` and
/// `-shm` files it creates the database's mode), and tightens an existing database, WAL
/// and shared-memory file that are looser.
fn private_files(path: &Path) -> Result<()> {
    drop(perms::open_private(path)?);
    for suffix in ["-wal", "-shm"] {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        perms::restrict(Path::new(&p))?;
    }
    Ok(())
}

/// Bytes on disk of the database at `path` plus its `-wal` and `-shm` files. Missing
/// files count as zero.
pub fn size_on_disk(path: &Path) -> Result<u64> {
    let mut total = 0;
    for suffix in ["", "-wal", "-shm"] {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        match std::fs::metadata(&p) {
            Ok(m) => total += m.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(total)
}

fn wall_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// A fresh, empty directory under the system temp dir for one test.
#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kelvo-store-{name}-{}-{}",
        std::process::id(),
        Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

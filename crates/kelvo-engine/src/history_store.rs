//! Store lifecycle and generation-tagged read pooling shared by local and mirror history.
//! A read checks out under the inner lock, releases it for the query, and returns
//! only to the same generation. Reset closes under that lock and advances the
//! generation, so old readers never enter the replacement store's pool. The inner
//! lock precedes the health lock; health operations never acquire the inner lock.
use crate::{HistoryHealth, Housekeeping, HousekeepingHandle, Schedule};
use kelvo_schema::{HostRecord, JsSafeInt, lock::LockExt};
use kelvo_store::rusqlite::ErrorCode;
use kelvo_store::{Reader, Retention, Store, StoreConfig, StoreError, Writer};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

pub const DB_FILE: &str = "history.sqlite";

#[derive(Debug, thiserror::Error)]
pub enum HistoryStoreError {
    #[error("history is unavailable")]
    Unavailable {
        reason: Option<HistoryUnavailableReason>,
    },
    #[error("{}", store_error_message(.0))]
    Store(#[from] StoreError),
}

/// Why history is unavailable, for the banner and for whether `reset_history` can help.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryUnavailableReason {
    /// Another Kelvo process (a second copy, or an installed build next to a dev build
    /// sharing its data) has the database open. Quit it; a reset cannot help.
    Locked,
    /// A newer Kelvo wrote the database.
    TooNew {
        #[specta(type = JsSafeInt)]
        found: i64,
        #[specta(type = JsSafeInt)]
        supported: i64,
    },
    /// The database file is damaged.
    Corrupt { message: String },
    /// Anything else: an I/O error opening it, or the local host could not be registered.
    Failed { message: String },
}

impl From<&StoreError> for HistoryUnavailableReason {
    fn from(e: &StoreError) -> Self {
        match e {
            StoreError::Locked { .. } => HistoryUnavailableReason::Locked,
            StoreError::TooNew { found, supported } => HistoryUnavailableReason::TooNew {
                found: *found,
                supported: *supported,
            },
            e if is_corrupt(e) => HistoryUnavailableReason::Corrupt {
                message: e.to_string(),
            },
            e => HistoryUnavailableReason::Failed {
                message: e.to_string(),
            },
        }
    }
}

fn sqlite_code(e: &StoreError) -> Option<ErrorCode> {
    match e {
        StoreError::Sqlite(e) => e.sqlite_error_code(),
        _ => None,
    }
}

fn is_corrupt(e: &StoreError) -> bool {
    matches!(e, StoreError::Corrupt(_) | StoreError::Cbor(_))
        || matches!(
            sqlite_code(e),
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
        )
}

// Keep the command-facing diagnostic text stable while retaining the typed source.
fn store_error_message(error: &StoreError) -> String {
    match error {
        StoreError::UnknownHost(host) => format!("unknown host {host}"),
        StoreError::WriterGone => "history is unavailable".into(),
        StoreError::TooNew { found, supported } => {
            format!("history store schema {found} is newer than this build supports ({supported})")
        }
        e if matches!(e, StoreError::Locked { .. })
            || matches!(
                sqlite_code(e),
                Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
            ) =>
        {
            format!("history store is busy: {e}")
        }
        e if is_corrupt(e) => format!("history store is corrupt: {e}"),
        e => format!("history store: {e}"),
    }
}

/// The open store and its read pool, or why there is none.
struct Inner {
    store: Option<Store>,
    writer: Option<Writer>,
    /// Pooled read connections, each tagged with the `generation` it was opened in, so a
    /// reader checked out before a reset is dropped instead of returned to the pool.
    readers: Vec<(u64, Reader)>,
    /// Bumped whenever the store is replaced or closed.
    generation: u64,
    reason: Option<HistoryUnavailableReason>,
}

impl Inner {
    fn unavailable(reason: Option<HistoryUnavailableReason>) -> Self {
        Self {
            store: None,
            writer: None,
            readers: Vec::new(),
            generation: 0,
            reason,
        }
    }

    fn error(&self) -> HistoryStoreError {
        HistoryStoreError::Unavailable {
            reason: self.reason.clone(),
        }
    }

    /// Closes the store (commit, checkpoint, stop the writer) and records why.
    fn close(&mut self, reason: Option<HistoryUnavailableReason>) {
        self.readers.clear();
        self.writer = None;
        self.generation += 1;
        self.reason = reason;
        if let Some(store) = self.store.take()
            && let Err(e) = store.close()
        {
            tracing::error!("closing the history store: {e}");
        }
    }
}

/// The history store, shared by commands, the pruner and the sources. It can change at
/// runtime: [`HistoryStore::register_host`] gives up on a store it cannot write the local host
/// into, and [`HistoryStore::reset`] replaces the file.
pub struct HistoryStore {
    inner: Arc<Mutex<Inner>>,
    path: PathBuf,
    health: Arc<Mutex<crate::HistoryHealth>>,
}

fn open_store(path: &Path) -> Result<Store, StoreError> {
    let store = Store::open(StoreConfig::new(path))?;
    tracing::info!(path = %path.display(), epoch = %store.epoch(), "history store opened");
    Ok(store)
}

impl HistoryStore {
    /// Opens `dir/history.sqlite`. On failure, logs it and returns a history that is
    /// unavailable with the reason: the app runs live-only and history commands return
    /// `history_unavailable`.
    pub fn open(dir: &Path) -> Self {
        let path = dir.join(DB_FILE);
        let inner = match open_store(&path) {
            Ok(store) => Inner {
                writer: Some(store.writer()),
                store: Some(store),
                readers: Vec::new(),
                generation: 0,
                reason: None,
            },
            Err(e) => {
                tracing::error!(path = %path.display(), "cannot open the history store, running live-only: {e}");
                Inner::unavailable(Some(HistoryUnavailableReason::from(&e)))
            }
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
            path,
            health: Arc::default(),
        }
    }

    pub fn unavailable() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::unavailable(None))),
            path: PathBuf::new(),
            health: Arc::default(),
        }
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock_ok()
    }

    /// Writes `record` into the store and returns the writer a source for it should use.
    /// When the store cannot take the host, history is closed and marked unavailable with
    /// the error as its reason, and `None` sends the source live-only: a broken store must
    /// never keep the app from starting (D-064).
    pub fn register_host(&self, record: &HostRecord) -> Option<Writer> {
        let mut inner = self.inner();
        let writer = inner.writer.clone()?;
        match writer.upsert_host(record.clone()) {
            Ok(()) => Some(writer),
            Err(e) => {
                tracing::error!(host = %record.id, "cannot register the host in the history store, running live-only: {e}");
                let reason = HistoryUnavailableReason::Failed {
                    message: format!("registering this Mac in the history store: {e}"),
                };
                inner.close(Some(reason));
                None
            }
        }
    }

    /// Low-disk pause and byte-cap trim state, as of the last check.
    pub fn health(&self) -> HistoryHealth {
        *self.health.lock_ok()
    }

    /// Remote mirrors use their own pruning policy, but expose the same history
    /// health as the local store. No local-engine pause or global budget is changed.
    pub fn record_mirror_prune(
        &self,
        report: &kelvo_store::PruneReport,
        now_ms: i64,
        retention: &Retention,
    ) {
        let mut health = self.health.lock_ok();
        *health = crate::next_health(*health, Some(report), false, now_ms, retention);
    }

    /// Forgets the trim after the history was cleared. Returns the new health when it
    /// changed, for `history-health-changed`.
    pub fn clear_trim(&self) -> Option<HistoryHealth> {
        let mut h = self.health.lock_ok();
        let next = h.without_trim();
        (next != *h).then(|| {
            *h = next;
            next
        })
    }

    pub fn is_available(&self) -> bool {
        self.inner().writer.is_some()
    }

    /// Why history is unavailable, when it is and the reason is known.
    pub fn unavailable_reason(&self) -> Option<HistoryUnavailableReason> {
        let inner = self.inner();
        inner
            .writer
            .is_none()
            .then(|| inner.reason.clone())
            .flatten()
    }

    pub fn writer(&self) -> Result<Writer, HistoryStoreError> {
        let inner = self.inner();
        inner.writer.clone().ok_or_else(|| inner.error())
    }
}

impl HistoryStore {
    /// Runs `f` on a pooled read connection. Blocking: call it off the main thread.
    pub fn read<T>(
        &self,
        f: impl FnOnce(&mut Reader) -> kelvo_store::Result<T>,
    ) -> Result<T, HistoryStoreError> {
        let (generation, mut reader) = {
            let mut inner = self.inner();
            match inner.readers.pop() {
                Some(pooled) => pooled,
                None => {
                    let store = inner.store.as_ref().ok_or_else(|| inner.error())?;
                    (inner.generation, store.reader()?)
                }
            }
        };
        let out = f(&mut reader);
        if reader.in_transaction() {
            // Its snapshot would go stale in the pool and hold back WAL checkpoints.
            tracing::warn!("dropping a read connection left inside a transaction");
            return Ok(out?);
        }
        let mut inner = self.inner();
        if inner.generation == generation {
            inner.readers.push((generation, reader));
        }
        Ok(out?)
    }

    pub fn size_on_disk(&self) -> Result<u64, HistoryStoreError> {
        let inner = self.inner();
        let store = inner.store.as_ref().ok_or_else(|| inner.error())?;
        Ok(store.size_on_disk()?)
    }

    /// Commits, checkpoints and closes. Call after every source has stopped writing.
    pub fn close(&self) {
        self.inner().close(None);
    }

    /// Closes the store, moves the file (and its WAL) aside as
    /// `history-reset-<now_ms>.sqlite` and opens a fresh one: the way out of a corrupt or
    /// too-new database, and a full reset. Call with every source detached from the
    /// store. Returns the new writer; on failure history stays unavailable with the
    /// reason, and the error says what went wrong.
    pub fn reset(&self, now_ms: i64) -> Result<Writer, HistoryStoreError> {
        let mut inner = self.inner();
        inner.close(None);
        *self.health.lock_ok() = crate::HistoryHealth::default();
        let opened = kelvo_store::move_aside(&self.path, now_ms).and_then(|moved| {
            if let Some(moved) = moved {
                tracing::warn!(to = %moved.display(), "history database moved aside");
            }
            open_store(&self.path)
        });
        match opened {
            Ok(store) => {
                let writer = store.writer();
                inner.writer = Some(writer.clone());
                inner.store = Some(store);
                inner.reason = None;
                Ok(writer)
            }
            Err(e) => {
                tracing::error!("resetting the history store failed: {e}");
                inner.reason = Some(HistoryUnavailableReason::from(&e));
                Err(e.into())
            }
        }
    }
}

impl HistoryStore {
    /// Starts [`Housekeeping`] on this history's file: pruning with the retention
    /// `retention` returns at that moment, the low-disk check, and `on_change` with the new
    /// health whenever it changes. Each round uses whatever store is open then and skips
    /// the round when there is none, so a store that comes back through `reset_history` is
    /// pruned too. Keep the handle: dropping it stops housekeeping.
    pub fn spawn_pruner(
        &self,
        retention: impl Fn() -> Retention + Send + 'static,
        on_change: impl Fn(HistoryHealth) + Send + 'static,
    ) -> std::io::Result<HousekeepingHandle> {
        let inner = Arc::clone(&self.inner);
        Housekeeping {
            path: self.path.clone(),
            schedule: Schedule::default(),
            writer: move || inner.lock_ok().writer.clone(),
            retention,
            health: Arc::clone(&self.health),
            on_change: move |h: crate::HistoryHealth| on_change(h),
        }
        .spawn()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const NOW: i64 = 1_800_000_000_000;
    fn temp_dir(name: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("kelvo-shell-{name}-"))
            .tempdir()
            .unwrap()
    }

    fn local_record() -> HostRecord {
        HostRecord {
            id: kelvo_schema::HostId(uuid::Uuid::from_u128(7)),
            is_local: true,
            display_name: "Mac".into(),
            info: kelvo_schema::HostInfo {
                os: kelvo_schema::OsKind::MacOs,
                os_version: "27.0".into(),
                model: None,
                chip: None,
                chip_known: true,
                cpu_topology: Vec::new(),
                mem_total_bytes: 0,
                boot_time_ms: 0,
                gpu_dvfs_mhz: Vec::new(),
                kernel: None,
                boot_mounts: Vec::new(),
            },
        }
    }

    #[test]
    fn a_store_that_cannot_take_the_host_goes_unavailable_and_reset_recovers_it() {
        let tmp = temp_dir("register-fails");
        let dir = tmp.path();
        drop(HistoryStore::open(dir));
        // The file opens fine but refuses every new host.
        let conn = kelvo_store::rusqlite::Connection::open(dir.join(DB_FILE)).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER refuse_hosts BEFORE INSERT ON hosts
             BEGIN SELECT RAISE(ABORT, 'disk says no'); END;",
        )
        .unwrap();
        drop(conn);

        let history = HistoryStore::open(dir);
        assert!(history.is_available(), "it opened");
        assert!(
            history.register_host(&local_record()).is_none(),
            "live-only"
        );
        assert!(!history.is_available());
        let reason = history.unavailable_reason().unwrap();
        assert!(
            matches!(&reason, HistoryUnavailableReason::Failed { message } if message.contains("disk says no")),
            "{reason:?}"
        );
        assert!(
            matches!(history.read(|r| r.hosts()), Err(HistoryStoreError::Unavailable { reason: Some(actual) }) if actual == reason)
        );
        assert!(history.writer().is_err());

        // The reset moves the file aside and starts over.
        history.reset(NOW).unwrap();
        assert!(history.register_host(&local_record()).is_some());
        assert_eq!(history.read(|r| r.hosts()).unwrap(), vec![local_record()]);
        assert!(dir.join(format!("history-reset-{NOW}.sqlite")).exists());
        history.close();
    }

    #[test]
    fn a_store_held_by_another_process_is_locked_and_reset_refuses() {
        let tmp = temp_dir("locked");
        let dir = tmp.path();
        let other = Store::open(StoreConfig::new(dir.join(DB_FILE))).unwrap();
        let history = HistoryStore::open(dir);
        assert_eq!(
            history.unavailable_reason(),
            Some(HistoryUnavailableReason::Locked)
        );
        assert!(matches!(
            history.reset(NOW),
            Err(HistoryStoreError::Store(StoreError::Locked { .. }))
        ));
        assert!(
            dir.join(DB_FILE).exists(),
            "the other process's file is untouched"
        );
        other.close().unwrap();
        history.reset(NOW).unwrap();
        assert!(history.is_available());
    }

    #[test]
    fn clearing_history_forgets_the_trim_once() {
        let h = HistoryStore::unavailable();
        assert_eq!(h.clear_trim(), None, "nothing to clear");
        *h.health.lock().unwrap() = crate::HistoryHealth {
            low_disk_paused: true,
            trimmed_before_ms: Some(NOW),
            trimmed_limit_bytes: Some(150_000_000),
            cap_met: false,
        };
        let cleared = h.clear_trim().unwrap();
        assert_eq!(cleared.trimmed_before_ms, None);
        assert!(cleared.cap_met && cleared.low_disk_paused);
        assert_eq!(h.health(), cleared);
        assert_eq!(h.clear_trim(), None);
    }
    #[test]
    fn reader_checked_out_across_reset_never_reenters_the_pool() {
        let temp = temp_dir("reader-generation");
        let history = HistoryStore::open(temp.path());
        let old_epoch = history.read(|r| Ok(r.epoch())).unwrap();
        history
            .read(|reader| {
                assert_eq!(reader.epoch(), old_epoch);
                history.reset(NOW).unwrap();
                Ok(())
            })
            .unwrap();
        let new_epoch = history.read(|r| Ok(r.epoch())).unwrap();
        assert_ne!(old_epoch, new_epoch);
        assert_eq!(history.read(|r| Ok(r.epoch())).unwrap(), new_epoch);
    }
}

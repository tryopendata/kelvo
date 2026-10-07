//! The single-writer lock (architecture.md infra 8): an exclusive `flock` on
//! `<database>.lock`, held for as long as a [`crate::Store`] is open. A second process (a
//! second app instance, a dev build pointed at the same file, a v4 agent) fails to open
//! with [`StoreError::Locked`] instead of writing beside the first.
//!
//! The lock is on a separate file, not the database, so moving the database aside
//! ([`crate::move_aside`]) never races the lock, and SQLite's own locking stays untouched.
//! `flock` locks belong to the open file description: the kernel drops them when the
//! process exits, so a crash never leaves a stale lock.

use std::fs::File;
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};
use rustix::io::Errno;

use crate::error::{Result, StoreError};

/// `history.sqlite` -> `history.sqlite.lock`.
pub fn lock_path(db_path: &Path) -> PathBuf {
    let mut p = db_path.as_os_str().to_owned();
    p.push(".lock");
    PathBuf::from(p)
}

/// Held exclusive lock; released when dropped.
#[derive(Debug)]
pub(crate) struct WriterLock {
    _file: File,
}

impl WriterLock {
    /// Takes the lock for the database at `db_path` without waiting.
    pub(crate) fn acquire(db_path: &Path) -> Result<WriterLock> {
        let path = lock_path(db_path);
        let file = crate::perms::open_private(&path)?;
        match flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => Ok(WriterLock { _file: file }),
            Err(e) if e == Errno::WOULDBLOCK => Err(StoreError::Locked {
                path: db_path.to_path_buf(),
            }),
            Err(e) => Err(StoreError::Io(e.into())),
        }
    }
}

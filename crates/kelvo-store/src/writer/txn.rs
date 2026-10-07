//! The batch transaction: savepoints per operation, commits, lost-span gaps, yielding
//! between batches of a long operation, WAL checkpoints and `seq` allocation.

#[cfg(feature = "write-stats")]
use std::sync::atomic::Ordering;
use std::time::Instant;

use kelvo_schema::{Gap, GapReason, HostId};

#[cfg(feature = "write-stats")]
use super::WriteStats;
use super::{Op, State, widen};
use crate::db;
use crate::error::{Result, StoreError};

impl State {
    // --- transactions -----------------------------------------------------------------

    /// Runs `f` inside the batch transaction, in its own savepoint. On error the
    /// savepoint is rolled back, `next_seq` restored and the intern caches dropped (they
    /// may name rows the rollback removed).
    pub(super) fn apply<T>(&mut self, f: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
        if !self.in_txn {
            self.conn.execute_batch("BEGIN")?;
            self.in_txn = true;
            self.deadline = Some(Instant::now() + self.commit_interval);
        }
        let seq_before = self.next_seq;
        self.conn.execute_batch("SAVEPOINT op")?;
        let res = f(self).and_then(|v| {
            if self.next_seq != seq_before {
                self.conn.execute(
                    "UPDATE meta SET value = ?1 WHERE key = 'next_seq'",
                    [self.next_seq],
                )?;
            }
            Ok(v)
        });
        match res {
            Ok(v) => {
                self.conn.execute_batch("RELEASE op")?;
                Ok(v)
            }
            Err(e) => {
                if let Err(rb) = self.conn.execute_batch("ROLLBACK TO op; RELEASE op") {
                    tracing::error!("store savepoint rollback failed: {rb}");
                }
                self.next_seq = seq_before;
                self.clear_caches();
                Err(e)
            }
        }
    }

    pub(super) fn commit(&mut self) -> Result<()> {
        if !self.lost.is_empty() {
            // Rides this batch: once it commits, the lost spans are on record.
            let lost: Vec<(HostId, (i64, i64))> =
                self.lost.iter().map(|(h, span)| (*h, *span)).collect();
            if let Err(e) = self.apply(|s| s.write_lost_gaps(&lost)) {
                tracing::warn!("store: recording lost spans failed: {e}");
            }
        }
        if !self.in_txn {
            return Ok(());
        }
        self.in_txn = false;
        self.deadline = None;
        if let Err(e) = self.commit_txn() {
            // The batch is lost. Remember what it covered, then resynchronise in-memory
            // state with the file.
            for (host, (start, end)) in std::mem::take(&mut self.batch_span) {
                widen(&mut self.lost, host, start, end);
            }
            let _ = self.conn.execute_batch("ROLLBACK");
            self.clear_caches();
            self.next_seq = db::read_next_seq(&self.conn)?;
            return Err(e.into());
        }
        self.batch_span.clear();
        self.lost.clear();
        #[cfg(feature = "write-stats")]
        {
            self.commits += 1;
        }
        Ok(())
    }

    pub(super) fn commit_txn(&self) -> rusqlite::Result<()> {
        #[cfg(feature = "write-stats")]
        if self.fail_commits.load(Ordering::SeqCst) {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL),
                Some("commit failure injected by a test".into()),
            ));
        }
        self.conn.execute_batch("COMMIT")
    }

    #[cfg(feature = "write-stats")]
    pub(super) fn write_stats(&self) -> Result<WriteStats> {
        use rusqlite::ffi;
        let (mut cur, mut high) = (0, 0);
        // SAFETY: the handle is this thread's open connection and outlives the call; both
        // out-pointers are locals. The call only reads the connection's counters.
        let rc = unsafe {
            ffi::sqlite3_db_status(
                self.conn.handle(),
                ffi::SQLITE_DBSTATUS_CACHE_WRITE,
                &raw mut cur,
                &raw mut high,
                0,
            )
        };
        if rc != ffi::SQLITE_OK {
            return Err(rusqlite::Error::SqliteFailure(ffi::Error::new(rc), None).into());
        }
        let page_size: i64 = self.conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        Ok(WriteStats {
            commits: self.commits,
            wal_frames: u64::try_from(cur).unwrap_or(0),
            page_size: u64::try_from(page_size).unwrap_or(0),
        })
    }

    /// One `write_failed` gap per host over the span a failed commit lost. A host the
    /// store does not know (cleared, or rolled back with the batch) is skipped.
    pub(super) fn write_lost_gaps(&mut self, lost: &[(HostId, (i64, i64))]) -> Result<()> {
        for &(host, (start, end)) in lost {
            let host_ref = match self.host_ref(host) {
                Ok(r) => r,
                Err(StoreError::UnknownHost(_)) => continue,
                Err(e) => return Err(e),
            };
            let gap = Gap::host(start, Some(end.max(start)), GapReason::WriteFailed)?;
            self.upsert_gap(host_ref, &gap)?;
        }
        Ok(())
    }

    /// Between two batches of a long operation: handles everything queued meanwhile, in
    /// order, so a flush (before sleep) or the engine's writes do not wait for the whole
    /// prune. Another prune or a clear waits until this one ends. A queued shutdown ends
    /// the operation with [`StoreError::WriterGone`]; `run` then shuts down.
    pub(super) fn yield_to_queue(&mut self) -> Result<()> {
        loop {
            match self.rx.try_recv() {
                Ok(Op::Shutdown(reply)) => {
                    self.pending_shutdown = Some(reply);
                    return Err(StoreError::WriterGone);
                }
                Ok(op @ (Op::Prune(..) | Op::ClearHost(..))) => self.deferred.push_back(op),
                Ok(op) => self.handle(op),
                Err(_) => return Ok(()),
            }
        }
    }

    pub(super) fn commit_logged(&mut self) {
        if let Err(e) = self.commit() {
            tracing::error!("store commit failed, batch lost: {e}");
        }
    }

    pub(super) fn shutdown(&mut self) -> Result<()> {
        self.commit()?;
        self.checkpoint_truncate()
    }

    /// Copies the WAL into the database and truncates it to zero bytes, which also lets
    /// the file shrink by what `incremental_vacuum` released. A reader mid-transaction
    /// can keep it from finishing; the WAL then stays until the next checkpoint.
    pub(super) fn checkpoint_truncate(&mut self) -> Result<()> {
        let busy: i64 = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
        if busy != 0 {
            tracing::debug!("store checkpoint blocked by a reader; the WAL stays for now");
        }
        Ok(())
    }

    /// Bytes of live pages: what the database file holds once free pages are vacuumed
    /// and the WAL is checkpointed. Unlike the file sizes it does not depend on whether a
    /// reader blocked the last checkpoint.
    pub(super) fn used_bytes(&self) -> Result<u64> {
        let get = |sql: &str| -> Result<i64> { Ok(self.conn.query_row(sql, [], |r| r.get(0))?) };
        let pages = get("PRAGMA page_count")? - get("PRAGMA freelist_count")?;
        Ok(u64::try_from(pages.max(0) * get("PRAGMA page_size")?).unwrap_or(0))
    }

    /// Returns free pages to the file system. SQLite frees one page per step of this
    /// pragma, so every row has to be stepped through; `execute_batch` would free one.
    pub(super) fn incremental_vacuum(&mut self) -> Result<()> {
        let mut stmt = self.conn.prepare("PRAGMA incremental_vacuum")?;
        let mut rows = stmt.query([])?;
        while rows.next()?.is_some() {}
        Ok(())
    }

    pub(super) fn clear_caches(&mut self) {
        self.hosts.clear();
        self.series.clear();
        self.layouts.clear();
        self.proc_names.clear();
    }

    pub(super) fn alloc_seq(&mut self) -> i64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }

    /// Gives back the `seq` just allocated when its write turned out to be a no-op.
    pub(super) fn unalloc_seq(&mut self) {
        self.next_seq -= 1;
    }
}

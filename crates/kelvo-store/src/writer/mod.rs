//! The single writer (architecture.md infra 8): one thread owns the only write
//! connection and applies operations sent over a channel.
//!
//! Writes accumulate in one open transaction that commits every `commit_interval`
//! (5 minutes by default, D-070) and on [`Writer::flush`]. A crash loses at most that
//! much persisted history; charts do not show the uncommitted span as missing because
//! the Timeline draws the newest buckets from the engine's ring buffer. Each
//! operation runs inside a savepoint, so a failing operation rolls back alone and leaves
//! the rest of the batch intact.
//!
//! Every row gets a `seq` from `meta.next_seq`, incremented inside the same transaction.
//! A row that changes (an upsert with new values, a gap that closes) gets a new `seq`, so
//! a cursor reader picks the change up; an upsert with identical values keeps its row and
//! its `seq` untouched.
//!
//! A batch that fails to commit is lost. The writer remembers the wall-clock span its
//! buckets, process snapshots and gap writes covered and writes a `write_failed` gap over
//! it with the next commit that succeeds, so a chart shows a gap there instead of a line
//! (D-064). A gap close that finds no open row writes the whole gap from the start the
//! caller passes, so a gap whose open was lost is still recorded (D-070).
//!
//! Pruning runs in batches and, between batches, handles whatever else is queued (a
//! flush before sleep, the engine's writes), so a long prune never holds up a flush. A
//! shutdown queued behind a prune ends the prune early (D-064).

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use kelvo_schema::{Event, Gap, GapReason, HostId, HostRecord, Module, SeriesKey, Tier};
use rusqlite::{Connection, params};

use crate::db::{self, tier_table, tier_width};
use crate::error::{Result, StoreError};
use crate::types::{
    BucketRow, CursorPage, IngestReport, NetBucket, ProcRow, PruneReport, Retention, SessionStart,
};

mod discard;
mod intern;
mod prune;
mod rows;
mod sync;
mod txn;

pub use intern::NET_NEW_NAMES_PER_HOUR;

type Reply<T> = mpsc::SyncSender<Result<T>>;

enum Op {
    UpsertHost(HostRecord, Reply<()>),
    BeginSession(HostId, i64, Reply<SessionStart>),
    Bucket(BucketRow),
    OpenGap(HostId, Gap),
    CloseGap {
        host: HostId,
        reason: GapReason,
        module: Option<Module>,
        start_ms: Option<i64>,
        end_ms: i64,
    },
    WriteGap(HostId, Gap),
    Event {
        host: HostId,
        ts_ms: i64,
        kind: String,
        payload: Vec<u8>,
    },
    ProcSnapshot {
        host: HostId,
        ts_ms: i64,
        rows: Vec<ProcRow>,
    },
    NetBucket {
        host: HostId,
        bucket_ts: i64,
        bucket: NetBucket,
    },
    IngestPage(HostId, CursorPage, Reply<IngestReport>),
    RecordTruncation(HostId, Tier, i64, Reply<Option<Gap>>),
    Prune(i64, Retention, Reply<PruneReport>),
    SetS10Paused(bool, i64, Reply<()>),
    ClearHost(HostId, i64, Reply<()>),
    DiscardFrom(HostId, i64, Reply<()>),
    Flush(Reply<()>),
    Commit,
    #[cfg(feature = "write-stats")]
    WriteStats(Reply<WriteStats>),
    Shutdown(Reply<()>),
}

/// What the writer has put on disk so far (feature `write-stats`, for the engine's
/// write-volume gate).
#[cfg(feature = "write-stats")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriteStats {
    /// Transactions committed.
    pub commits: u64,
    /// Pages the write connection wrote to the WAL (`SQLITE_DBSTATUS_CACHE_WRITE`),
    /// cache spills of an open transaction included. Checkpoint copies into the main file
    /// are not counted.
    pub wal_frames: u64,
    pub page_size: u64,
}

#[cfg(feature = "write-stats")]
impl WriteStats {
    /// Bytes appended to the WAL: each frame is a page plus its 24-byte header.
    pub fn wal_bytes(&self) -> u64 {
        self.wal_frames * (self.page_size + 24)
    }
}

/// A handle to the writer thread. Cheap to clone and `Send`; every clone feeds the same
/// thread.
///
/// Methods that return data or that callers must know succeeded (host upsert, sync
/// ingest, pruning, flush) wait for the writer. The per-tick write paths (buckets, gaps,
/// events, process snapshots) are validated here and queued without waiting, so the
/// engine never blocks on disk; a failure inside the writer is logged.
#[derive(Clone)]
pub struct Writer {
    tx: mpsc::Sender<Op>,
    /// Low-disk mode: S10 buckets are dropped before they are queued.
    s10_paused: Arc<AtomicBool>,
    /// Test support: every commit fails while set ([`Writer::set_fail_commits`]).
    #[cfg(feature = "write-stats")]
    fail_commits: Arc<AtomicBool>,
}

impl Writer {
    fn call<T>(&self, make: impl FnOnce(Reply<T>) -> Op) -> Result<T> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.tx
            .send(make(reply))
            .map_err(|_| StoreError::WriterGone)?;
        rx.recv().map_err(|_| StoreError::WriterGone)?
    }

    fn queue(&self, op: Op) -> Result<()> {
        self.tx.send(op).map_err(|_| StoreError::WriterGone)
    }

    /// Inserts or updates a host, keyed by its UUID, and commits. Must precede any write
    /// for it. At most one host is local: registering a local host makes any other
    /// stored host non-local (its history stays, under its own id).
    pub fn upsert_host(&self, record: HostRecord) -> Result<()> {
        self.call(|r| Op::UpsertHost(record, r))
    }

    /// Startup repair for `host`: closes gaps the previous run left open and writes an
    /// `app_not_running` gap from the last persisted bucket to `now_ms`.
    pub fn begin_session(&self, host: HostId, now_ms: i64) -> Result<SessionStart> {
        self.call(|r| Op::BeginSession(host, now_ms, r))
    }

    /// Queues one closed bucket. Upserts on `(host, bucket_ts, layout)`. While S10 is
    /// paused ([`Writer::set_s10_paused`]) an S10 bucket is dropped and this returns `Ok`.
    pub fn write_bucket(&self, row: BucketRow) -> Result<()> {
        tier_table(row.tier)?;
        check_stats(row.series.len(), row.stats.len())?;
        if row.tier == Tier::S10 && self.s10_paused() {
            return Ok(());
        }
        self.queue(Op::Bucket(row))
    }

    /// Queues the start of a gap (`end_ms` = `None`). A second open for the same reason
    /// and module while one is open is a no-op.
    pub fn open_gap(
        &self,
        host: HostId,
        start_ms: i64,
        module: Option<Module>,
        reason: GapReason,
    ) -> Result<()> {
        let gap = Gap::new(start_ms, None, module, reason)?;
        self.queue(Op::OpenGap(host, gap))
    }

    /// Queues closing every open gap of `reason` and `module` at `end_ms`. When none is
    /// open and the caller knows when the gap began (`start_ms`), writes the closed gap
    /// `[start_ms, end_ms]` instead: its open was in a batch that failed to commit, and
    /// without this the gap would never be recorded.
    pub fn close_gap(
        &self,
        host: HostId,
        reason: GapReason,
        module: Option<Module>,
        start_ms: Option<i64>,
        end_ms: i64,
    ) -> Result<()> {
        self.queue(Op::CloseGap {
            host,
            reason,
            module,
            start_ms,
            end_ms,
        })
    }

    /// Queues a complete gap. Upserts on `(host, start, reason, module)`.
    pub fn write_gap(&self, host: HostId, gap: Gap) -> Result<()> {
        gap.validate()?;
        self.queue(Op::WriteGap(host, gap))
    }

    /// Queues an event row (v1.2 detectors). Upserts on `(host, ts, kind)`.
    pub fn write_event(
        &self,
        host: HostId,
        ts_ms: i64,
        kind: impl Into<String>,
        payload: Vec<u8>,
    ) -> Result<()> {
        self.queue(Op::Event {
            host,
            ts_ms,
            kind: kind.into(),
            payload,
        })
    }

    /// Queues `event` as an `events` row: `ts` and `kind` from the event, the whole event
    /// in CBOR as the payload. Upserts on `(host, ts, kind)`.
    pub fn record_event(&self, host: HostId, event: &Event) -> Result<()> {
        let mut payload = Vec::new();
        ciborium::into_writer(event, &mut payload).map_err(|e| StoreError::Cbor(e.to_string()))?;
        self.write_event(host, event.ts_ms, event.detail.kind(), payload)
    }

    /// Queues a commit of everything queued so far, without waiting for it: what
    /// [`Writer::flush`] does for callers that must not block (the engine after an event,
    /// so readers see it within a tick instead of at the 5-minute commit, D-083). A
    /// failure is logged by the writer.
    pub fn commit_soon(&self) -> Result<()> {
        self.queue(Op::Commit)
    }

    /// Queues a process snapshot (the top 30 by CPU, every 10 s).
    pub fn write_proc_snapshot(&self, host: HostId, ts_ms: i64, rows: Vec<ProcRow>) -> Result<()> {
        self.queue(Op::ProcSnapshot { host, ts_ms, rows })
    }

    /// Queues one closed 10 s bucket of per-app network bytes (D-089). Upserts the
    /// `proc_net_10s` row on `(host, bucket_ts)`, keeping the top
    /// [`crate::NET_TOP_APPS`] apps by rx + tx and folding the rest into "other apps",
    /// then rewrites the minute's `proc_net_1m` row as the exact sum of that minute's
    /// 10 s rows. Replaying a bucket changes nothing. `measured_ms` is capped at the
    /// bucket width. [`StoreError::Misaligned`] unless `bucket_ts` is on the 10 s grid.
    ///
    /// The minute is recomputed from its 10 s rows rather than kept in an accumulator or
    /// added to in place: an addition would count a replayed bucket twice, and an
    /// in-memory accumulator would have to be rebuilt after a failed commit. Up to six
    /// small rows of the open transaction are read per write; the minute row is rewritten
    /// in the page cache and reaches the WAL once per commit.
    pub fn write_net_bucket(
        &self,
        host: HostId,
        bucket_ts_ms: i64,
        bucket: NetBucket,
    ) -> Result<()> {
        let width = tier_width(Tier::S10)?;
        if bucket_ts_ms.rem_euclid(width) != 0 {
            return Err(StoreError::Misaligned {
                ts_ms: bucket_ts_ms,
                width_ms: width,
            });
        }
        self.queue(Op::NetBucket {
            host,
            bucket_ts: bucket_ts_ms,
            bucket,
        })
    }

    /// Controller side of sync: stores a page from a remote host's database under `host`,
    /// remapping its series keys to local IDs, and advances the `(host, tier)` cursor to
    /// `(page.epoch, page.last_seq)` in the same transaction. Replaying a page is harmless.
    pub fn ingest_page(&self, host: HostId, page: CursorPage) -> Result<IngestReport> {
        self.call(|r| Op::IngestPage(host, page, r))
    }

    /// Controller side of sync after a `Truncated` reply: forgets the `(host, tier)`
    /// cursor so the next request starts from the earliest row, and for `M1` writes a
    /// `truncated` gap from the end of the last synced bucket to `earliest_ts_ms`.
    pub fn record_truncation(
        &self,
        host: HostId,
        tier: Tier,
        earliest_ts_ms: i64,
    ) -> Result<Option<Gap>> {
        self.call(|r| Op::RecordTruncation(host, tier, earliest_ts_ms, r))
    }

    /// Deletes rows past retention in batches ([`crate::StoreConfig::prune_batch`]),
    /// rolls process snapshots older than 72 h down into `proc_top_1m`, rolls minutes
    /// (`tier_1m`, `proc_top_1m`) older than the M1 window into 15-minute rows
    /// (`tier_15m`, `proc_top_15m`, D-076) and network minutes into `proc_net_15m`
    /// (D-089), deletes network 10 s rows past `proc_snap_ms` (on minute boundaries, and
    /// never longer than history), runs `incremental_vacuum` and checkpoints the
    /// WAL with `TRUNCATE`. If the file is then over `retention.max_bytes`, trims the
    /// oldest history on every host until it is under the low-water mark (D-057).
    pub fn prune(&self, now_ms: i64, retention: Retention) -> Result<PruneReport> {
        self.call(|r| Op::Prune(now_ms, retention, r))
    }

    /// Low-disk mode (D-057): while paused, S10 buckets are dropped instead of written;
    /// M1 and everything else is still written. The span from the pause to the resume is
    /// recorded so history queries read M1 there rather than a 10 s tier with holes.
    pub fn set_s10_paused(&self, paused: bool, now_ms: i64) -> Result<()> {
        if paused {
            self.s10_paused.store(true, Ordering::Relaxed);
            self.call(|r| Op::SetS10Paused(true, now_ms, r))
        } else {
            let res = self.call(|r| Op::SetS10Paused(false, now_ms, r));
            self.s10_paused.store(false, Ordering::Relaxed);
            res
        }
    }

    /// Whether S10 writes are paused for low disk space.
    pub fn s10_paused(&self) -> bool {
        self.s10_paused.load(Ordering::Relaxed)
    }

    /// Deletes every history row of `host` (Settings, "Clear history"). Series, layouts
    /// and the host record stay. A remote cursor into the cleared span gets `Truncated`.
    pub fn clear_host(&self, host: HostId, now_ms: i64) -> Result<()> {
        self.call(|r| Op::ClearHost(host, now_ms, r))
    }

    /// Deletes every row of `host` stamped at or after `from_ms`, and commits: buckets of
    /// every tier, process snapshots and their roll-ups, network rows of every tier,
    /// events, and gaps that start there. A closed gap that began earlier and ended later
    /// is cut at `from_ms`; the network minute holding `from_ms` is summed again from its
    /// remaining 10 s rows (a 15-minute row is not: it is a week old). The
    /// engine sends it after the wall clock stepped back a long way, so the rows a wrong
    /// clock wrote ahead of the new one never mix with the new timeline (D-070).
    ///
    /// What was queued before it commits first, on its own; the discard is then its own
    /// transaction. An error means the discard did not reach the file, and the caller
    /// retries.
    pub fn discard_from(&self, host: HostId, from_ms: i64) -> Result<()> {
        self.call(|r| Op::DiscardFrom(host, from_ms, r))
    }

    /// Commits everything queued so far (shutdown, sleep, tests).
    pub fn flush(&self) -> Result<()> {
        self.call(Op::Flush)
    }

    /// Commits and WAL frames written so far, after everything queued before this call.
    #[cfg(feature = "write-stats")]
    pub fn write_stats(&self) -> Result<WriteStats> {
        self.call(Op::WriteStats)
    }

    /// Test support for the engine's store-failure tests (feature `write-stats`, a
    /// dev-dependency feature there): while set, every COMMIT fails the way a full disk
    /// would, and the batch is lost.
    #[cfg(feature = "write-stats")]
    pub fn set_fail_commits(&self, fail: bool) {
        self.fail_commits.store(fail, Ordering::SeqCst);
    }

    /// Commits, checkpoints the WAL and stops the thread.
    pub(crate) fn shutdown(&self) -> Result<()> {
        self.call(Op::Shutdown)
    }
}

fn check_stats(series: usize, stats: usize) -> Result<()> {
    if stats == series * 3 {
        Ok(())
    } else {
        Err(StoreError::StatsLength { series, stats })
    }
}

pub(crate) fn spawn(
    conn: Connection,
    path: PathBuf,
    commit_interval: Duration,
    prune_batch: u64,
) -> Result<(Writer, JoinHandle<()>)> {
    let (tx, rx) = mpsc::channel();
    let mut state = State::new(conn, path, commit_interval, rx)?;
    state.prune_batch = prune_batch;
    #[cfg(feature = "write-stats")]
    let fail_commits = Arc::clone(&state.fail_commits);
    let handle = std::thread::Builder::new()
        .name("kelvo-store-writer".into())
        .spawn(move || state.run())?;
    Ok((
        Writer {
            tx,
            s10_paused: Arc::new(AtomicBool::new(false)),
            #[cfg(feature = "write-stats")]
            fail_commits,
        },
        handle,
    ))
}

struct State {
    conn: Connection,
    rx: mpsc::Receiver<Op>,
    /// The database file, for measuring its size on disk.
    path: PathBuf,
    next_seq: i64,
    in_txn: bool,
    deadline: Option<Instant>,
    commit_interval: Duration,
    /// Rows deleted per pruning transaction ([`crate::StoreConfig::prune_batch`]).
    prune_batch: u64,
    hosts: HashMap<HostId, i64>,
    series: HashMap<i64, HashMap<SeriesKey, u32>>,
    layouts: HashMap<i64, HashMap<Arc<[SeriesKey]>, u32>>,
    proc_names: HashMap<i64, HashMap<String, u32>>,
    /// Per host: the hour (bucket time, ms / 1 h) and how many names network buckets
    /// interned in it ([`NET_NEW_NAMES_PER_HOUR`]).
    net_new_names: HashMap<i64, (i64, u32)>,
    /// Wall-clock span, per host, of the buckets, process snapshots and gap writes in the
    /// open batch.
    batch_span: HashMap<HostId, (i64, i64)>,
    /// Spans whose batch failed to commit. The next commit writes a `write_failed` gap
    /// over each and forgets them once that commit succeeds.
    lost: HashMap<HostId, (i64, i64)>,
    /// A shutdown that arrived while a prune was yielding; `run` performs it next.
    pending_shutdown: Option<Reply<()>>,
    /// Operations that must not run in the middle of a prune (another prune, clearing a
    /// host), held until it ends.
    deferred: VecDeque<Op>,
    #[cfg(feature = "write-stats")]
    commits: u64,
    #[cfg(feature = "write-stats")]
    fail_commits: Arc<AtomicBool>,
}

/// Widens the span of `host` to cover `[start, end]`.
fn widen(spans: &mut HashMap<HostId, (i64, i64)>, host: HostId, start: i64, end: i64) {
    spans
        .entry(host)
        .and_modify(|(s, e)| {
            *s = (*s).min(start);
            *e = (*e).max(end);
        })
        .or_insert((start, end));
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn to_u32(id: i64, what: &str) -> Result<u32> {
    u32::try_from(id).map_err(|_| StoreError::Corrupt(format!("{what} id {id} exceeds u32")))
}

impl State {
    fn new(
        conn: Connection,
        path: PathBuf,
        commit_interval: Duration,
        rx: mpsc::Receiver<Op>,
    ) -> Result<State> {
        let next_seq = db::read_next_seq(&conn)?;
        Ok(State {
            conn,
            rx,
            path,
            next_seq,
            in_txn: false,
            deadline: None,
            commit_interval,
            prune_batch: crate::DEFAULT_PRUNE_BATCH,
            hosts: HashMap::new(),
            series: HashMap::new(),
            layouts: HashMap::new(),
            proc_names: HashMap::new(),
            net_new_names: HashMap::new(),
            batch_span: HashMap::new(),
            lost: HashMap::new(),
            pending_shutdown: None,
            deferred: VecDeque::new(),
            #[cfg(feature = "write-stats")]
            commits: 0,
            #[cfg(feature = "write-stats")]
            fail_commits: Arc::new(AtomicBool::new(false)),
        })
    }

    fn run(&mut self) {
        loop {
            if let Some(reply) = self.pending_shutdown.take() {
                let _ = reply.send(self.shutdown());
                return;
            }
            if let Some(op) = self.deferred.pop_front() {
                self.handle(op);
                continue;
            }
            let op = match self.deadline {
                Some(d) => match self
                    .rx
                    .recv_timeout(d.saturating_duration_since(Instant::now()))
                {
                    Ok(op) => op,
                    Err(RecvTimeoutError::Timeout) => {
                        self.commit_logged();
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match self.rx.recv() {
                    Ok(op) => op,
                    Err(_) => break,
                },
            };
            if let Op::Shutdown(reply) = op {
                let _ = reply.send(self.shutdown());
                return;
            }
            self.handle(op);
        }
        // Every handle dropped without a shutdown: still leave the file consistent.
        if let Err(e) = self.shutdown() {
            tracing::error!("store writer shutdown failed: {e}");
        }
    }

    fn handle(&mut self, op: Op) {
        match op {
            Op::UpsertHost(record, r) => {
                // Rare, and readers (list_hosts) need it at once: commit with it.
                let res = self.apply(|s| s.upsert_host(&record));
                let _ = r.send(res.and_then(|()| self.commit()));
            }
            Op::BeginSession(host, now, r) => {
                let _ = r.send(self.apply(|s| s.begin_session(host, now)));
            }
            Op::Bucket(row) => {
                let res = self.apply(|s| s.write_bucket(&row).map(|_| ()));
                if res.is_ok()
                    && let Ok(width) = tier_width(row.tier)
                {
                    widen(
                        &mut self.batch_span,
                        row.host,
                        row.bucket_ts,
                        row.bucket_ts + width,
                    );
                }
                log_queued("bucket", res);
            }
            Op::OpenGap(host, gap) => {
                let res = self.apply(|s| s.open_gap(host, &gap));
                if res.is_ok() {
                    widen(&mut self.batch_span, host, gap.start_ms, gap.start_ms);
                }
                log_queued("open gap", res);
            }
            Op::CloseGap {
                host,
                reason,
                module,
                start_ms,
                end_ms,
            } => {
                let res = self.apply(|s| s.close_gap(host, reason, module, start_ms, end_ms));
                if res.is_ok() {
                    widen(&mut self.batch_span, host, end_ms, end_ms);
                }
                log_queued("close gap", res);
            }
            Op::WriteGap(host, gap) => {
                let res = self.apply(|s| {
                    let h = s.host_ref(host)?;
                    s.upsert_gap(h, &gap).map(|_| ())
                });
                if res.is_ok() {
                    let end = gap.end_ms.unwrap_or(gap.start_ms);
                    widen(&mut self.batch_span, host, gap.start_ms, end);
                }
                log_queued("gap", res);
            }
            Op::Event {
                host,
                ts_ms,
                kind,
                payload,
            } => {
                let res = self.apply(|s| {
                    let h = s.host_ref(host)?;
                    s.upsert_event(h, ts_ms, &kind, &payload).map(|_| ())
                });
                log_queued("event", res);
            }
            Op::ProcSnapshot { host, ts_ms, rows } => {
                let res = self.apply(|s| s.write_proc_snapshot(host, ts_ms, &rows));
                if res.is_ok() {
                    widen(&mut self.batch_span, host, ts_ms, ts_ms);
                }
                log_queued("process snapshot", res);
            }
            Op::NetBucket {
                host,
                bucket_ts,
                bucket,
            } => {
                let res = self.apply(|s| s.write_net_bucket(host, bucket_ts, &bucket));
                if res.is_ok()
                    && let Ok(width) = tier_width(Tier::S10)
                {
                    widen(&mut self.batch_span, host, bucket_ts, bucket_ts + width);
                }
                log_queued("network bucket", res);
            }
            Op::IngestPage(host, page, r) => {
                let _ = r.send(self.apply(|s| s.ingest_page(host, &page)));
            }
            Op::RecordTruncation(host, tier, earliest, r) => {
                let _ = r.send(self.apply(|s| s.record_truncation(host, tier, earliest)));
            }
            Op::Prune(now, retention, r) => {
                let _ = r.send(self.prune(now, retention));
            }
            Op::SetS10Paused(paused, now, r) => {
                let until = if paused { i64::MAX } else { now };
                let res = self
                    .apply(|s| {
                        s.conn.execute(
                            "INSERT INTO meta (key, value) VALUES (?1, ?2)
                             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                            params![db::S10_HOLE_UNTIL, until],
                        )?;
                        Ok(())
                    })
                    .and_then(|()| self.commit());
                let _ = r.send(res);
            }
            Op::ClearHost(host, now, r) => {
                let _ = r.send(self.clear_host(host, now));
            }
            Op::DiscardFrom(host, from, r) => {
                let _ = r.send(self.discard_committed(host, from));
            }
            Op::Flush(r) => {
                let _ = r.send(self.commit());
            }
            Op::Commit => self.commit_logged(),
            #[cfg(feature = "write-stats")]
            Op::WriteStats(r) => {
                let _ = r.send(self.write_stats());
            }
            Op::Shutdown(_) => {}
        }
    }
}

fn log_queued(what: &str, res: Result<()>) {
    if let Err(e) = res {
        tracing::warn!("store: queued {what} write failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A writer state on its own file whose COMMIT fails while the returned flag is set,
    /// with one host registered.
    fn failing_state(name: &str) -> (State, Arc<AtomicBool>, HostId) {
        use kelvo_schema::{ClusterInfo, CoreKind, HostInfo, OsKind};

        let dir = crate::test_dir(name);
        let path = dir.join("h.sqlite");
        let (conn, _) = db::open_writer(&path, 0).unwrap();
        let fail = Arc::new(AtomicBool::new(false));
        let hook = Arc::clone(&fail);
        conn.commit_hook(Some(move || hook.load(Ordering::SeqCst)))
            .unwrap();
        let (_tx, rx) = mpsc::channel();
        let mut s = State::new(conn, path, Duration::from_secs(3600), rx).unwrap();
        let host = HostId(uuid::Uuid::from_u128(1));
        let record = HostRecord {
            id: host,
            is_local: true,
            display_name: "h".into(),
            info: HostInfo {
                os: OsKind::MacOs,
                os_version: "27.0".into(),
                model: None,
                chip: None,
                chip_known: false,
                cpu_topology: vec![ClusterInfo {
                    name: "P0".into(),
                    kind: CoreKind::Performance,
                    cores: vec!["P0".into()],
                    dvfs_mhz: vec![],
                }],
                mem_total_bytes: 1,
                boot_time_ms: 0,
                gpu_dvfs_mhz: Vec::new(),
                boot_mounts: Vec::new(),
            },
        };
        let (tx, rx) = mpsc::sync_channel(1);
        s.handle(Op::UpsertHost(record, tx));
        rx.recv().unwrap().unwrap();
        (s, fail, host)
    }

    fn flush(s: &mut State) -> Result<()> {
        let (tx, rx) = mpsc::sync_channel(1);
        s.handle(Op::Flush(tx));
        rx.recv().unwrap()
    }

    fn gap_rows(s: &State) -> Vec<(i64, Option<i64>, String)> {
        s.conn
            .prepare("SELECT start_ts, end_ts, reason FROM gaps ORDER BY start_ts, reason")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    const T: i64 = 1_788_220_800_000;
    const MIN: i64 = 60_000;

    /// D-064: a batch whose COMMIT fails is lost. The next commit that succeeds records
    /// a `write_failed` gap over the span the lost buckets covered, so the chart shows a
    /// gap there instead of a line drawn across it.
    #[test]
    fn a_failed_commit_is_recorded_as_a_write_failed_gap() {
        let (mut s, fail, host) = failing_state("write-failed");

        let series: Arc<[SeriesKey]> = Arc::from(vec![SeriesKey::parse("cpu.total").unwrap()]);
        let bucket = |ts: i64| {
            Op::Bucket(BucketRow {
                host,
                tier: Tier::M1,
                bucket_ts: ts,
                series: Arc::clone(&series),
                stats: vec![1.0, 2.0, 1.5],
            })
        };
        fail.store(true, Ordering::SeqCst);
        for i in 0..3 {
            s.handle(bucket(T + i * MIN));
        }
        assert!(
            flush(&mut s).is_err(),
            "the commit hook rolled the batch back"
        );
        // Still failing: the next batch is lost too, and the span grows.
        s.handle(bucket(T + 3 * MIN));
        assert!(flush(&mut s).is_err());

        fail.store(false, Ordering::SeqCst);
        s.handle(bucket(T + 10 * MIN));
        flush(&mut s).unwrap();
        // A later commit does not write the gap again.
        s.handle(bucket(T + 11 * MIN));
        flush(&mut s).unwrap();

        assert_eq!(
            gap_rows(&s),
            [(T, Some(T + 4 * MIN), "write_failed".to_string())]
        );
        let rows: Vec<i64> = s
            .conn
            .prepare("SELECT bucket_ts FROM tier_1m ORDER BY bucket_ts")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(rows, [T + 10 * MIN, T + 11 * MIN], "only what committed");
    }

    fn discard(s: &mut State, host: HostId, from: i64) -> Result<()> {
        let (tx, rx) = mpsc::sync_channel(1);
        s.handle(Op::DiscardFrom(host, from, tx));
        rx.recv().unwrap()
    }

    fn m1_bucket(host: HostId, ts: i64) -> Op {
        Op::Bucket(BucketRow {
            host,
            tier: Tier::M1,
            bucket_ts: ts,
            series: Arc::from(vec![SeriesKey::parse("cpu.total").unwrap()]),
            stats: vec![1.0, 2.0, 1.5],
        })
    }

    /// `tier_1m` as another connection sees it: committed rows only.
    fn committed_minutes(s: &State) -> Vec<i64> {
        Connection::open(&s.path)
            .unwrap()
            .prepare("SELECT bucket_ts FROM tier_1m ORDER BY bucket_ts")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// D-070: the discard commits at once, with what was queued before it, so the wrong
    /// clock's rows are gone from the file when the call returns.
    #[test]
    fn discard_from_commits_before_it_returns() {
        let (mut s, _fail, host) = failing_state("discard-commits");
        for i in 0..3 {
            s.handle(m1_bucket(host, T + i * 10 * MIN));
        }
        discard(&mut s, host, T + 15 * MIN).unwrap();
        assert_eq!(committed_minutes(&s), [T, T + 10 * MIN]);
    }

    /// A discard whose commit fails reports it, so the engine retries; the retry removes
    /// the rows, and the `write_failed` gap over the lost batch stops at the cut.
    #[test]
    fn a_discard_that_fails_to_commit_is_reported_and_can_be_retried() {
        let (mut s, fail, host) = failing_state("discard-fails");
        s.handle(m1_bucket(host, T));
        flush(&mut s).unwrap();
        s.handle(m1_bucket(host, T + 10 * MIN));
        s.handle(m1_bucket(host, T + 30 * MIN));
        fail.store(true, Ordering::SeqCst);
        assert!(discard(&mut s, host, T + 20 * MIN).is_err());

        fail.store(false, Ordering::SeqCst);
        // A row past the cut that did commit before the retry.
        s.handle(m1_bucket(host, T + 40 * MIN));
        flush(&mut s).unwrap();
        discard(&mut s, host, T + 20 * MIN).unwrap();
        assert_eq!(committed_minutes(&s), [T]);
        assert_eq!(
            gap_rows(&s),
            [(T + 10 * MIN, Some(T + 20 * MIN), "write_failed".to_string())],
            "the gap over the lost batch ends at the cut"
        );
    }

    /// Review #9: the sleep gap's open was in a batch that failed to commit. The close
    /// carries the start the engine knows, so the gap is still recorded.
    #[test]
    fn a_sleep_gap_whose_open_was_lost_is_written_on_close() {
        let (mut s, fail, host) = failing_state("lost-open-gap");
        fail.store(true, Ordering::SeqCst);
        s.handle(Op::OpenGap(
            host,
            Gap::host(T, None, GapReason::Sleep).unwrap(),
        ));
        assert!(flush(&mut s).is_err());

        fail.store(false, Ordering::SeqCst);
        s.handle(Op::CloseGap {
            host,
            reason: GapReason::Sleep,
            module: None,
            start_ms: Some(T),
            end_ms: T + 5 * MIN,
        });
        flush(&mut s).unwrap();
        let gaps = gap_rows(&s);
        assert!(
            gaps.contains(&(T, Some(T + 5 * MIN), "sleep".to_string())),
            "{gaps:?}"
        );
    }
}

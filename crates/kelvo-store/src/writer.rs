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

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use kelvo_schema::{Event, Gap, GapReason, HostId, HostRecord, Module, SeriesKey, Tier};
use rusqlite::{Connection, OptionalExtension, params};

use crate::blob::{self, OTHER_APPS, PackedProc};
use crate::db::{self, net_table, tier_table, tier_width};
use crate::error::{Result, StoreError};
use crate::rolldown::{NetFold, StatsFold};
use crate::types::{
    BucketRow, CapTrim, CursorPage, IngestReport, NetBucket, ProcRow, PruneReport, Retention,
    SessionStart,
};

/// Span of process snapshots rolled down per transaction (500 minutes).
const ROLL_DOWN_SNAPSHOTS_MS: i64 = 500 * 60_000;
/// Span of minute rows (buckets or process top 5) rolled into 15-minute rows per
/// transaction: a day, 1,440 rows per layout.
const ROLL_DOWN_MINUTES_MS: i64 = Retention::DAY_MS;
/// Processes kept per minute in `proc_top_1m`.
const TOP_PER_MINUTE: usize = 5;
/// The byte cap never trims the most recent day: below that, history is not worth having
/// and the cap is reported unmet instead.
const CAP_TRIM_KEEP_MS: i64 = Retention::DAY_MS;
/// Smallest slice of history one cap-trim round removes.
const CAP_TRIM_MIN_STEP_MS: i64 = 3_600_000;
/// Bound on cap-trim rounds per prune; each round re-measures the file.
const CAP_TRIM_MAX_ROUNDS: usize = 64;
/// New `proc_names` rows network buckets may create per host and hour of bucket time.
/// Past it, an app whose name is not interned yet is counted in "other apps", so a
/// process that rewrites its argv on every launch cannot grow the table without bound.
/// Names already interned (by a process snapshot or an earlier bucket) are not limited.
/// The count lives in the writer, so it starts over when the app restarts.
pub const NET_NEW_NAMES_PER_HOUR: u32 = 64;
const HOUR_MS: i64 = 3_600_000;

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

    // --- transactions -----------------------------------------------------------------

    /// Runs `f` inside the batch transaction, in its own savepoint. On error the
    /// savepoint is rolled back, `next_seq` restored and the intern caches dropped (they
    /// may name rows the rollback removed).
    fn apply<T>(&mut self, f: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
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

    fn commit(&mut self) -> Result<()> {
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

    fn commit_txn(&self) -> rusqlite::Result<()> {
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
    fn write_stats(&self) -> Result<WriteStats> {
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
    fn write_lost_gaps(&mut self, lost: &[(HostId, (i64, i64))]) -> Result<()> {
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
    fn yield_to_queue(&mut self) -> Result<()> {
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

    fn commit_logged(&mut self) {
        if let Err(e) = self.commit() {
            tracing::error!("store commit failed, batch lost: {e}");
        }
    }

    fn shutdown(&mut self) -> Result<()> {
        self.commit()?;
        self.checkpoint_truncate()
    }

    /// Copies the WAL into the database and truncates it to zero bytes, which also lets
    /// the file shrink by what `incremental_vacuum` released. A reader mid-transaction
    /// can keep it from finishing; the WAL then stays until the next checkpoint.
    fn checkpoint_truncate(&mut self) -> Result<()> {
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
    fn used_bytes(&self) -> Result<u64> {
        let get = |sql: &str| -> Result<i64> { Ok(self.conn.query_row(sql, [], |r| r.get(0))?) };
        let pages = get("PRAGMA page_count")? - get("PRAGMA freelist_count")?;
        Ok(u64::try_from(pages.max(0) * get("PRAGMA page_size")?).unwrap_or(0))
    }

    /// Returns free pages to the file system. SQLite frees one page per step of this
    /// pragma, so every row has to be stepped through; `execute_batch` would free one.
    fn incremental_vacuum(&mut self) -> Result<()> {
        let mut stmt = self.conn.prepare("PRAGMA incremental_vacuum")?;
        let mut rows = stmt.query([])?;
        while rows.next()?.is_some() {}
        Ok(())
    }

    fn clear_caches(&mut self) {
        self.hosts.clear();
        self.series.clear();
        self.layouts.clear();
        self.proc_names.clear();
    }

    fn alloc_seq(&mut self) -> i64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }

    /// Gives back the `seq` just allocated when its write turned out to be a no-op.
    fn unalloc_seq(&mut self) {
        self.next_seq -= 1;
    }

    // --- interning --------------------------------------------------------------------

    fn upsert_host(&mut self, record: &HostRecord) -> Result<()> {
        let mut info = Vec::new();
        ciborium::into_writer(&record.info, &mut info)
            .map_err(|e| StoreError::Cbor(e.to_string()))?;
        let uuid = record.id.to_string();
        if record.is_local {
            // `hosts_one_local`: the newest local registration wins (a restored or
            // replaced `host-id` file); the old host keeps its rows as a non-local host.
            let demoted = self.conn.execute(
                "UPDATE hosts SET is_local = 0 WHERE is_local = 1 AND uuid <> ?1",
                [&uuid],
            )?;
            if demoted > 0 {
                tracing::warn!(host = %record.id, "another stored host was local; it no longer is");
            }
        } else {
            // A remote host never takes the local host's row (D-071): the upsert below
            // would demote this Mac and merge a cloned Mac's history into it.
            let is_local_id: bool = self.conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM hosts WHERE uuid = ?1 AND is_local = 1)",
                [&uuid],
                |r| r.get(0),
            )?;
            if is_local_id {
                return Err(StoreError::HostConflict(record.id));
            }
        }
        self.conn.execute(
            "INSERT INTO hosts (uuid, is_local, name, info, created_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (uuid) DO UPDATE SET
               is_local = excluded.is_local, name = excluded.name, info = excluded.info",
            params![uuid, record.is_local, record.display_name, info, now_ms()],
        )?;
        Ok(())
    }

    fn host_ref(&mut self, host: HostId) -> Result<i64> {
        if let Some(&r) = self.hosts.get(&host) {
            return Ok(r);
        }
        let r: i64 = self
            .conn
            .prepare_cached("SELECT id FROM hosts WHERE uuid = ?1")?
            .query_row([host.to_string()], |r| r.get(0))
            .optional()?
            .ok_or(StoreError::UnknownHost(host))?;
        self.hosts.insert(host, r);
        Ok(r)
    }

    fn series_id(&mut self, host_ref: i64, key: &SeriesKey) -> Result<u32> {
        if let Some(&id) = self.series.get(&host_ref).and_then(|m| m.get(key)) {
            return Ok(id);
        }
        let metric = key.metric.as_str();
        let labels = key.labels.canonical();
        let found: Option<i64> = self
            .conn
            .prepare_cached(
                "SELECT id FROM series WHERE host_id = ?1 AND metric_id = ?2 AND labels = ?3",
            )?
            .query_row(params![host_ref, metric, labels], |r| r.get(0))
            .optional()?;
        let id = match found {
            Some(id) => id,
            None => {
                self.conn
                    .prepare_cached(
                        "INSERT INTO series (host_id, metric_id, labels) VALUES (?1, ?2, ?3)",
                    )?
                    .execute(params![host_ref, metric, labels])?;
                self.conn.last_insert_rowid()
            }
        };
        let id = to_u32(id, "series")?;
        self.series
            .entry(host_ref)
            .or_default()
            .insert(key.clone(), id);
        Ok(id)
    }

    /// The layout ID for this exact series list, minting one on first sight. Layouts are
    /// immutable: a different list, even a reordering, is a different layout.
    fn layout_id(&mut self, host_ref: i64, series: &Arc<[SeriesKey]>) -> Result<u32> {
        if let Some(&id) = self.layouts.get(&host_ref).and_then(|m| m.get(&series[..])) {
            return Ok(id);
        }
        let mut seen = HashSet::with_capacity(series.len());
        let mut ids = Vec::with_capacity(series.len());
        for key in series.iter() {
            if !seen.insert(key) {
                return Err(StoreError::DuplicateSeries(key.to_string()));
            }
            ids.push(self.series_id(host_ref, key)?);
        }
        let packed = blob::pack_u32s(&ids);
        let hash = blob::layout_hash(&packed);
        let found: Option<(i64, Vec<u8>)> = self
            .conn
            .prepare_cached("SELECT id, series_ids FROM layouts WHERE host_id = ?1 AND hash = ?2")?
            .query_row(params![host_ref, &hash[..]], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let id = match found {
            Some((id, existing)) if existing == packed => id,
            Some((id, _)) => {
                return Err(StoreError::Corrupt(format!(
                    "layout hash collision with layout {id}"
                )));
            }
            None => {
                self.conn
                    .prepare_cached(
                        "INSERT INTO layouts (host_id, series_ids, hash, created_ms)
                         VALUES (?1, ?2, ?3, ?4)",
                    )?
                    .execute(params![host_ref, packed, &hash[..], now_ms()])?;
                self.conn.last_insert_rowid()
            }
        };
        let id = to_u32(id, "layout")?;
        self.layouts
            .entry(host_ref)
            .or_default()
            .insert(Arc::clone(series), id);
        Ok(id)
    }

    fn proc_name_id(&mut self, host_ref: i64, name: &str) -> Result<u32> {
        if let Some(&id) = self.proc_names.get(&host_ref).and_then(|m| m.get(name)) {
            return Ok(id);
        }
        let found: Option<i64> = self
            .conn
            .prepare_cached("SELECT id FROM proc_names WHERE host_id = ?1 AND name = ?2")?
            .query_row(params![host_ref, name], |r| r.get(0))
            .optional()?;
        let id = match found {
            Some(id) => id,
            None => {
                self.conn
                    .prepare_cached("INSERT INTO proc_names (host_id, name) VALUES (?1, ?2)")?
                    .execute(params![host_ref, name])?;
                self.conn.last_insert_rowid()
            }
        };
        let id = to_u32(id, "process name")?;
        self.proc_names
            .entry(host_ref)
            .or_default()
            .insert(name.to_owned(), id);
        Ok(id)
    }

    /// The `proc_names` id of a network app, or [`OTHER_APPS`] for one with no name or
    /// a new name past this hour's [`NET_NEW_NAMES_PER_HOUR`].
    fn net_name_id(&mut self, host_ref: i64, name: Option<&str>, bucket_ts: i64) -> Result<u32> {
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            return Ok(OTHER_APPS);
        };
        if let Some(&id) = self.proc_names.get(&host_ref).and_then(|m| m.get(name)) {
            return Ok(id);
        }
        let found: Option<i64> = self
            .conn
            .prepare_cached("SELECT id FROM proc_names WHERE host_id = ?1 AND name = ?2")?
            .query_row(params![host_ref, name], |r| r.get(0))
            .optional()?;
        let id = match found {
            Some(id) => id,
            None => {
                let hour = bucket_ts.div_euclid(HOUR_MS);
                let used = self.net_new_names.entry(host_ref).or_insert((hour, 0));
                if used.0 != hour {
                    *used = (hour, 0);
                }
                if used.1 >= NET_NEW_NAMES_PER_HOUR {
                    return Ok(OTHER_APPS);
                }
                used.1 += 1;
                self.conn
                    .prepare_cached("INSERT INTO proc_names (host_id, name) VALUES (?1, ?2)")?
                    .execute(params![host_ref, name])?;
                self.conn.last_insert_rowid()
            }
        };
        let id = to_u32(id, "process name")?;
        if id == OTHER_APPS {
            // Rowids start at 1; a 0 would read back as "other apps".
            return Err(StoreError::Corrupt(format!(
                "process name {name:?} has the reserved id 0"
            )));
        }
        self.proc_names
            .entry(host_ref)
            .or_default()
            .insert(name.to_owned(), id);
        Ok(id)
    }

    // --- rows ---------------------------------------------------------------------------

    fn write_bucket(&mut self, row: &BucketRow) -> Result<bool> {
        let host_ref = self.host_ref(row.host)?;
        let layout = self.layout_id(host_ref, &row.series)?;
        self.upsert_bucket(
            host_ref,
            row.tier,
            row.bucket_ts,
            layout,
            &blob::pack_f32s(&row.stats),
        )
    }

    /// Upserts on `(host, bucket_ts, layout)`. Returns whether the row changed.
    fn upsert_bucket(
        &mut self,
        host_ref: i64,
        tier: Tier,
        bucket_ts: i64,
        layout: u32,
        stats: &[u8],
    ) -> Result<bool> {
        let table = tier_table(tier)?;
        let seq = self.alloc_seq();
        let n = self
            .conn
            .prepare_cached(&format!(
                "INSERT INTO {table} (host_id, bucket_ts, layout_id, seq, blob)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (host_id, bucket_ts, layout_id) DO UPDATE
                   SET seq = excluded.seq, blob = excluded.blob
                   WHERE blob IS NOT excluded.blob"
            ))?
            .execute(params![host_ref, bucket_ts, layout, seq, stats])?;
        if n == 0 {
            self.unalloc_seq();
        }
        Ok(n > 0)
    }

    /// Upserts on `(host, start, reason, module)`. A closed gap is never reopened by an
    /// older copy of itself. Returns whether the row changed.
    fn upsert_gap(&mut self, host_ref: i64, gap: &Gap) -> Result<bool> {
        let module = gap.module.map(Module::as_str);
        let reason = gap.reason.as_str();
        let existing: Option<(i64, Option<i64>)> = self
            .conn
            .prepare_cached(
                "SELECT id, end_ts FROM gaps
                 WHERE host_id = ?1 AND start_ts = ?2 AND reason = ?3 AND module IS ?4",
            )?
            .query_row(params![host_ref, gap.start_ms, reason, module], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        match existing {
            Some((_, end)) if end == gap.end_ms || gap.end_ms.is_none() => Ok(false),
            Some((id, _)) => {
                let seq = self.alloc_seq();
                self.conn
                    .prepare_cached("UPDATE gaps SET end_ts = ?1, seq = ?2 WHERE id = ?3")?
                    .execute(params![gap.end_ms, seq, id])?;
                Ok(true)
            }
            None => {
                let seq = self.alloc_seq();
                self.conn
                    .prepare_cached(
                        "INSERT INTO gaps (host_id, start_ts, end_ts, module, reason, seq)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    )?
                    .execute(params![
                        host_ref,
                        gap.start_ms,
                        gap.end_ms,
                        module,
                        reason,
                        seq
                    ])?;
                Ok(true)
            }
        }
    }

    fn open_gap(&mut self, host: HostId, gap: &Gap) -> Result<()> {
        let host_ref = self.host_ref(host)?;
        let open: Option<i64> = self
            .conn
            .prepare_cached(
                "SELECT id FROM gaps
                 WHERE host_id = ?1 AND reason = ?2 AND module IS ?3 AND end_ts IS NULL",
            )?
            .query_row(
                params![
                    host_ref,
                    gap.reason.as_str(),
                    gap.module.map(Module::as_str)
                ],
                |r| r.get(0),
            )
            .optional()?;
        if open.is_none() {
            self.upsert_gap(host_ref, gap)?;
        }
        Ok(())
    }

    fn close_gap(
        &mut self,
        host: HostId,
        reason: GapReason,
        module: Option<Module>,
        start_ms: Option<i64>,
        end_ms: i64,
    ) -> Result<()> {
        let host_ref = self.host_ref(host)?;
        let ids: Vec<i64> = self
            .conn
            .prepare_cached(
                "SELECT id FROM gaps
                 WHERE host_id = ?1 AND reason = ?2 AND module IS ?3 AND end_ts IS NULL",
            )?
            .query_map(
                params![host_ref, reason.as_str(), module.map(Module::as_str)],
                |r| r.get(0),
            )?
            .collect::<rusqlite::Result<_>>()?;
        if ids.is_empty()
            && let Some(start) = start_ms
        {
            // The open was lost with a batch that failed to commit (review #9).
            let gap = Gap::new(start, Some(end_ms.max(start)), module, reason)?;
            self.upsert_gap(host_ref, &gap)?;
        }
        for id in ids {
            self.close_gap_row(id, end_ms)?;
        }
        Ok(())
    }

    /// Sets `end_ts` (never before `start_ts`) and a new `seq`.
    fn close_gap_row(&mut self, id: i64, end_ms: i64) -> Result<()> {
        let seq = self.alloc_seq();
        self.conn
            .prepare_cached("UPDATE gaps SET end_ts = max(start_ts, ?1), seq = ?2 WHERE id = ?3")?
            .execute(params![end_ms, seq, id])?;
        Ok(())
    }

    /// Upserts on `(host, ts, kind)`. Returns whether the row changed.
    fn upsert_event(
        &mut self,
        host_ref: i64,
        ts_ms: i64,
        kind: &str,
        payload: &[u8],
    ) -> Result<bool> {
        let existing: Option<(i64, Vec<u8>)> = self
            .conn
            .prepare_cached(
                "SELECT id, payload FROM events WHERE host_id = ?1 AND ts = ?2 AND kind = ?3",
            )?
            .query_row(params![host_ref, ts_ms, kind], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        match existing {
            Some((_, p)) if p == payload => Ok(false),
            Some((id, _)) => {
                let seq = self.alloc_seq();
                self.conn
                    .prepare_cached("UPDATE events SET payload = ?1, seq = ?2 WHERE id = ?3")?
                    .execute(params![payload, seq, id])?;
                Ok(true)
            }
            None => {
                let seq = self.alloc_seq();
                self.conn
                    .prepare_cached(
                        "INSERT INTO events (host_id, ts, kind, payload, seq) VALUES (?1, ?2, ?3, ?4, ?5)",
                    )?
                    .execute(params![host_ref, ts_ms, kind, payload, seq])?;
                Ok(true)
            }
        }
    }

    fn write_proc_snapshot(&mut self, host: HostId, ts_ms: i64, rows: &[ProcRow]) -> Result<()> {
        let host_ref = self.host_ref(host)?;
        let mut packed = Vec::with_capacity(rows.len());
        for r in rows {
            packed.push(PackedProc {
                name_id: self.proc_name_id(host_ref, &r.name)?,
                pid: r.pid,
                cpu: r.cpu_pct,
                mem_kib: u32::try_from(r.mem_bytes / 1024).unwrap_or(u32::MAX),
                threads: r.threads,
                wakeups: r.idle_wakeups_per_s,
                energy: r.energy,
            });
        }
        let seq = self.alloc_seq();
        let n = self
            .conn
            .prepare_cached(
                "INSERT INTO proc_snap (host_id, ts, seq, blob) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (host_id, ts) DO UPDATE SET seq = excluded.seq, blob = excluded.blob
                   WHERE blob IS NOT excluded.blob",
            )?
            .execute(params![host_ref, ts_ms, seq, blob::pack_procs(&packed)])?;
        if n == 0 {
            self.unalloc_seq();
        }
        Ok(())
    }

    fn write_net_bucket(&mut self, host: HostId, bucket_ts: i64, bucket: &NetBucket) -> Result<()> {
        let host_ref = self.host_ref(host)?;
        let width = tier_width(Tier::S10)?;
        let mut fold = NetFold::default();
        fold.add_header(&blob::NetHeader {
            measured_ms: bucket
                .measured_ms
                .min(u32::try_from(width).unwrap_or(u32::MAX)),
            rx_bytes: bucket.iface_rx_bytes,
            tx_bytes: bucket.iface_tx_bytes,
            rx_pkts: bucket.iface_rx_pkts,
            tx_pkts: bucket.iface_tx_pkts,
        });
        for app in &bucket.apps {
            let id = self.net_name_id(host_ref, app.name.as_deref(), bucket_ts)?;
            fold.add_app(id, app.rx_bytes, app.tx_bytes);
        }
        let (header, rows) = fold.finish();
        self.upsert_net(
            Tier::S10,
            host_ref,
            bucket_ts,
            &blob::pack_net(&header, &rows),
        )?;
        let minute = Tier::M1.bucket_start(bucket_ts).unwrap_or(bucket_ts);
        self.recompute_net_minute(host_ref, minute)
    }

    /// Upserts a network row on `(host, bucket_ts)`. Returns whether the row changed.
    fn upsert_net(
        &mut self,
        tier: Tier,
        host_ref: i64,
        bucket_ts: i64,
        blob: &[u8],
    ) -> Result<bool> {
        let table = net_table(tier)?;
        let seq = self.alloc_seq();
        let n = self
            .conn
            .prepare_cached(&format!(
                "INSERT INTO {table} (host_id, bucket_ts, seq, blob) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (host_id, bucket_ts) DO UPDATE SET seq = excluded.seq, blob = excluded.blob
                   WHERE blob IS NOT excluded.blob"
            ))?
            .execute(params![host_ref, bucket_ts, seq, blob])?;
        if n == 0 {
            self.unalloc_seq();
        }
        Ok(n > 0)
    }

    /// Rewrites the `proc_net_1m` row of `minute` as the sum of its `proc_net_10s` rows,
    /// or deletes it when the minute has none.
    fn recompute_net_minute(&mut self, host_ref: i64, minute: i64) -> Result<()> {
        let width = tier_width(Tier::M1)?;
        let blobs: Vec<Vec<u8>> = self
            .conn
            .prepare_cached(
                "SELECT blob FROM proc_net_10s
                 WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3",
            )?
            .query_map(params![host_ref, minute, minute + width], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if blobs.is_empty() {
            self.conn
                .prepare_cached("DELETE FROM proc_net_1m WHERE host_id = ?1 AND bucket_ts = ?2")?
                .execute(params![host_ref, minute])?;
            return Ok(());
        }
        let mut fold = NetFold::default();
        for b in &blobs {
            let (header, rows) = blob::unpack_net(b)?;
            fold.add(&header, &rows);
        }
        let (header, rows) = fold.finish();
        self.upsert_net(Tier::M1, host_ref, minute, &blob::pack_net(&header, &rows))?;
        Ok(())
    }

    // --- startup --------------------------------------------------------------------------

    /// End of the last persisted bucket of `host` across every tier.
    fn last_data_end(&self, host_ref: i64) -> Result<Option<i64>> {
        let mut last = None;
        for tier in Tier::PERSISTED {
            let (table, width) = (tier_table(tier)?, tier_width(tier)?);
            let ts: Option<i64> = self.conn.query_row(
                &format!("SELECT max(bucket_ts) FROM {table} WHERE host_id = ?1"),
                [host_ref],
                |r| r.get(0),
            )?;
            last = last.max(ts.map(|t| t + width));
        }
        Ok(last)
    }

    fn begin_session(&mut self, host: HostId, now: i64) -> Result<SessionStart> {
        let host_ref = self.host_ref(host)?;
        let last_data = self.last_data_end(host_ref)?;
        let open: Vec<(i64, i64)> = self
            .conn
            .prepare("SELECT id, start_ts FROM gaps WHERE host_id = ?1 AND end_ts IS NULL")?
            .query_map([host_ref], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut out = SessionStart::default();
        // A gap left open means the previous run stopped during it. What happened after
        // the last persisted bucket is unknown, so the gap ends there (or at its own
        // start) and `app_not_running` covers the rest.
        for &(id, start) in &open {
            self.close_gap_row(id, last_data.unwrap_or(start).max(start))?;
            out.closed_gaps += 1;
        }
        let last_gap_end: Option<i64> = self.conn.query_row(
            "SELECT max(end_ts) FROM gaps WHERE host_id = ?1",
            [host_ref],
            |r| r.get(0),
        )?;
        if let Some(start) = last_data.max(last_gap_end)
            && start < now
        {
            let gap = Gap::host(start, Some(now), GapReason::AppNotRunning)?;
            self.upsert_gap(host_ref, &gap)?;
            out.app_not_running = Some(gap);
        }
        let newest_net: Option<i64> = self.conn.query_row(
            "SELECT max(bucket_ts) FROM proc_net_10s WHERE host_id = ?1",
            [host_ref],
            |r| r.get(0),
        )?;
        let width = tier_width(Tier::S10)?;
        out.net_written_to_ms = newest_net.map(|ts| ts + width);
        Ok(out)
    }

    // --- sync ingest ----------------------------------------------------------------------

    fn ingest_page(&mut self, host: HostId, page: &CursorPage) -> Result<IngestReport> {
        let host_ref = self.host_ref(host)?;
        tier_table(page.tier)?;
        let mut report = IngestReport::default();
        let mut layouts: HashMap<u32, (u32, usize)> = HashMap::new();
        for l in &page.layouts {
            let series: Arc<[SeriesKey]> = l.series.clone().into();
            let id = self.layout_id(host_ref, &series)?;
            layouts.insert(l.layout_no, (id, series.len()));
        }
        for row in &page.rows {
            let &(layout, n) = layouts
                .get(&row.layout_no)
                .ok_or(StoreError::PageLayout(row.layout_no))?;
            check_stats(n, row.stats.len())?;
            if self.upsert_bucket(
                host_ref,
                page.tier,
                row.bucket_ts,
                layout,
                &blob::pack_f32s(&row.stats),
            )? {
                report.rows_written += 1;
            } else {
                report.rows_unchanged += 1;
            }
        }
        for g in &page.gaps {
            // A gap for a module this build does not know blanks nothing it can draw
            // (D-040); a malformed gap from a peer is dropped rather than stalling sync.
            if g.gap.module == Some(Module::Unknown) || g.gap.validate().is_err() {
                report.gaps_skipped += 1;
                continue;
            }
            if self.upsert_gap(host_ref, &g.gap)? {
                report.gaps_written += 1;
            }
        }
        for e in &page.events {
            if self.upsert_event(host_ref, e.ts_ms, &e.kind, &e.payload)? {
                report.events_written += 1;
            }
        }
        self.conn
            .prepare_cached(
                "INSERT INTO cursors (host_id, tier, epoch, seq, kinds) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (host_id, tier) DO UPDATE
                   SET epoch = excluded.epoch, seq = excluded.seq, kinds = excluded.kinds",
            )?
            .execute(params![
                host_ref,
                page.tier.as_str(),
                page.epoch.hyphenated().to_string(),
                page.last_seq,
                page.kinds.to_stored()
            ])?;
        Ok(report)
    }

    fn record_truncation(
        &mut self,
        host: HostId,
        tier: Tier,
        earliest: i64,
    ) -> Result<Option<Gap>> {
        let host_ref = self.host_ref(host)?;
        tier_width(tier)?;
        self.conn.execute(
            "DELETE FROM cursors WHERE host_id = ?1 AND tier = ?2",
            params![host_ref, tier.as_str()],
        )?;
        // Gaps are host-wide, not per tier. The 10 s tier is a 24 h window that a
        // controller offline for a day always falls behind; a gap for that would blank
        // minute data that is actually there. So only M1 and M15 truncation write one
        // (D-041), from the end of the newest minute or 15-minute bucket held: what one
        // of them still covers is not missing.
        if !matches!(tier, Tier::M1 | Tier::M15) {
            return Ok(None);
        }
        let mut last: Option<i64> = None;
        for t in [Tier::M1, Tier::M15] {
            let (table, width) = (tier_table(t)?, tier_width(t)?);
            let ts: Option<i64> = self.conn.query_row(
                &format!("SELECT max(bucket_ts) FROM {table} WHERE host_id = ?1"),
                [host_ref],
                |r| r.get(0),
            )?;
            last = last.max(ts.map(|t| t + width));
        }
        match last {
            Some(start) if start < earliest => {
                let gap = Gap::host(start, Some(earliest), GapReason::Truncated)?;
                self.upsert_gap(host_ref, &gap)?;
                Ok(Some(gap))
            }
            _ => Ok(None),
        }
    }

    // --- pruning --------------------------------------------------------------------------

    fn host_refs(&self) -> Result<Vec<i64>> {
        Ok(self
            .conn
            .prepare("SELECT id FROM hosts")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn prune(&mut self, now: i64, retention: Retention) -> Result<PruneReport> {
        self.commit()?;
        let mut report = PruneReport::default();
        let history_cut = now - retention.history_ms;
        // Whole 15-minute buckets only, so a bucket is never split across two passes.
        let roll_cut = Tier::M15
            .bucket_start(now - retention.m1_keep_ms())
            .unwrap_or(now - retention.m1_keep_ms());
        for host_ref in self.host_refs()? {
            let snaps = self.roll_down(
                host_ref,
                "proc_snap",
                "ts",
                Tier::M1,
                now - retention.proc_snap_ms,
                ROLL_DOWN_SNAPSHOTS_MS,
                |s, start, end| s.roll_down_snapshots(host_ref, start, end),
            )?;
            report.proc_top_rows += snaps.written;
            report.proc_snaps_rolled += snaps.rolled;

            // Whole minutes, so a minute's `proc_net_1m` row never outlives part of the
            // 10 s rows it was summed from (reads take one or the other per minute).
            let net_s10_cut = now - retention.proc_snap_ms.min(retention.history_ms);
            let net_s10_cut = Tier::M1.bucket_start(net_s10_cut).unwrap_or(net_s10_cut);
            let (n, _) = self.delete_batched("proc_net_10s", "bucket_ts", host_ref, net_s10_cut)?;
            report.net_rows += n;

            let s10_cut = now - retention.s10_ms;
            let (n, max) = self.delete_batched("tier_10s", "bucket_ts", host_ref, s10_cut)?;
            report.s10_rows += n;
            self.mark_pruned(host_ref, Tier::S10, max, s10_cut)?;

            // Past retention first, so the roll-down never folds rows about to go.
            let (m1, a) = self.delete_batched("tier_1m", "bucket_ts", host_ref, history_cut)?;
            let (m15, d) = self.delete_batched("tier_15m", "bucket_ts", host_ref, history_cut)?;
            // Only closed gaps that ended before the cutoff.
            let (gaps, b) = self.delete_batched("gaps", "end_ts", host_ref, history_cut)?;
            let (events, c) = self.delete_batched("events", "ts", host_ref, history_cut)?;
            let (top, _) =
                self.delete_batched("proc_top_1m", "bucket_ts", host_ref, history_cut)?;
            let (top15, _) =
                self.delete_batched("proc_top_15m", "bucket_ts", host_ref, history_cut)?;
            let (net1, _) =
                self.delete_batched("proc_net_1m", "bucket_ts", host_ref, history_cut)?;
            let (net15, _) =
                self.delete_batched("proc_net_15m", "bucket_ts", host_ref, history_cut)?;
            report.net_rows += net1 + net15;
            report.m1_rows += m1;
            report.m15_rows += m15;
            report.gaps += gaps;
            report.events += events;
            report.proc_top_rows += top + top15;
            self.mark_pruned(host_ref, Tier::M1, a.max(b).max(c), history_cut)?;
            self.mark_pruned(host_ref, Tier::M15, d, history_cut)?;

            let minutes = self.roll_down(
                host_ref,
                "tier_1m",
                "bucket_ts",
                Tier::M15,
                roll_cut,
                ROLL_DOWN_MINUTES_MS,
                |s, start, end| s.roll_down_minutes(host_ref, start, end),
            )?;
            report.m15_written += minutes.written;
            report.m1_rolled += minutes.rolled;
            self.mark(host_ref, db::M1_ROLLED, minutes.max_seq, roll_cut)?;

            let tops = self.roll_down(
                host_ref,
                "proc_top_1m",
                "bucket_ts",
                Tier::M15,
                roll_cut,
                ROLL_DOWN_MINUTES_MS,
                |s, start, end| s.roll_down_top_minutes(host_ref, start, end),
            )?;
            report.proc_top_rolled += tops.rolled;

            let nets = self.roll_down(
                host_ref,
                "proc_net_1m",
                "bucket_ts",
                Tier::M15,
                roll_cut,
                ROLL_DOWN_MINUTES_MS,
                |s, start, end| s.roll_down_net_minutes(host_ref, start, end),
            )?;
            report.net_rolled += nets.rolled;
        }
        self.incremental_vacuum()?;
        self.checkpoint_truncate()?;
        let size = crate::size_on_disk(&self.path)?;
        if size > retention.max_bytes {
            report.cap_trim = Some(self.trim_to_cap(now, retention, size)?);
        }
        report.size_bytes = crate::size_on_disk(&self.path)?;
        Ok(report)
    }

    /// Deletes the oldest history on every host, a slice at a time, until the live pages
    /// fit in the low-water mark or only the last day is left. Each slice goes from the
    /// start of history to a cutoff, so the trimmed span is "before history began", never
    /// a hole in the middle; the `pruned` marks move to the cutoff so a cursor behind it
    /// gets `Truncated` (D-041).
    fn trim_to_cap(&mut self, now: i64, retention: Retention, size_before: u64) -> Result<CapTrim> {
        let low_water = retention.low_water_bytes();
        let floor = Tier::M15
            .bucket_start(now - CAP_TRIM_KEEP_MS)
            .unwrap_or(now - CAP_TRIM_KEEP_MS);
        let mut trim = CapTrim {
            size_before,
            ..CapTrim::default()
        };
        let mut used = self.used_bytes()?;
        for _ in 0..CAP_TRIM_MAX_ROUNDS {
            if used <= low_water {
                break;
            }
            let (oldest, newest): (Option<i64>, Option<i64>) = self.conn.query_row(
                "SELECT min(t), max(t) FROM (
                   SELECT min(bucket_ts) AS t FROM tier_1m UNION ALL
                   SELECT max(bucket_ts) FROM tier_1m UNION ALL
                   SELECT min(bucket_ts) FROM tier_15m UNION ALL
                   SELECT max(bucket_ts) FROM tier_15m
                 )",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let (Some(oldest), Some(newest)) = (oldest, newest) else {
                break;
            };
            trim.earliest_ts_ms = trim.earliest_ts_ms.max(oldest);
            // Remove the share of the span that is over the mark. It undershoots: S10,
            // snapshots and the recent minutes do not shrink with it, and the oldest span
            // is 15-minute rows, which cost a fifteenth of minutes. That only costs more
            // rounds.
            let span = (newest - oldest).max(0) + 60_000;
            let excess = used - low_water;
            let share = i128::from(span) * i128::from(excess) / i128::from(used.max(1));
            let step = i64::try_from(share)
                .unwrap_or(span)
                .max(CAP_TRIM_MIN_STEP_MS);
            let cutoff = Tier::M15
                .bucket_start(oldest + step)
                .unwrap_or(oldest + step)
                .min(floor);
            if cutoff <= oldest {
                break;
            }
            for host_ref in self.host_refs()? {
                let (m15, e) = self.delete_batched("tier_15m", "bucket_ts", host_ref, cutoff)?;
                let (m1, a) = self.delete_batched("tier_1m", "bucket_ts", host_ref, cutoff)?;
                let (gaps, b) = self.delete_batched("gaps", "end_ts", host_ref, cutoff)?;
                let (events, c) = self.delete_batched("events", "ts", host_ref, cutoff)?;
                let (top15, _) =
                    self.delete_batched("proc_top_15m", "bucket_ts", host_ref, cutoff)?;
                let (top, _) = self.delete_batched("proc_top_1m", "bucket_ts", host_ref, cutoff)?;
                let (s10, d) = self.delete_batched("tier_10s", "bucket_ts", host_ref, cutoff)?;
                let (snaps, _) = self.delete_batched("proc_snap", "ts", host_ref, cutoff)?;
                for table in ["proc_net_15m", "proc_net_1m", "proc_net_10s"] {
                    let (n, _) = self.delete_batched(table, "bucket_ts", host_ref, cutoff)?;
                    trim.net_rows += n;
                }
                self.mark_pruned(host_ref, Tier::M1, a.max(b).max(c), cutoff)?;
                self.mark_pruned(host_ref, Tier::M15, e, cutoff)?;
                self.mark_pruned(host_ref, Tier::S10, d, cutoff)?;
                trim.m1_rows += m1;
                trim.m15_rows += m15;
                trim.gaps += gaps;
                trim.events += events;
                trim.proc_rows += top + top15 + snaps;
                trim.s10_rows += s10;
            }
            trim.earliest_ts_ms = cutoff;
            self.incremental_vacuum()?;
            used = self.used_bytes()?;
        }
        trim.cap_met = used <= low_water;
        self.checkpoint_truncate()?;
        Ok(trim)
    }

    /// Deletes rows of `table` with `ts_col < cutoff` in batches, one transaction each.
    /// Returns how many went and the highest `seq` among them.
    fn delete_batched(
        &mut self,
        table: &str,
        ts_col: &str,
        host_ref: i64,
        cutoff: i64,
    ) -> Result<(u64, Option<i64>)> {
        let batch = self.prune_batch;
        let sql = format!(
            "DELETE FROM {table} WHERE rowid IN (
               SELECT rowid FROM {table} WHERE host_id = ?1 AND {ts_col} < ?2 LIMIT {batch}
             ) RETURNING seq"
        );
        let (mut total, mut max_seq) = (0, None);
        loop {
            let (n, max) = self.apply(|s| {
                let mut stmt = s.conn.prepare_cached(&sql)?;
                let mut rows = stmt.query(params![host_ref, cutoff])?;
                let (mut n, mut max) = (0u64, None::<i64>);
                while let Some(r) = rows.next()? {
                    n += 1;
                    max = max.max(Some(r.get(0)?));
                }
                Ok((n, max))
            })?;
            self.commit()?;
            total += n;
            max_seq = max_seq.max(max);
            if n < batch {
                return Ok((total, max_seq));
            }
            self.yield_to_queue()?;
        }
    }

    fn mark_pruned(&mut self, host_ref: i64, tier: Tier, seq: Option<i64>, ts: i64) -> Result<()> {
        self.mark(host_ref, tier.as_str(), seq, ts)
    }

    /// Moves the `pruned` row `key` of `host_ref` forward to `(seq, ts)`; never back.
    fn mark(&mut self, host_ref: i64, key: &str, seq: Option<i64>, ts: i64) -> Result<()> {
        let Some(seq) = seq else {
            return Ok(());
        };
        self.apply(|s| {
            s.conn.execute(
                "INSERT INTO pruned (host_id, tier, seq, ts) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (host_id, tier) DO UPDATE
                   SET seq = max(seq, excluded.seq), ts = max(ts, excluded.ts)",
                params![host_ref, key, seq, ts],
            )?;
            Ok(())
        })?;
        self.commit()
    }

    /// Rolls the rows of `table` older than `cutoff` into a coarser table, oldest first,
    /// `span_ms` of them per transaction, handling queued work between transactions.
    /// `cutoff` and each span start are aligned to `bucket`'s width, so a coarse bucket is
    /// never split across two passes. `chunk` folds and deletes the rows of one span; it
    /// must delete every row of `table` in it, or this would not end.
    #[allow(clippy::too_many_arguments)] // Each is one knob of the roll-down; a struct would only rename them.
    fn roll_down(
        &mut self,
        host_ref: i64,
        table: &str,
        ts_col: &str,
        bucket: Tier,
        cutoff: i64,
        span_ms: i64,
        mut chunk: impl FnMut(&mut State, i64, i64) -> Result<Rolled>,
    ) -> Result<Rolled> {
        let cutoff = bucket.bucket_start(cutoff).unwrap_or(cutoff);
        let mut total = Rolled::default();
        loop {
            let first: Option<i64> = self.conn.query_row(
                &format!("SELECT min({ts_col}) FROM {table} WHERE host_id = ?1 AND {ts_col} < ?2"),
                params![host_ref, cutoff],
                |r| r.get(0),
            )?;
            let Some(first) = first else {
                return Ok(total);
            };
            let start = bucket.bucket_start(first).unwrap_or(first);
            let end = (start + span_ms).min(cutoff);
            let done = self.apply(|s| chunk(s, start, end))?;
            self.commit()?;
            total.written += done.written;
            total.rolled += done.rolled;
            total.max_seq = total.max_seq.max(done.max_seq);
            self.yield_to_queue()?;
        }
    }

    /// Rolls the `proc_snap` rows in `[start, end)` into `proc_top_1m` (top 5 per minute
    /// by mean CPU) and deletes them.
    fn roll_down_snapshots(&mut self, host_ref: i64, start: i64, end: i64) -> Result<Rolled> {
        let snaps: Vec<(i64, Vec<u8>)> = self
            .conn
            .prepare_cached(
                "SELECT ts, blob FROM proc_snap WHERE host_id = ?1 AND ts >= ?2 AND ts < ?3 ORDER BY ts",
            )?
            .query_map(params![host_ref, start, end], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut by_minute: BTreeMap<i64, Vec<Vec<PackedProc>>> = BTreeMap::new();
        for (ts, b) in &snaps {
            let minute = Tier::M1.bucket_start(*ts).unwrap_or(*ts);
            by_minute
                .entry(minute)
                .or_default()
                .push(blob::unpack_procs(b)?);
        }
        let mut written = 0;
        for (minute, group) in by_minute {
            let top = top_of_group(&group);
            let seq = self.alloc_seq();
            self.conn
                .prepare_cached(
                    "INSERT INTO proc_top_1m (host_id, bucket_ts, seq, blob) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (host_id, bucket_ts) DO UPDATE SET seq = excluded.seq, blob = excluded.blob",
                )?
                .execute(params![host_ref, minute, seq, blob::pack_procs(&top)])?;
            written += 1;
        }
        let deleted = self
            .conn
            .prepare_cached("DELETE FROM proc_snap WHERE host_id = ?1 AND ts >= ?2 AND ts < ?3")?
            .execute(params![host_ref, start, end])?;
        Ok(Rolled {
            written,
            rolled: deleted as u64,
            max_seq: None,
        })
    }

    /// Rolls the `tier_1m` rows in `[start, end)` (15-minute boundaries) into `tier_15m`,
    /// one row per 15-minute bucket and layout ([`StatsFold`]), and deletes them. A bucket
    /// that already has a row for the layout keeps it: buckets are rolled whole, so a
    /// second fold could only come from minutes that arrived after the first one, and
    /// would replace a full bucket with a fraction of it. A row whose length does not fit
    /// its layout is dropped with a warning rather than stopping every later prune.
    fn roll_down_minutes(&mut self, host_ref: i64, start: i64, end: i64) -> Result<Rolled> {
        let rows: Vec<(i64, u32, Vec<u8>)> = self
            .conn
            .prepare_cached(
                "SELECT bucket_ts, layout_id, blob FROM tier_1m
                 WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3 ORDER BY bucket_ts",
            )?
            .query_map(params![host_ref, start, end], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut folds: BTreeMap<(i64, u32), StatsFold> = BTreeMap::new();
        for (ts, layout, b) in &rows {
            let stats = match blob::unpack_f32s(b) {
                Ok(v) if v.len().is_multiple_of(3) => v,
                _ => {
                    tracing::warn!(ts, layout, "store: malformed minute row not rolled down");
                    continue;
                }
            };
            let bucket = Tier::M15.bucket_start(*ts).unwrap_or(*ts);
            let fold = folds
                .entry((bucket, *layout))
                .or_insert_with(|| StatsFold::new(stats.len() / 3));
            if !fold.add(&stats) {
                tracing::warn!(
                    ts,
                    layout,
                    "store: minute row of another width not rolled down"
                );
            }
        }
        let mut written = 0;
        for ((bucket, layout), fold) in folds {
            let seq = self.alloc_seq();
            let n = self
                .conn
                .prepare_cached(
                    "INSERT INTO tier_15m (host_id, bucket_ts, layout_id, seq, blob)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (host_id, bucket_ts, layout_id) DO NOTHING",
                )?
                .execute(params![
                    host_ref,
                    bucket,
                    layout,
                    seq,
                    blob::pack_f32s(&fold.finish())
                ])?;
            if n == 0 {
                self.unalloc_seq();
            }
            written += n as u64;
        }
        let (rolled, max_seq) =
            self.delete_span_returning_seq("tier_1m", "bucket_ts", host_ref, start, end)?;
        Ok(Rolled {
            written,
            rolled,
            max_seq,
        })
    }

    /// Rolls the `proc_top_1m` rows in `[start, end)` (15-minute boundaries) into
    /// `proc_top_15m`, the top 5 by mean CPU over the minutes present, and deletes them. An
    /// existing row is kept, as in [`State::roll_down_minutes`].
    fn roll_down_top_minutes(&mut self, host_ref: i64, start: i64, end: i64) -> Result<Rolled> {
        let rows: Vec<(i64, Vec<u8>)> = self
            .conn
            .prepare_cached(
                "SELECT bucket_ts, blob FROM proc_top_1m
                 WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3 ORDER BY bucket_ts",
            )?
            .query_map(params![host_ref, start, end], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut groups: BTreeMap<i64, Vec<Vec<PackedProc>>> = BTreeMap::new();
        for (ts, b) in &rows {
            let bucket = Tier::M15.bucket_start(*ts).unwrap_or(*ts);
            match blob::unpack_procs(b) {
                Ok(procs) => groups.entry(bucket).or_default().push(procs),
                Err(e) => tracing::warn!(ts, "store: process minute not rolled down: {e}"),
            }
        }
        let mut written = 0;
        for (bucket, group) in groups {
            let seq = self.alloc_seq();
            let n = self
                .conn
                .prepare_cached(
                    "INSERT INTO proc_top_15m (host_id, bucket_ts, seq, blob) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (host_id, bucket_ts) DO NOTHING",
                )?
                .execute(params![
                    host_ref,
                    bucket,
                    seq,
                    blob::pack_procs(&top_of_group(&group))
                ])?;
            if n == 0 {
                self.unalloc_seq();
            }
            written += n as u64;
        }
        let (rolled, max_seq) =
            self.delete_span_returning_seq("proc_top_1m", "bucket_ts", host_ref, start, end)?;
        Ok(Rolled {
            written,
            rolled,
            max_seq,
        })
    }

    /// Rolls the `proc_net_1m` rows in `[start, end)` (15-minute boundaries) into
    /// `proc_net_15m` by exact sums ([`NetFold`]) and deletes them. An existing row is
    /// kept, as in [`State::roll_down_minutes`]; a malformed row is dropped with a warning.
    fn roll_down_net_minutes(&mut self, host_ref: i64, start: i64, end: i64) -> Result<Rolled> {
        let rows: Vec<(i64, Vec<u8>)> = self
            .conn
            .prepare_cached(
                "SELECT bucket_ts, blob FROM proc_net_1m
                 WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3 ORDER BY bucket_ts",
            )?
            .query_map(params![host_ref, start, end], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut folds: BTreeMap<i64, NetFold> = BTreeMap::new();
        for (ts, b) in &rows {
            match blob::unpack_net(b) {
                Ok((header, apps)) => folds
                    .entry(Tier::M15.bucket_start(*ts).unwrap_or(*ts))
                    .or_default()
                    .add(&header, &apps),
                Err(e) => tracing::warn!(ts, "store: network minute not rolled down: {e}"),
            }
        }
        let mut written = 0;
        for (bucket, fold) in folds {
            let (header, apps) = fold.finish();
            let seq = self.alloc_seq();
            let n = self
                .conn
                .prepare_cached(
                    "INSERT INTO proc_net_15m (host_id, bucket_ts, seq, blob) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (host_id, bucket_ts) DO NOTHING",
                )?
                .execute(params![host_ref, bucket, seq, blob::pack_net(&header, &apps)])?;
            if n == 0 {
                self.unalloc_seq();
            }
            written += n as u64;
        }
        let (rolled, max_seq) =
            self.delete_span_returning_seq("proc_net_1m", "bucket_ts", host_ref, start, end)?;
        Ok(Rolled {
            written,
            rolled,
            max_seq,
        })
    }

    /// Deletes `host_ref`'s rows of `table` with `ts_col` in `[start, end)`; returns how
    /// many and the highest `seq` among them.
    fn delete_span_returning_seq(
        &mut self,
        table: &str,
        ts_col: &str,
        host_ref: i64,
        start: i64,
        end: i64,
    ) -> Result<(u64, Option<i64>)> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "DELETE FROM {table} WHERE host_id = ?1 AND {ts_col} >= ?2 AND {ts_col} < ?3
             RETURNING seq"
        ))?;
        let mut rows = stmt.query(params![host_ref, start, end])?;
        let (mut n, mut max) = (0u64, None::<i64>);
        while let Some(r) = rows.next()? {
            n += 1;
            max = max.max(Some(r.get(0)?));
        }
        Ok((n, max))
    }

    fn discard_from(&mut self, host: HostId, from: i64) -> Result<()> {
        let host_ref = self.host_ref(host)?;
        for (table, col) in [
            ("tier_10s", "bucket_ts"),
            ("tier_1m", "bucket_ts"),
            ("tier_15m", "bucket_ts"),
            ("proc_snap", "ts"),
            ("proc_top_1m", "bucket_ts"),
            ("proc_top_15m", "bucket_ts"),
            ("proc_net_10s", "bucket_ts"),
            ("proc_net_1m", "bucket_ts"),
            ("proc_net_15m", "bucket_ts"),
            ("events", "ts"),
            ("gaps", "start_ts"),
        ] {
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE host_id = ?1 AND {col} >= ?2"),
                params![host_ref, from],
            )?;
        }
        // The minute holding `from` summed 10 s rows that just went.
        let minute = Tier::M1.bucket_start(from).unwrap_or(from);
        if minute < from {
            self.recompute_net_minute(host_ref, minute)?;
        }
        let cut: Vec<i64> = self
            .conn
            .prepare("SELECT id FROM gaps WHERE host_id = ?1 AND end_ts > ?2")?
            .query_map(params![host_ref, from], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for id in cut {
            self.close_gap_row(id, from)?;
        }
        Ok(())
    }

    /// [`Writer::discard_from`]. A batch before it that fails to commit is lost as any
    /// other (its span recorded as `write_failed`), and the discard still runs.
    fn discard_committed(&mut self, host: HostId, from: i64) -> Result<()> {
        self.commit_logged();
        self.apply(|s| s.discard_from(host, from))?;
        // A `write_failed` gap rides the next commit: it must not cover the span that was
        // just discarded.
        for spans in [&mut self.batch_span, &mut self.lost] {
            if let Some((start, end)) = spans.get(&host).copied() {
                if start >= from {
                    spans.remove(&host);
                } else {
                    spans.insert(host, (start, end.min(from)));
                }
            }
        }
        self.commit()
    }

    fn clear_host(&mut self, host: HostId, now: i64) -> Result<()> {
        self.commit()?;
        self.apply(|s| {
            let host_ref = s.host_ref(host)?;
            // Everything up to now is gone: a remote cursor into it must resync.
            let last = s.next_seq - 1;
            for tier in Tier::PERSISTED {
                s.conn.execute(
                    "INSERT INTO pruned (host_id, tier, seq, ts) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (host_id, tier) DO UPDATE SET seq = excluded.seq, ts = excluded.ts",
                    params![host_ref, tier.as_str(), last, now],
                )?;
            }
            for table in [
                "tier_10s",
                "tier_1m",
                "tier_15m",
                "gaps",
                "events",
                "proc_snap",
                "proc_top_1m",
                "proc_top_15m",
                "proc_net_10s",
                "proc_net_1m",
                "proc_net_15m",
                "cursors",
            ] {
                s.conn
                    .execute(&format!("DELETE FROM {table} WHERE host_id = ?1"), [host_ref])?;
            }
            Ok(())
        })?;
        self.commit()?;
        self.incremental_vacuum()?;
        Ok(())
    }
}

/// What one roll-down pass did.
#[derive(Clone, Copy, Debug, Default)]
struct Rolled {
    /// Coarse rows written.
    written: u64,
    /// Fine rows folded and deleted.
    rolled: u64,
    /// Highest `seq` of the deleted rows, where the caller marks it.
    max_seq: Option<i64>,
}

/// Top processes of one bucket by mean CPU across its member lists: the snapshots of a
/// minute, or the minutes of 15 minutes. A process missing from a list was below that
/// list's cut (top 30 per snapshot, top 5 per minute), and counts as 0 there: an
/// approximation that can only understate it.
fn top_of_group(snapshots: &[Vec<PackedProc>]) -> Vec<PackedProc> {
    let n = snapshots.len().max(1) as f32;
    let mut acc: HashMap<u32, PackedProc> = HashMap::new();
    for snap in snapshots {
        for p in snap {
            let e = acc.entry(p.name_id).or_insert(PackedProc {
                name_id: p.name_id,
                pid: p.pid,
                cpu: 0.0,
                mem_kib: 0,
                threads: 0,
                wakeups: 0.0,
                energy: 0.0,
            });
            e.pid = p.pid;
            e.cpu += p.cpu;
            e.mem_kib = e.mem_kib.max(p.mem_kib);
            e.threads = e.threads.max(p.threads);
            e.wakeups += p.wakeups;
            e.energy += p.energy;
        }
    }
    let mut rows: Vec<PackedProc> = acc
        .into_values()
        .map(|mut p| {
            p.cpu /= n;
            p.wakeups /= n;
            p.energy /= n;
            p
        })
        .collect();
    rows.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(a.name_id.cmp(&b.name_id)));
    rows.truncate(TOP_PER_MINUTE);
    rows
}

fn log_queued(what: &str, res: Result<()>) {
    if let Err(e) = res {
        tracing::warn!("store: queued {what} write failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(name_id: u32, cpu: f32) -> PackedProc {
        PackedProc {
            name_id,
            pid: name_id as i32,
            cpu,
            mem_kib: 10,
            threads: 1,
            wakeups: 0.0,
            energy: 0.0,
        }
    }

    #[test]
    fn top_of_minute_averages_over_all_snapshots() {
        let snaps = vec![
            vec![proc(1, 90.0), proc(2, 10.0)],
            vec![proc(2, 50.0)],
            vec![proc(3, 5.0), proc(4, 4.0), proc(5, 3.0), proc(6, 2.0)],
        ];
        let top = top_of_group(&snaps);
        let ids: Vec<u32> = top.iter().map(|p| p.name_id).collect();
        assert_eq!(ids, [1, 2, 3, 4, 5], "top 5 by mean cpu");
        assert_eq!(top[0].cpu, 30.0, "90 in one of three snapshots");
        assert_eq!(top[1].cpu, 20.0);
    }
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

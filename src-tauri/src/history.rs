//! The history store as the shell sees it: opened at launch, or absent when it cannot be
//! opened or cannot take the local host (live-only mode, `history_unavailable` with a
//! reason); replaced by `reset_history`; a small pool of read connections for commands.
//! Pruning, the low-disk check and the health they produce are
//! [`kelvo_engine::Housekeeping`]; this starts it on the open store and maps its health to
//! the IPC type.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use kelvo_engine::{Housekeeping, HousekeepingHandle, Schedule};
use kelvo_schema::{HostRecord, Labels, MetricId, SeriesKey, Tier};
use kelvo_store::{
    BucketRow, ExportQuery, HistoryQuery, HistoryResult, ProcResolution, Reader, Retention, Store,
    StoreConfig, StoreError, TierChoice, Writer,
};

use crate::error::{CommandError, HistoryUnavailableReason};
use crate::ipc::{
    BatteryHour, ExportRequest, HeatmapDay, HeatmapMetric, HeatmapRequest, HistoryHealth,
    HistoryPage, HistoryPoint, HistoryRequest, HistorySeries, NetworkByApp, NetworkSpan,
    ProcessResolution, ProcessesAt, StoredProcess, TierRequest,
};

/// File name of the history database in the app data directory (v1-local-monitor.md 6.5).
pub const DB_FILE: &str = "history.sqlite";

/// The longest range `query_network_by_app` reads: the longest history retention. A
/// longer one could only add unrecorded time, at the cost of reading every row.
pub(crate) const MAX_NET_SPAN_MS: i64 = {
    let days = kelvo_schema::settings::HistorySettings::RETENTION_DAYS;
    days[days.len() - 1] as i64 * Retention::DAY_MS
};

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

    fn error(&self) -> CommandError {
        CommandError::HistoryUnavailable {
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
/// runtime: [`History::register_host`] gives up on a store it cannot write the local host
/// into, and [`History::reset`] replaces the file.
pub struct History {
    inner: Arc<Mutex<Inner>>,
    path: PathBuf,
    health: Arc<Mutex<kelvo_engine::HistoryHealth>>,
}

fn open_store(path: &Path) -> Result<Store, StoreError> {
    let store = Store::open(StoreConfig::new(path))?;
    tracing::info!(path = %path.display(), epoch = %store.epoch(), "history store opened");
    Ok(store)
}

impl History {
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
        lock_inner(&self.inner)
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
        (*self.health.lock().unwrap_or_else(|e| e.into_inner())).into()
    }

    /// Forgets the trim after the history was cleared. Returns the new health when it
    /// changed, for `history-health-changed`.
    pub fn clear_trim(&self) -> Option<HistoryHealth> {
        let mut h = self.health.lock().unwrap_or_else(|e| e.into_inner());
        let next = h.without_trim();
        (next != *h).then(|| {
            *h = next;
            next.into()
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

    pub fn writer(&self) -> Result<Writer, CommandError> {
        let inner = self.inner();
        inner.writer.clone().ok_or_else(|| inner.error())
    }

    /// `query_network_by_app`: the stored tiers plus `recent` (the engine's in-memory
    /// buckets, which replace stored rows of the same bucket), split into apps, overhead
    /// and the remainder. `recent` is called after the stored rows are read, so its
    /// copies are never older than them; `complete_to_ms` comes from the same call and
    /// stops where the engine's buckets stop being final. With the store unavailable,
    /// the ring alone answers (live-only). Blocking.
    pub fn network_by_app(
        &self,
        host: kelvo_schema::HostId,
        from_ms: i64,
        to_ms: i64,
        recent: impl FnOnce() -> kelvo_engine::RecentNet,
    ) -> Result<NetworkByApp, CommandError> {
        if to_ms < from_ms {
            return Err(CommandError::InvalidArgument {
                message: format!("the range ends ({to_ms}) before it starts ({from_ms})"),
            });
        }
        if to_ms - from_ms > MAX_NET_SPAN_MS {
            return Err(CommandError::InvalidArgument {
                message: format!(
                    "the range is {} days long; history keeps at most {} days",
                    (to_ms - from_ms) / Retention::DAY_MS,
                    MAX_NET_SPAN_MS / Retention::DAY_MS
                ),
            });
        }
        let mut recent = Some(recent);
        let mut complete_to_ms = None;
        let mut take = || {
            let r = recent.take().map(|f| f()).unwrap_or_default();
            complete_to_ms = r.complete_to_ms;
            r.buckets
        };
        // The store can close between a check and the read, so the read's own
        // `history_unavailable` is what sends this to the ring alone.
        let read = match self.read(|r| r.net_by_app_after(host, from_ms, to_ms, &mut take)) {
            Err(CommandError::HistoryUnavailable { .. }) => {
                kelvo_store::net_by_app_recent(from_ms, to_ms, &take())?
            }
            read => read?,
        };
        let mut out: NetworkByApp = kelvo_engine::attribute(read).into();
        // The engine always has an edge once it has started; before that, nothing is
        // promised final.
        out.complete_to_ms = out
            .complete_to_ms
            .min(complete_to_ms.unwrap_or(out.from_ms));
        Ok(out)
    }

    /// `query_history`: the stored tier plus `recent` (the engine's bucket rows since its
    /// last commit and its open buckets, which replace stored rows of the same bucket and
    /// layout), so history answers through now (D-092). `hold(key, bucket_ms)` is each
    /// series' `hold_ms`. With the store unavailable, the recent rows alone answer
    /// (live-only). Blocking.
    pub fn history(
        &self,
        request: &HistoryRequest,
        recent: impl Fn() -> Vec<BucketRow>,
        hold: impl Fn(&SeriesKey, i64) -> i64,
    ) -> Result<HistoryPage, CommandError> {
        let query = request.to_query()?;
        Ok(history_page(
            self.history_through_now(&query, recent)?,
            hold,
        ))
    }

    /// [`Reader::history_after`] with `recent`, called after the stored rows are read so
    /// its copies are never older than them; the recent rows alone when the store is
    /// unavailable. The store can close between a check and the read, so the read's own
    /// `history_unavailable` is what sends this to the recent rows.
    fn history_through_now(
        &self,
        query: &HistoryQuery,
        recent: impl Fn() -> Vec<BucketRow>,
    ) -> Result<HistoryResult, CommandError> {
        match self.read(|r| r.history_after(query, &recent)) {
            Err(CommandError::HistoryUnavailable { .. }) => {
                Ok(kelvo_store::history_recent(query, &recent())?)
            }
            read => read,
        }
    }

    /// `query_series_stats`: each unlabelled metric of `metrics` over `[from_ms, to_ms)`
    /// from its rollups, through now as `query_history` reads them: the span-weighted
    /// average (D-092), the largest sample, and the average times the time measured (bytes
    /// for a bytes-per-second metric). A bucket counts the time it covers, cut at
    /// `now_ms`, less any gap inside it that applies to the metric's module; a bucket with
    /// no reading counts nothing. The tier is the one `query_history` would pick.
    /// Blocking.
    pub fn series_stats(
        &self,
        host: kelvo_schema::HostId,
        metrics: &[String],
        from_ms: i64,
        to_ms: i64,
        now_ms: i64,
        recent: impl Fn() -> Vec<BucketRow>,
    ) -> Result<RangeStats, CommandError> {
        if to_ms < from_ms {
            return Err(CommandError::InvalidArgument {
                message: format!("the range ends ({to_ms}) before it starts ({from_ms})"),
            });
        }
        if to_ms - from_ms > MAX_NET_SPAN_MS {
            return Err(CommandError::InvalidArgument {
                message: format!(
                    "the range is {} days long; history keeps at most {} days",
                    (to_ms - from_ms) / Retention::DAY_MS,
                    MAX_NET_SPAN_MS / Retention::DAY_MS
                ),
            });
        }
        let catalog = kelvo_schema::Catalog::builtin();
        let defs = metrics
            .iter()
            .map(|m| match catalog.get(m) {
                Some(d) if d.label_keys.is_empty() => Ok(d),
                Some(_) => Err(CommandError::InvalidArgument {
                    message: format!("{m} has labels; series stats take unlabelled metrics"),
                }),
                None => Err(CommandError::InvalidArgument {
                    message: format!("{m} is not in the catalog"),
                }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let finest = Tier::S10.bucket_ms().unwrap_or(10_000);
        let query = HistoryQuery {
            host,
            selectors: defs
                .iter()
                .map(|d| kelvo_schema::SeriesSelector {
                    metric: d.id.clone(),
                    labels: Labels::new(),
                })
                .collect(),
            from_ms,
            to_ms,
            tier: TierChoice::Auto,
            // One point per bucket of the finest tier: no merging.
            max_points: u32::try_from((to_ms - from_ms) / finest + 2).unwrap_or(u32::MAX),
        };
        let read = self.history_through_now(&query, recent)?;
        Ok(range_stats(&read, &defs, from_ms, to_ms, now_ms))
    }

    /// `battery_hours`: for each hour between consecutive `hour_starts` (the
    /// client's local hours in UTC ms, DST applied, as a heatmap day's), the charge in the
    /// hour's last minute that sampled it and whether it charged in any minute. Minutes
    /// come from `tier_1m` and `recent`, as `query_history` reads them. An hour with no
    /// minute left (`tier_1m` keeps a week; older minutes are rolled into `tier_15m`)
    /// takes the average charge of its last 15 minutes that sampled it, and whether it
    /// charged in any of them. Every zone is offset from UTC by a multiple of 15 minutes,
    /// so neither a minute nor a quarter hour straddles a local hour. Blocking.
    pub fn battery_hours(
        &self,
        host: kelvo_schema::HostId,
        hour_starts: &[i64],
        recent: impl Fn() -> Vec<BucketRow>,
    ) -> Result<Vec<BatteryHour>, CommandError> {
        let invalid = |message: String| CommandError::InvalidArgument { message };
        let hours = hour_starts.len().saturating_sub(1);
        if hours == 0 || hours > MAX_BATTERY_HOURS {
            return Err(invalid(format!(
                "{} hour boundaries, expected 2 to {}",
                hour_starts.len(),
                MAX_BATTERY_HOURS + 1
            )));
        }
        let cells: Vec<(i64, i64)> = hour_starts
            .windows(2)
            .filter_map(|w| Some((*w.first()?, *w.get(1)?)))
            .collect();
        if let Some(&(start, _)) = cells.iter().find(|(start, end)| end < start) {
            return Err(invalid(format!("hour boundaries go backwards at {start}")));
        }
        let mut out: Vec<BatteryHour> = cells
            .iter()
            .map(|&(start_ms, _)| BatteryHour {
                start_ms,
                charge: None,
                charging: false,
            })
            .collect();
        let (from_ms, to_ms) = (
            cells.first().map_or(0, |c| c.0),
            cells.last().map_or(0, |c| c.1),
        );
        if to_ms <= from_ms {
            return Ok(out);
        }
        let query = |tier: Tier, from_ms: i64, to_ms: i64| {
            let width = tier.bucket_ms().unwrap_or(60_000);
            HistoryQuery {
                host,
                selectors: [CHARGE, CHARGING]
                    .into_iter()
                    .map(|m| kelvo_schema::SeriesSelector {
                        metric: MetricId::from_static(m),
                        labels: Labels::new(),
                    })
                    .collect(),
                from_ms,
                to_ms,
                tier: TierChoice::Fixed(tier),
                // One point per bucket: no merging.
                max_points: u32::try_from((to_ms - from_ms) / width + 2).unwrap_or(u32::MAX),
            }
        };
        let minutes = self.history_through_now(&query(Tier::M1, from_ms, to_ms), &recent)?;
        let mut last_t = vec![i64::MIN; out.len()];
        fill_battery_hours(&cells, &minutes, &mut out, &mut last_t);
        // Hours older than the minutes kept: their quarter hours.
        let bare = |i: &usize| last_t.get(*i).is_some_and(|&t| t == i64::MIN);
        let first = (0..cells.len()).find(bare);
        let last = (0..cells.len()).rev().find(bare);
        if let (Some(&(q_from, _)), Some(&(_, q_to))) = (
            first.and_then(|i| cells.get(i)),
            last.and_then(|i| cells.get(i)),
        ) && q_to > q_from
        {
            let quarters = self.history_through_now(&query(Tier::M15, q_from, q_to), &recent)?;
            let mut quarter_out: Vec<BatteryHour> = out.clone();
            let mut quarter_t = vec![i64::MIN; out.len()];
            fill_battery_hours(&cells, &quarters, &mut quarter_out, &mut quarter_t);
            for ((hour, q), &t) in out.iter_mut().zip(quarter_out).zip(&last_t) {
                if t == i64::MIN {
                    *hour = q;
                }
            }
        }
        Ok(out)
    }

    /// Runs `f` on a pooled read connection. Blocking: call it off the main thread.
    pub fn read<T>(
        &self,
        f: impl FnOnce(&mut Reader) -> kelvo_store::Result<T>,
    ) -> Result<T, CommandError> {
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

    pub fn size_on_disk(&self) -> Result<u64, CommandError> {
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
    pub fn reset(&self, now_ms: i64) -> Result<Writer, CommandError> {
        let mut inner = self.inner();
        inner.close(None);
        *self.health.lock().unwrap_or_else(|e| e.into_inner()) =
            kelvo_engine::HistoryHealth::default();
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

impl History {
    /// [`History::reset`], then `attach` (reattach every source, registering its host),
    /// then confirm the store is still there. Returns the health to announce; an error
    /// means history is unavailable and nothing should be announced, so a window's cached
    /// `history_unavailable` (and its banner) stays.
    pub fn reset_and_attach(
        &self,
        now_ms: i64,
        attach: impl FnOnce(&History),
    ) -> Result<HistoryHealth, CommandError> {
        self.reset(now_ms)?;
        attach(self);
        if !self.is_available() {
            return Err(self
                .writer()
                .err()
                .unwrap_or_else(CommandError::history_unavailable));
        }
        Ok(self.health())
    }
}

fn lock_inner(m: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl History {
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
            writer: move || lock_inner(&inner).writer.clone(),
            retention,
            health: Arc::clone(&self.health),
            on_change: move |h: kelvo_engine::HistoryHealth| on_change(h.into()),
        }
        .spawn()
    }
}

impl From<kelvo_engine::HistoryHealth> for HistoryHealth {
    fn from(h: kelvo_engine::HistoryHealth) -> Self {
        HistoryHealth {
            low_disk_paused: h.low_disk_paused,
            trimmed_before_ms: h.trimmed_before_ms,
            trimmed_limit_bytes: h.trimmed_limit_bytes,
            cap_met: h.cap_met,
        }
    }
}

impl HistoryRequest {
    pub fn to_query(&self) -> Result<HistoryQuery, CommandError> {
        if self.from_ms >= self.to_ms {
            return Err(CommandError::InvalidArgument {
                message: format!(
                    "from_ms {} is not before to_ms {}",
                    self.from_ms, self.to_ms
                ),
            });
        }
        if self.max_points == 0 {
            return Err(CommandError::InvalidArgument {
                message: "max_points must be at least 1".into(),
            });
        }
        Ok(HistoryQuery {
            host: self.host,
            selectors: self.selectors.clone(),
            from_ms: self.from_ms,
            to_ms: self.to_ms,
            tier: tier_choice(self.tier),
            max_points: self.max_points,
        })
    }
}

fn tier_choice(tier: TierRequest) -> TierChoice {
    match tier {
        TierRequest::Auto => TierChoice::Auto,
        TierRequest::S10 => TierChoice::Fixed(Tier::S10),
        TierRequest::M1 => TierChoice::Fixed(Tier::M1),
    }
}

/// The most days one heatmap request may ask for: the longest retention (90 days) plus
/// slack for the days a range starts and ends in.
pub const MAX_HEATMAP_DAYS: usize = 92;

/// The most hours one `battery_hours` request may ask for: a heatmap's days.
pub const MAX_BATTERY_HOURS: usize = MAX_HEATMAP_DAYS * 24;

const CHARGE: &str = "battery.charge";
const CHARGING: &str = "battery.charging";

/// One metric's [`RangeStats`].
#[derive(Clone, Debug, PartialEq)]
pub struct MetricStats {
    pub metric: &'static str,
    /// Time inside the range with a reading, outside the gaps that apply to the metric.
    pub measured_ms: i64,
    /// Span-weighted average over `measured_ms`; `None` when nothing was measured.
    pub avg: Option<f64>,
    /// The largest sample; `None` when nothing was measured.
    pub max: Option<f64>,
    /// `avg` times the measured seconds.
    pub integral: f64,
}

/// [`History::series_stats`]: the range widened to whole buckets, as the read took them,
/// and cut at now, with one entry per metric asked for, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeStats {
    pub from_ms: i64,
    pub to_ms: i64,
    pub metrics: Vec<MetricStats>,
}

/// Sums a `series_stats` read. A bucket is measured where the metric has a reading,
/// outside the gaps that apply to its module (host-wide ones and the module's own); an
/// open gap runs to `now_ms`.
fn range_stats(
    read: &HistoryResult,
    defs: &[&'static kelvo_schema::MetricDef],
    from_ms: i64,
    to_ms: i64,
    now_ms: i64,
) -> RangeStats {
    let width = read.bucket_ms.max(1);
    let from = from_ms - from_ms.rem_euclid(width);
    let to = (to_ms + (width - to_ms.rem_euclid(width)) % width)
        .min(now_ms)
        .max(from);
    let metrics = defs
        .iter()
        .map(|d| {
            let mut gaps: Vec<(i64, i64)> = read
                .gaps
                .iter()
                .filter(|g| g.module.is_none_or(|m| m == d.module))
                .map(|g| (g.start_ms, g.end_ms.unwrap_or(now_ms)))
                .collect();
            gaps.sort_unstable();
            // Time in `[a, b)` outside every gap.
            let measured = |a: i64, b: i64| -> i64 {
                let mut covered = 0;
                let mut cursor = a;
                for &(s, e) in &gaps {
                    let (s, e) = (s.max(cursor), e.min(b));
                    if e > s {
                        covered += e - s;
                        cursor = e;
                    }
                }
                (b - a - covered).max(0)
            };
            let mut measured_ms = 0_i64;
            let mut weighted = 0.0_f64;
            let mut max: Option<f64> = None;
            let points = read
                .series
                .iter()
                .filter(|s| s.key.metric == d.id)
                .flat_map(|s| &s.points);
            for p in points {
                let ms = measured(p.t.max(from), (p.t + width).min(to));
                if ms > 0 && p.avg.is_finite() {
                    measured_ms += ms;
                    weighted += f64::from(p.avg) * ms as f64;
                    if p.max.is_finite() {
                        max = Some(max.map_or(f64::from(p.max), |m| m.max(f64::from(p.max))));
                    }
                }
            }
            let avg = (measured_ms > 0).then(|| weighted / measured_ms as f64);
            MetricStats {
                metric: d.id.as_str(),
                measured_ms,
                avg,
                max,
                integral: avg.map_or(0.0, |a| a.max(0.0) * measured_ms as f64 / 1_000.0),
            }
        })
        .collect();
    RangeStats {
        from_ms: from,
        to_ms: to,
        metrics,
    }
}

/// Each battery point of `result` into the hour of `cells` holding it: the latest charge
/// (`last_t` tracks its time, `i64::MIN` while the hour has none) and whether any bucket
/// charged.
fn fill_battery_hours(
    cells: &[(i64, i64)],
    result: &HistoryResult,
    out: &mut [BatteryHour],
    last_t: &mut [i64],
) {
    for s in &result.series {
        for p in &s.points {
            let Some(i) = cell_of(cells, p.t) else {
                continue;
            };
            let (Some(hour), Some(last)) = (out.get_mut(i), last_t.get_mut(i)) else {
                continue;
            };
            match s.key.metric.as_str() {
                CHARGE if p.t > *last => {
                    hour.charge = Some(p.avg);
                    *last = p.t;
                }
                CHARGING if p.max >= 0.5 => hour.charging = true,
                _ => {}
            }
        }
    }
}

/// The index of the cell `[start, end)` containing `ts`; cells are in order of start.
fn cell_of(cells: &[(i64, i64)], ts: i64) -> Option<usize> {
    let i = cells.partition_point(|c| c.0 <= ts).checked_sub(1)?;
    let (start, end) = *cells.get(i)?;
    (start <= ts && ts < end).then_some(i)
}

impl HeatmapMetric {
    pub fn series(self) -> SeriesKey {
        let metric = match self {
            HeatmapMetric::Cpu => "cpu.total",
            HeatmapMetric::Temp => "thermal.hottest",
        };
        SeriesKey::new(MetricId::from_static(metric), Labels::new())
    }
}

impl HeatmapRequest {
    /// The hour cells, every day's 24 in order, after checking that each day has 25
    /// never-decreasing boundaries and the days do not overlap.
    pub fn cells(&self) -> Result<Vec<(i64, i64)>, CommandError> {
        let invalid = |message: String| CommandError::InvalidArgument { message };
        if self.days.len() > MAX_HEATMAP_DAYS {
            return Err(invalid(format!(
                "{} days asked for, at most {MAX_HEATMAP_DAYS}",
                self.days.len()
            )));
        }
        let mut cells = Vec::with_capacity(self.days.len() * 24);
        let mut prev_end = i64::MIN;
        for day in &self.days {
            if day.hour_starts.len() != 25 {
                return Err(invalid(format!(
                    "{}: {} hour boundaries, expected 25",
                    day.date,
                    day.hour_starts.len()
                )));
            }
            for w in day.hour_starts.windows(2) {
                let (Some(start), Some(end)) = (w.first(), w.get(1)) else {
                    continue;
                };
                if start.0 < prev_end || end.0 < start.0 {
                    return Err(invalid(format!(
                        "{}: hour boundaries go backwards at {}",
                        day.date, start.0
                    )));
                }
                cells.push((start.0, end.0));
                prev_end = end.0;
            }
        }
        Ok(cells)
    }

    /// `hours` (one per cell, from [`HeatmapRequest::cells`]) back into days.
    pub fn days(&self, hours: &[Option<f32>]) -> Vec<HeatmapDay> {
        self.days
            .iter()
            .zip(hours.chunks(24))
            .map(|(day, hours)| HeatmapDay {
                date: day.date.clone(),
                hours: hours.to_vec(),
            })
            .collect()
    }
}

impl ExportRequest {
    pub fn to_query(&self) -> Result<ExportQuery, CommandError> {
        if self.from_ms >= self.to_ms {
            return Err(CommandError::InvalidArgument {
                message: format!(
                    "from_ms {} is not before to_ms {}",
                    self.from_ms, self.to_ms
                ),
            });
        }
        if self.selectors.is_empty() {
            return Err(CommandError::InvalidArgument {
                message: "no series selected".into(),
            });
        }
        Ok(ExportQuery {
            host: self.host,
            selectors: self.selectors.clone(),
            from_ms: self.from_ms,
            to_ms: self.to_ms,
            tier: tier_choice(self.tier),
        })
    }
}

/// The IPC page of `r`, each series with `hold(key, bucket_ms)` as its `hold_ms`.
fn history_page(r: HistoryResult, hold: impl Fn(&SeriesKey, i64) -> i64) -> HistoryPage {
    HistoryPage {
        tier: r.tier,
        bucket_ms: r.bucket_ms,
        series: r
            .series
            .into_iter()
            .map(|s| HistorySeries {
                hold_ms: hold(&s.key, r.bucket_ms),
                key: s.key,
                points: s
                    .points
                    .into_iter()
                    .map(|p| HistoryPoint {
                        t: p.t,
                        min: p.min,
                        max: p.max,
                        avg: p.avg,
                    })
                    .collect(),
            })
            .collect(),
        gaps: r.gaps,
    }
}

impl From<kelvo_engine::NetAttribution> for NetworkByApp {
    fn from(a: kelvo_engine::NetAttribution) -> Self {
        NetworkByApp {
            from_ms: a.from_ms,
            to_ms: a.to_ms,
            complete_to_ms: a.to_ms,
            resolution_ms: a.resolution_ms,
            measured_ms: a.measured_ms,
            coverage: a
                .coverage
                .iter()
                .map(|s| NetworkSpan {
                    from_ms: s.from_ms,
                    to_ms: s.to_ms,
                    tier: s.tier,
                })
                .collect(),
            clamped: a.clamped(),
            apps: a
                .apps
                .into_iter()
                .map(|app| crate::ipc::AppBytes {
                    name: app.name,
                    rx_bytes: app.rx_bytes,
                    tx_bytes: app.tx_bytes,
                })
                .collect(),
            other_apps_rx_bytes: a.other_apps_rx_bytes,
            other_apps_tx_bytes: a.other_apps_tx_bytes,
            iface_rx_bytes: a.rx.iface_bytes,
            iface_tx_bytes: a.tx.iface_bytes,
            overhead_rx_bytes: a.rx.overhead_bytes,
            overhead_tx_bytes: a.tx.overhead_bytes,
            system_rx_bytes: a.rx.system_bytes,
            system_tx_bytes: a.tx.system_bytes,
        }
    }
}

impl From<kelvo_store::ProcessesAt> for ProcessesAt {
    fn from(p: kelvo_store::ProcessesAt) -> Self {
        ProcessesAt {
            ts_ms: p.ts_ms,
            resolution: match p.resolution {
                ProcResolution::Snapshot => ProcessResolution::Snapshot,
                ProcResolution::Top5PerMinute => ProcessResolution::Top5PerMinute,
                ProcResolution::Top5Per15Minutes => ProcessResolution::Top5Per15Minutes,
            },
            rows: p
                .rows
                .into_iter()
                .map(|r| StoredProcess {
                    name: r.name,
                    pid: r.pid,
                    cpu_pct: r.cpu_pct,
                    mem_bytes: r.mem_bytes,
                    threads: r.threads,
                    idle_wakeups_per_s: r.idle_wakeups_per_s,
                    energy: r.energy,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelvo_engine::RecentNet;

    const NOW: i64 = 1_800_000_000_000;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kelvo-shell-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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
                boot_mounts: Vec::new(),
            },
        }
    }

    /// D-064 (#2): a store that opens but cannot take the local host makes history
    /// unavailable with the reason; it does not stop the launch. `reset_history` then
    /// recovers it.
    #[test]
    fn a_store_that_cannot_take_the_host_goes_unavailable_and_reset_recovers_it() {
        let dir = temp_dir("register-fails");
        drop(History::open(&dir));
        // The file opens fine but refuses every new host.
        let conn = kelvo_store::rusqlite::Connection::open(dir.join(DB_FILE)).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER refuse_hosts BEFORE INSERT ON hosts
             BEGIN SELECT RAISE(ABORT, 'disk says no'); END;",
        )
        .unwrap();
        drop(conn);

        let history = History::open(&dir);
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
        assert_eq!(
            history.read(|r| r.hosts()).unwrap_err(),
            CommandError::HistoryUnavailable {
                reason: Some(reason.clone())
            },
            "commands say why"
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
        let dir = temp_dir("locked");
        let other = Store::open(StoreConfig::new(dir.join(DB_FILE))).unwrap();
        let history = History::open(&dir);
        assert_eq!(
            history.unavailable_reason(),
            Some(HistoryUnavailableReason::Locked)
        );
        assert!(matches!(
            history.reset(NOW),
            Err(CommandError::StoreBusy { .. })
        ));
        assert!(
            dir.join(DB_FILE).exists(),
            "the other process's file is untouched"
        );
        other.close().unwrap();
        history.reset(NOW).unwrap();
        assert!(history.is_available());
    }

    /// `reset_history` emits `history-health-changed` only with the health this returns:
    /// a failed reset returns the error and the frontend keeps its cached
    /// `history_unavailable` (and the Settings banner).
    #[test]
    fn reset_returns_health_only_when_history_is_back() {
        let dir = temp_dir("reset-health");
        let other = Store::open(StoreConfig::new(dir.join(DB_FILE))).unwrap();
        let history = History::open(&dir);
        let mut attached = 0;
        let err = history
            .reset_and_attach(NOW, |_| attached += 1)
            .unwrap_err();
        assert!(matches!(err, CommandError::StoreBusy { .. }), "{err:?}");
        assert_eq!(
            attached, 0,
            "nothing reattached to a store that is not there"
        );

        other.close().unwrap();
        let health = history
            .reset_and_attach(NOW, |h| {
                attached += 1;
                assert!(h.register_host(&local_record()).is_some());
            })
            .unwrap();
        assert_eq!(health, HistoryHealth::default());
        assert_eq!(attached, 1);

        // Reattaching can still lose the store (it refuses a host, as `register_host`
        // handles it): an error, no health.
        let err = history
            .reset_and_attach(NOW + 1, |h| {
                h.inner().close(Some(HistoryUnavailableReason::Failed {
                    message: "disk says no".into(),
                }));
            })
            .unwrap_err();
        assert!(
            matches!(&err, CommandError::HistoryUnavailable { reason: Some(_) }),
            "{err:?}"
        );
        history.close();
    }

    /// The request as the frontend sends it: one local day per entry, 25 boundaries each.
    fn heatmap_request(days: serde_json::Value) -> HeatmapRequest {
        serde_json::from_value(serde_json::json!({
            "host": local_record().id,
            "metric": "cpu",
            "days": days,
        }))
        .unwrap()
    }

    #[test]
    fn a_heatmap_request_reads_one_value_per_local_hour() {
        let dir = temp_dir("heatmap");
        let history = History::open(&dir);
        let w = history.register_host(&local_record()).unwrap();
        let series: std::sync::Arc<[SeriesKey]> =
            vec![SeriesKey::parse("cpu.total").unwrap()].into();
        const H: i64 = 3_600_000;
        // A 23-hour day starting at NOW (hour 2 does not exist), with minutes in hour 0
        // (value 10) and hour 3 (value 30).
        let mut starts: Vec<i64> = vec![NOW, NOW + H, NOW + 2 * H];
        starts.extend((3..=24).map(|h| NOW + (h - 1) * H));
        for (start, v) in [(NOW, 10.0), (NOW + 2 * H, 30.0)] {
            for m in 0..60 {
                w.write_bucket(kelvo_store::BucketRow {
                    host: local_record().id,
                    tier: Tier::M1,
                    bucket_ts: start + m * 60_000,
                    series: std::sync::Arc::clone(&series),
                    stats: vec![v, v, v],
                })
                .unwrap();
            }
        }
        w.flush().unwrap();

        let req = heatmap_request(serde_json::json!([
            { "date": "2026-03-08", "hour_starts": starts },
        ]));
        let cells = req.cells().unwrap();
        let hours = history
            .read(|r| r.heatmap(req.host, &req.metric.series(), &cells))
            .unwrap();
        let days = req.days(&hours);
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].date, "2026-03-08");
        let mut want = vec![None; 24];
        want[0] = Some(10.0);
        want[3] = Some(30.0);
        assert_eq!(days[0].hours, want, "hour 2 is empty, empty hours are null");
        assert_eq!(
            serde_json::to_value(&days[0]).unwrap()["hours"][1],
            serde_json::Value::Null
        );
        history.close();
    }

    #[test]
    fn malformed_heatmap_days_are_invalid_arguments() {
        let day = |starts: Vec<i64>| serde_json::json!({ "date": "d", "hour_starts": starts });
        let ok: Vec<i64> = (0..25).map(|h| NOW + h * 3_600_000).collect();
        let invalid = |days: serde_json::Value| {
            matches!(
                heatmap_request(days).cells(),
                Err(CommandError::InvalidArgument { .. })
            )
        };
        assert!(!invalid(serde_json::json!([day(ok.clone())])));
        assert!(
            invalid(serde_json::json!([day(ok[..24].to_vec())])),
            "24 boundaries"
        );
        let mut backwards = ok.clone();
        backwards.swap(3, 4);
        assert!(invalid(serde_json::json!([day(backwards)])));
        assert!(
            invalid(serde_json::json!([day(ok.clone()), day(ok.clone())])),
            "the second day overlaps the first"
        );
        let many: Vec<_> = (0..=MAX_HEATMAP_DAYS as i64)
            .map(|d| day(ok.iter().map(|t| t + d * 86_400_000).collect()))
            .collect();
        assert!(invalid(serde_json::Value::Array(many)));
    }

    fn net_bucket(apps: &[(Option<&str>, u64, u64)], rx: u64, tx: u64) -> kelvo_store::NetBucket {
        kelvo_store::NetBucket {
            measured_ms: 10_000,
            iface_rx_bytes: rx,
            iface_tx_bytes: tx,
            iface_rx_pkts: rx / 1_500,
            iface_tx_pkts: tx / 1_500,
            apps: apps
                .iter()
                .map(|&(name, rx_bytes, tx_bytes)| kelvo_store::NetApp {
                    name: name.map(str::to_owned),
                    rx_bytes,
                    tx_bytes,
                })
                .collect(),
        }
    }

    /// `query_network_by_app`'s read: committed 10 s rows plus the engine's ring, which
    /// replaces a stored bucket it also holds, split into apps, other apps, overhead and
    /// "System and other" that add up to the interface totals.
    #[test]
    fn network_by_app_merges_the_ring_over_the_store_and_adds_up() {
        let dir = temp_dir("net-by-app");
        let history = History::open(&dir);
        let host = local_record().id;
        let w = history.register_host(&local_record()).unwrap();
        // NOW is on the 10 s grid. Two committed buckets; the second is also in the ring,
        // newer (the open bucket grew).
        w.write_net_bucket(
            host,
            NOW,
            net_bucket(&[(Some("Safari"), 400_000, 20_000)], 600_000, 60_000),
        )
        .unwrap();
        w.write_net_bucket(
            host,
            NOW + 10_000,
            net_bucket(&[(Some("Safari"), 1, 1)], 2, 2),
        )
        .unwrap();
        w.flush().unwrap();
        let recent = vec![
            (
                NOW + 10_000,
                net_bucket(
                    &[
                        (Some("Docker Desktop"), 9_000_000, 90_000),
                        (None, 50_000, 5_000),
                    ],
                    9_900_000,
                    300_000,
                ),
            ),
            (
                NOW + 20_000,
                net_bucket(&[(Some("Safari"), 100_000, 10_000)], 150_000, 30_000),
            ),
        ];

        let n = history
            .network_by_app(host, NOW + 3_000, NOW + 25_000, || RecentNet {
                buckets: recent.clone(),
                complete_to_ms: Some(NOW + 20_000),
            })
            .unwrap();
        assert_eq!((n.from_ms, n.to_ms), (NOW, NOW + 30_000), "whole buckets");
        assert_eq!(n.complete_to_ms, NOW + 20_000, "the open bucket's start");
        assert_eq!(n.measured_ms, 30_000);
        assert_eq!(n.resolution_ms, 10_000);
        assert_eq!(
            n.coverage,
            vec![NetworkSpan {
                from_ms: NOW,
                to_ms: NOW + 30_000,
                tier: Some(Tier::S10)
            }]
        );
        let names: Vec<_> = n
            .apps
            .iter()
            .map(|a| (a.name.as_str(), a.rx_bytes))
            .collect();
        assert_eq!(
            names,
            [("Docker Desktop", 9_000_000), ("Safari", 500_000)],
            "the ring's bucket replaced the stored one; other apps are separate"
        );
        assert_eq!(
            (n.other_apps_rx_bytes, n.other_apps_tx_bytes),
            (50_000, 5_000)
        );
        assert_eq!(n.iface_rx_bytes, 10_650_000);
        assert_eq!(
            n.overhead_rx_bytes,
            (600_000 / 1_500 + 9_900_000 / 1_500 + 150_000 / 1_500) * 66
        );
        for (iface, apps, other, overhead, system) in [
            (
                n.iface_rx_bytes,
                n.apps.iter().map(|a| a.rx_bytes).sum::<u64>(),
                n.other_apps_rx_bytes,
                n.overhead_rx_bytes,
                n.system_rx_bytes,
            ),
            (
                n.iface_tx_bytes,
                n.apps.iter().map(|a| a.tx_bytes).sum::<u64>(),
                n.other_apps_tx_bytes,
                n.overhead_tx_bytes,
                n.system_tx_bytes,
            ),
        ] {
            assert!(system > 0);
            assert_eq!(apps + other + overhead + system, iface);
        }
        assert!(!n.clamped);

        // Before anything was recorded: an empty, honest answer.
        let empty = history
            .network_by_app(host, NOW - 60_000, NOW - 30_000, || RecentNet {
                buckets: Vec::new(),
                complete_to_ms: Some(NOW + 20_000),
            })
            .unwrap();
        assert_eq!(empty.measured_ms, 0);
        assert!(empty.apps.is_empty());
        assert!(empty.coverage.iter().all(|s| s.tier.is_none()));
        assert_eq!(
            empty.complete_to_ms, empty.to_ms,
            "before the engine's edge: final"
        );

        assert!(matches!(
            history.network_by_app(host, NOW, NOW - 1, RecentNet::default),
            Err(CommandError::InvalidArgument { .. })
        ));
        history.close();
    }

    #[test]
    fn network_by_app_promises_nothing_past_what_the_engine_closed() {
        let history = History::unavailable();
        let host = local_record().id;
        // Nothing open: the streams have reported, and the engine closed, to NOW + 10 s.
        // The range runs into the future; only its part before that edge is final.
        let n = history
            .network_by_app(host, NOW - 30_000, NOW + 60_000, || RecentNet {
                buckets: Vec::new(),
                complete_to_ms: Some(NOW + 10_000),
            })
            .unwrap();
        assert_eq!(n.to_ms, NOW + 60_000);
        assert_eq!(n.complete_to_ms, NOW + 10_000);
        // No edge (only before the engine starts): nothing promised.
        let n = history
            .network_by_app(host, NOW - 30_000, NOW + 60_000, RecentNet::default)
            .unwrap();
        assert_eq!(n.complete_to_ms, NOW - 30_000);
    }

    #[test]
    fn network_by_app_refuses_spans_past_the_longest_retention() {
        let history = History::unavailable();
        let host = local_record().id;
        let longest = 90 * Retention::DAY_MS;
        assert!(
            history
                .network_by_app(host, NOW - longest, NOW, RecentNet::default)
                .is_ok()
        );
        let err = history
            .network_by_app(host, NOW - longest - 10_000, NOW, RecentNet::default)
            .unwrap_err();
        assert!(
            matches!(&err, CommandError::InvalidArgument { message } if message.contains("90 days")),
            "{err:?}"
        );
    }

    #[test]
    fn network_by_app_answers_from_the_ring_while_history_is_unavailable() {
        let history = History::unavailable();
        let host = local_record().id;
        let n = history
            .network_by_app(host, NOW, NOW + 30_000, || RecentNet {
                buckets: vec![(
                    NOW + 10_000,
                    net_bucket(&[(Some("Safari"), 400_000, 20_000)], 600_000, 60_000),
                )],
                complete_to_ms: Some(NOW + 20_000),
            })
            .unwrap();
        assert_eq!(n.measured_ms, 10_000);
        assert_eq!(n.apps.first().map(|a| a.name.as_str()), Some("Safari"));
        assert_eq!(n.iface_rx_bytes, 600_000);
        assert_eq!(
            n.coverage.iter().map(|s| s.tier).collect::<Vec<_>>(),
            [None, Some(Tier::S10), None],
            "live-only: what the ring has, the rest not recorded"
        );
        assert_eq!(n.complete_to_ms, NOW + 20_000);
    }

    /// Minute rows of `battery.charge` (`charge(m)`) and `battery.charging`
    /// (`charging(m)`) for minutes `minutes` after `NOW`.
    fn battery_minutes(
        minutes: std::ops::Range<i64>,
        charge: impl Fn(i64) -> f32,
        charging: impl Fn(i64) -> f32,
    ) -> Vec<BucketRow> {
        let series: std::sync::Arc<[SeriesKey]> = vec![
            SeriesKey::parse("battery.charge").unwrap(),
            SeriesKey::parse("battery.charging").unwrap(),
        ]
        .into();
        minutes
            .map(|m| {
                let (v, c) = (charge(m), charging(m));
                BucketRow {
                    host: local_record().id,
                    tier: Tier::M1,
                    bucket_ts: NOW + m * 60_000,
                    series: std::sync::Arc::clone(&series),
                    stats: vec![v, v, v, c, c, c],
                }
            })
            .collect()
    }

    /// 10 s rows of `net.rx_total` (1,000 B/s) and `net.tx_total` (100 B/s) for buckets
    /// `buckets` after `NOW`.
    fn net_buckets(buckets: impl Iterator<Item = i64>) -> Vec<BucketRow> {
        let series: std::sync::Arc<[SeriesKey]> = vec![
            SeriesKey::parse("net.rx_total").unwrap(),
            SeriesKey::parse("net.tx_total").unwrap(),
        ]
        .into();
        buckets
            .map(|b| BucketRow {
                host: local_record().id,
                tier: Tier::S10,
                bucket_ts: NOW + b * 10_000,
                series: std::sync::Arc::clone(&series),
                stats: vec![900.0, 1_100.0, 1_000.0, 90.0, 110.0, 100.0],
            })
            .collect()
    }

    #[test]
    fn series_stats_average_peak_and_gaps_by_module() {
        const S: i64 = 1_000;
        let series: std::sync::Arc<[SeriesKey]> = vec![
            SeriesKey::parse("cpu.total").unwrap(),
            SeriesKey::parse("disk.read_total").unwrap(),
        ]
        .into();
        // `cpu.total` at 20% for buckets 0..3 and 50% (peaking at 90%) for 3..6;
        // `disk.read_total` at 1,000 B/s throughout.
        let rows = (0..6)
            .map(|b| BucketRow {
                host: local_record().id,
                tier: Tier::S10,
                bucket_ts: NOW + b * 10_000,
                series: std::sync::Arc::clone(&series),
                stats: if b < 3 {
                    vec![10.0, 30.0, 20.0, 1_000.0, 1_000.0, 1_000.0]
                } else {
                    vec![10.0, 90.0, 50.0, 1_000.0, 1_000.0, 1_000.0]
                },
            })
            .collect::<Vec<_>>();
        let dir = temp_dir("series-stats");
        let history = History::open(&dir);
        let w = history.register_host(&local_record()).unwrap();
        for row in rows {
            w.write_bucket(row).unwrap();
        }
        // Disk switched off over bucket 0: CPU still counts it.
        let host = local_record().id;
        w.write_gap(
            host,
            kelvo_schema::Gap::new(
                NOW,
                Some(NOW + 10 * S),
                Some(kelvo_schema::Module::Disk),
                kelvo_schema::GapReason::ModuleDisabled,
            )
            .unwrap(),
        )
        .unwrap();
        w.flush().unwrap();
        let metrics = ["cpu.total".to_owned(), "disk.read_total".to_owned()];
        let s = history
            .series_stats(host, &metrics, NOW, NOW + 60 * S, NOW + 600 * S, Vec::new)
            .unwrap();
        let [cpu, disk] = s.metrics.as_slice() else {
            panic!("two metrics: {s:?}")
        };
        assert_eq!(cpu.measured_ms, 60_000);
        assert_eq!(cpu.avg, Some(35.0));
        assert_eq!(cpu.max, Some(90.0));
        assert_eq!(disk.measured_ms, 50_000, "Disk's own gap");
        assert_eq!(disk.integral, 50_000.0);
        history.close();

        let labelled = History::unavailable().series_stats(
            host,
            &["cpu.load".to_owned()],
            NOW,
            NOW + S,
            NOW,
            Vec::new,
        );
        assert!(matches!(
            labelled,
            Err(CommandError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn series_stats_count_measured_time_through_now() {
        const S: i64 = 1_000;
        let dir = temp_dir("network-totals");
        let history = History::open(&dir);
        let w = history.register_host(&local_record()).unwrap();
        // Buckets 12 and 13 have no reading; 25.. are only in the engine.
        for row in net_buckets((0..25).filter(|b| !(12..14).contains(b))) {
            w.write_bucket(row).unwrap();
        }
        let host = local_record().id;
        // Asleep over all of bucket 10 and half of 11. CPU switched off over the first
        // buckets does not touch Network.
        w.write_gap(
            host,
            kelvo_schema::Gap::new(
                NOW + 100 * S,
                Some(NOW + 115 * S),
                None,
                kelvo_schema::GapReason::Sleep,
            )
            .unwrap(),
        )
        .unwrap();
        w.write_gap(
            host,
            kelvo_schema::Gap::new(
                NOW,
                Some(NOW + 20 * S),
                Some(kelvo_schema::Module::Cpu),
                kelvo_schema::GapReason::ModuleDisabled,
            )
            .unwrap(),
        )
        .unwrap();
        w.flush().unwrap();
        // Bucket 27 is open: now is halfway through it. 28 is past now.
        let recent = || net_buckets(20..29);
        let net = || vec!["net.rx_total".to_owned(), "net.tx_total".to_owned()];
        let totals = history
            .series_stats(
                host,
                &net(),
                NOW + 5 * S,
                NOW + 400 * S,
                NOW + 275 * S,
                recent,
            )
            .unwrap();
        // 28 buckets to now, less 12 and 13, 10 (asleep), half of 11 and half of 27.
        let measured = 240 * S;
        let rx_tx = |t: &RangeStats| {
            t.metrics
                .iter()
                .map(|m| (m.measured_ms, m.integral.round() as i64))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            (totals.from_ms, totals.to_ms),
            (NOW, NOW + 275 * S),
            "widened to the bucket, cut at now"
        );
        assert_eq!(
            rx_tx(&totals),
            [(measured, 240_000), (measured, 24_000)],
            "gaps and empty buckets not counted"
        );
        history.close();

        // Live-only: the engine's rows alone.
        let totals = History::unavailable()
            .series_stats(
                host,
                &net(),
                NOW + 200 * S,
                NOW + 270 * S,
                NOW + 275 * S,
                recent,
            )
            .unwrap();
        assert_eq!(rx_tx(&totals)[0], (70_000, 70_000));

        let bad = History::unavailable().series_stats(host, &net(), NOW + S, NOW, NOW, Vec::new);
        assert!(matches!(bad, Err(CommandError::InvalidArgument { .. })));
    }

    #[test]
    fn battery_hours_follow_local_hours_in_a_half_hour_zone_through_now() {
        const MIN: i64 = 60_000;
        let dir = temp_dir("battery-hours");
        let history = History::open(&dir);
        let w = history.register_host(&local_record()).unwrap();
        // Charge is the minute's index; the battery charged in minute 25 and minute 105.
        let charging = |m: i64| if m == 25 || m == 105 { 1.0 } else { 0.0 };
        for row in battery_minutes(0..100, |m| m as f32, charging) {
            w.write_bucket(row).unwrap();
        }
        w.flush().unwrap();
        // UTC+5:30: local hours start at :30 UTC. NOW is a UTC hour.
        let starts = [NOW + 30 * MIN, NOW + 90 * MIN, NOW + 150 * MIN];
        // Minutes 100..110 are only in the engine (not committed yet), 95..100 in both.
        let recent = || battery_minutes(95..110, |m| m as f32, charging);
        let hours = history
            .battery_hours(local_record().id, &starts, recent)
            .unwrap();
        assert_eq!(
            hours,
            [
                BatteryHour {
                    start_ms: NOW + 30 * MIN,
                    charge: Some(89.0),
                    charging: false,
                },
                BatteryHour {
                    start_ms: NOW + 90 * MIN,
                    charge: Some(109.0),
                    charging: true,
                },
            ],
            "minute 25 is in the UTC hour but before the local one; 105 is not committed"
        );
        history.close();

        // Live-only: the engine's minutes alone.
        let hours = History::unavailable()
            .battery_hours(local_record().id, &starts, recent)
            .unwrap();
        assert_eq!(hours[0].charge, None, "nothing that old in the engine");
        assert_eq!(hours[1].charge, Some(109.0));

        let bad =
            History::unavailable().battery_hours(local_record().id, &[NOW + MIN, NOW], Vec::new);
        assert!(matches!(bad, Err(CommandError::InvalidArgument { .. })));
    }

    #[test]
    fn battery_hours_past_the_minutes_kept_read_quarter_hours() {
        const MIN: i64 = 60_000;
        let dir = temp_dir("battery-hours-quarters");
        let history = History::open(&dir);
        let w = history.register_host(&local_record()).unwrap();
        // The hour before last is only in `tier_15m` (rolled down): charge 10..40 by
        // quarter, charging in the second. The last hour has its minutes.
        for row in battery_minutes(0..4, |q| 10.0 * (q + 1) as f32, |q| (q == 1) as u8 as f32) {
            w.write_bucket(BucketRow {
                tier: Tier::M15,
                bucket_ts: NOW - 120 * MIN + (row.bucket_ts - NOW) * 15,
                ..row
            })
            .unwrap();
        }
        for row in battery_minutes(-60..0, |m| (100 + m) as f32, |_| 0.0) {
            w.write_bucket(row).unwrap();
        }
        w.flush().unwrap();
        let starts = [NOW - 120 * MIN, NOW - 60 * MIN, NOW, NOW + 60 * MIN];
        let hours = history
            .battery_hours(local_record().id, &starts, Vec::new)
            .unwrap();
        let got: Vec<_> = hours.iter().map(|h| (h.charge, h.charging)).collect();
        assert_eq!(
            got,
            [(Some(40.0), true), (Some(99.0), false), (None, false)],
            "the old hour from its quarters, the last from its minutes"
        );
        history.close();
    }

    #[test]
    fn history_pages_carry_each_series_hold() {
        let history = History::unavailable();
        let request = HistoryRequest {
            host: local_record().id,
            selectors: vec![kelvo_schema::SeriesSelector {
                metric: MetricId::from_static("battery.charge"),
                labels: Labels::new(),
            }],
            from_ms: NOW,
            to_ms: NOW + 3_600_000,
            tier: TierRequest::M1,
            max_points: 60,
        };
        let page = history
            .history(
                &request,
                || battery_minutes(0..3, |m| m as f32, |_| 0.0),
                |_, bucket_ms| bucket_ms * 3,
            )
            .unwrap();
        assert_eq!(page.series.len(), 1);
        assert_eq!(page.series[0].points.len(), 3);
        assert_eq!(page.series[0].hold_ms, 180_000);
    }

    #[test]
    fn network_by_app_json_is_snake_case_numbers() {
        let n = NetworkByApp::from(kelvo_engine::attribute(kelvo_store::NetByApp {
            from_ms: NOW,
            to_ms: NOW + 10_000,
            coverage: vec![kelvo_store::NetSpan {
                from_ms: NOW,
                to_ms: NOW + 10_000,
                tier: None,
            }],
            resolution_ms: 10_000,
            measured_ms: 0,
            iface_rx_bytes: 0,
            iface_tx_bytes: 0,
            iface_rx_pkts: 0,
            iface_tx_pkts: 0,
            apps: Vec::new(),
        }));
        let json = serde_json::to_value(&n).unwrap();
        assert_eq!(
            json["coverage"],
            serde_json::json!([{ "from_ms": NOW, "to_ms": NOW + 10_000, "tier": null }])
        );
        assert_eq!(json["system_rx_bytes"], 0);
        assert_eq!(json["clamped"], false);
    }

    #[test]
    fn health_event_json() {
        let health = HistoryHealth {
            low_disk_paused: true,
            trimmed_before_ms: Some(1_700_000_000_000),
            trimmed_limit_bytes: Some(150_000_000),
            cap_met: true,
        };
        assert_eq!(
            serde_json::to_value(crate::ipc::HistoryHealthChanged { health }).unwrap(),
            serde_json::json!({ "health": {
                "low_disk_paused": true,
                "trimmed_before_ms": 1_700_000_000_000_i64,
                "trimmed_limit_bytes": 150_000_000,
                "cap_met": true,
            }})
        );
    }

    #[test]
    fn clearing_history_forgets_the_trim_once() {
        let h = History::unavailable();
        assert_eq!(h.clear_trim(), None, "nothing to clear");
        *h.health.lock().unwrap() = kelvo_engine::HistoryHealth {
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
}

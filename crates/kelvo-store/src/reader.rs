//! Read-only connections: history queries, process lookups, and the cursor read API the
//! sync exporter serves. Any number of readers can run beside the writer (WAL).

use std::borrow::Cow;
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use kelvo_schema::{
    Cursor, Event, Gap, GapReason, HostId, HostInfo, HostRecord, Labels, MetricId, Module,
    SeriesKey, SyncKinds, SyncRowKind, Tier,
};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::blob;
use crate::db::{self, tier_table, tier_width};
use crate::error::{Result, StoreError};
use crate::types::{
    BucketRow, CursorPage, CursorRead, HistoryQuery, HistoryResult, PageEvent, PageGap, PageLayout,
    PageRow, Point, ProcResolution, ProcRow, ProcessesAt, Retention, SeriesPoints, TierChoice,
};

mod export;
mod growth;
mod heatmap;
mod net;
mod stats;

pub use export::{ExportQuery, ExportSummary};
pub use growth::{HistoryGrowth, MIN_MEASURED_MS};
pub use heatmap::{BATTERY_CHARGE, BATTERY_CHARGING, BatteryCell, fill_battery_hours};
pub use net::net_by_app_recent;
pub use stats::{MetricStats, RangeStats, range_stats};

/// How far from the requested instant a process snapshot still counts as "at" it: half
/// the widest snapshot spacing (30 s in Performance mode, D-088), so an instant between
/// two snapshots always finds one.
const PROC_SNAP_TOLERANCE_MS: i64 = 15_000;

/// A read-only connection with its own caches. Not shared between threads; open one per
/// thread with [`crate::Store::reader`].
pub struct Reader {
    conn: Connection,
    epoch: Uuid,
    hosts: HashMap<HostId, i64>,
    /// Layouts are immutable once minted, so this never goes stale.
    layouts: HashMap<u32, Arc<[SeriesKey]>>,
}

impl Reader {
    pub(crate) fn new(conn: Connection, epoch: Uuid) -> Self {
        Self {
            conn,
            epoch,
            hosts: HashMap::new(),
            layouts: HashMap::new(),
        }
    }

    /// This database's `db_instance_uuid`.
    pub fn epoch(&self) -> Uuid {
        self.epoch
    }

    /// Whether the connection is inside a transaction, holding a read snapshot. A pool
    /// must drop such a reader rather than reuse it.
    pub fn in_transaction(&self) -> bool {
        !self.conn.is_autocommit()
    }

    /// Every host record, local first.
    pub fn hosts(&self) -> Result<Vec<HostRecord>> {
        self.host_records("SELECT uuid, is_local, name, info FROM hosts ORDER BY is_local DESC, id")
    }

    /// The local host, if one was ever registered. The `hosts_one_local` index allows at
    /// most one, so this is how a lost `host-id` file is recovered (D-051, D-064).
    pub fn local_host(&self) -> Result<Option<HostRecord>> {
        Ok(self
            .host_records("SELECT uuid, is_local, name, info FROM hosts WHERE is_local = 1")?
            .pop())
    }

    fn host_records(&self, sql: &str) -> Result<Vec<HostRecord>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, bool>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Vec<u8>>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (uuid, is_local, name, info) = row?;
            let id = Uuid::parse_str(&uuid)
                .map_err(|e| StoreError::Corrupt(format!("host uuid {uuid}: {e}")))?;
            let info: HostInfo = ciborium::from_reader(info.as_slice())
                .map_err(|e| StoreError::Cbor(e.to_string()))?;
            out.push(HostRecord {
                id: HostId(id),
                is_local,
                display_name: name,
                info,
            });
        }
        Ok(out)
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

    fn layout(&mut self, id: u32) -> Result<Arc<[SeriesKey]>> {
        load_layout(&self.conn, &mut self.layouts, id)
    }

    /// The `pruned` mark of `(host, tier)`: (highest pruned seq, cutoff ts).
    fn pruned(&self, host_ref: i64, tier: Tier) -> Result<Option<(i64, i64)>> {
        self.pruned_key(host_ref, tier.as_str())
    }

    /// A `pruned` row by key: a tier, or [`db::M1_ROLLED`].
    fn pruned_key(&self, host_ref: i64, key: &str) -> Result<Option<(i64, i64)>> {
        Ok(self
            .conn
            .prepare_cached("SELECT seq, ts FROM pruned WHERE host_id = ?1 AND tier = ?2")?
            .query_row(params![host_ref, key], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?)
    }

    // --- history --------------------------------------------------------------------------

    /// `query_history`: per-series points from one tier, merged down to `max_points`, plus
    /// the gaps overlapping the range. Stored rows alone; see [`Reader::history_after`].
    pub fn history(&mut self, q: &HistoryQuery) -> Result<HistoryResult> {
        self.history_after(q, Vec::new)
    }

    /// [`Reader::history`] over the stored rows and `recent`: the engine's bucket rows
    /// since its last commit and its open buckets flushed as they are (D-092). The writer
    /// commits every 5 minutes, so the newest buckets are not in the file yet. A recent
    /// row replaces the stored row of the same tier, bucket and layout (what the writer's
    /// upsert will do), so a bucket both committed and still held counts once. `recent`
    /// is called after the stored rows are read: the engine keeps a row before it queues
    /// it to the writer, so a copy taken after the read is never older than the stored
    /// row it replaces. Rows of other tiers are ignored; an M15 read takes recent minutes
    /// as it takes the stored ones.
    pub fn history_after<R: AsRef<[BucketRow]>>(
        &mut self,
        q: &HistoryQuery,
        recent: impl FnOnce() -> R,
    ) -> Result<HistoryResult> {
        let host_ref = self.host_ref(q.host)?;
        let tier = match q.tier {
            TierChoice::Fixed(t) => t,
            TierChoice::Auto => self.auto_tier(host_ref, q.from_ms, q.to_ms)?,
        };
        let width = tier_width(tier)?;
        let gaps = self.gaps_in(host_ref, q.from_ms, q.to_ms)?;
        if q.to_ms <= q.from_ms || q.selectors.is_empty() {
            return Ok(HistoryResult {
                tier,
                bucket_ms: width,
                series: Vec::new(),
                gaps,
            });
        }
        let mut merge = Merge::new(q, width);

        // An M15 read also takes the minutes still in `tier_1m`, weighed by width (D-076).
        let sql = format!("{} ORDER BY bucket_ts", db::bucket_rows_sql(tier)?);
        let rows: Vec<(i64, u32, Vec<u8>, u32)> = self
            .conn
            .prepare_cached(&sql)?
            .query_map(params![host_ref, merge.base, q.to_ms], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<rusqlite::Result<_>>()?;

        let recent = recent();
        let recent = merge.recent(recent.as_ref(), tier);
        let recent_ts: HashSet<i64> = recent.iter().map(|r| r.bucket_ts).collect();
        // Per layout: (result index, position in layout) of each matching series.
        let mut matches: HashMap<u32, Positions> = HashMap::new();
        for (ts, layout_id, stats, weight) in rows {
            // Weight 1: a row of the tier the recent rows are read for.
            if weight == 1 && recent_ts.contains(&ts) {
                let layout = self.layout(layout_id)?;
                if recent
                    .iter()
                    .any(|r| r.bucket_ts == ts && *r.series == *layout)
                {
                    continue;
                }
            }
            let positions = match matches.entry(layout_id) {
                Entry::Occupied(e) => e.into_mut(),
                Entry::Vacant(e) => {
                    let layout = self.layout(layout_id)?;
                    e.insert(merge.positions(&layout))
                }
            };
            merge.add(ts, positions, |i| f32_at(&stats, i), weight)?;
        }
        merge.add_recent(&recent)?;
        Ok(HistoryResult {
            tier,
            bucket_ms: merge.slot_w,
            series: merge.finish(),
            gaps,
        })
    }

    /// `S10` when the range fits in the 10 s tier's window, pruning has not removed
    /// anything after its start, and low-disk mode left no holes in it (D-057).
    /// Otherwise `M15` when the range starts more than one 15-minute bucket before the
    /// minutes the roll-down has kept (D-076), and `M1` when it does not. The slack keeps
    /// a 7-day range ending now on minutes whether or not the last prune has rolled its
    /// first few minutes down.
    fn auto_tier(&self, host_ref: i64, from_ms: i64, to_ms: i64) -> Result<Tier> {
        if self.s10_covers(host_ref, from_ms, to_ms)? {
            return Ok(Tier::S10);
        }
        let quarter = Tier::M15.bucket_ms().unwrap_or(900_000);
        match self.pruned_key(host_ref, db::M1_ROLLED)? {
            Some((_, rolled_until)) if rolled_until.saturating_sub(from_ms) > quarter => {
                Ok(Tier::M15)
            }
            _ => Ok(Tier::M1),
        }
    }

    fn s10_covers(&self, host_ref: i64, from_ms: i64, to_ms: i64) -> Result<bool> {
        if to_ms - from_ms > Retention::default().s10_ms {
            return Ok(false);
        }
        if db::read_s10_hole_until(&self.conn)?.is_some_and(|until| until > from_ms) {
            return Ok(false);
        }
        Ok(!matches!(
            self.pruned(host_ref, Tier::S10)?,
            Some((_, cutoff)) if cutoff > from_ms
        ))
    }

    /// Gaps of `host` overlapping `[from, to)`, by start.
    pub fn gaps(&mut self, host: HostId, from_ms: i64, to_ms: i64) -> Result<Vec<Gap>> {
        let host_ref = self.host_ref(host)?;
        self.gaps_in(host_ref, from_ms, to_ms)
    }

    fn gaps_in(&self, host_ref: i64, from: i64, to: i64) -> Result<Vec<Gap>> {
        let rows = self
            .conn
            .prepare_cached(
                "SELECT start_ts, end_ts, module, reason FROM gaps
                 WHERE host_id = ?1 AND start_ts < ?3 AND (end_ts IS NULL OR end_ts > ?2)
                 ORDER BY start_ts",
            )?
            .query_map(params![host_ref, from, to], gap_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    // --- events ---------------------------------------------------------------------------

    /// `query_events`: the events with `from_ms <= ts < to_ms`, oldest first. A payload
    /// this build cannot decode, a kind from a newer build among them, is skipped (D-083).
    pub fn events(&mut self, host: HostId, from_ms: i64, to_ms: i64) -> Result<Vec<Event>> {
        let host_ref = self.host_ref(host)?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT payload FROM events WHERE host_id = ?1 AND ts >= ?2 AND ts < ?3
             ORDER BY ts, kind",
        )?;
        let rows = stmt.query_map(params![host_ref, from_ms, to_ms], |r| {
            r.get::<_, Vec<u8>>(0)
        })?;
        let mut out = Vec::new();
        for payload in rows {
            match ciborium::from_reader::<Event, _>(payload?.as_slice()) {
                Ok(e) => out.push(e),
                Err(e) => tracing::debug!("skipping an event this build cannot decode: {e}"),
            }
        }
        Ok(out)
    }

    // --- processes ------------------------------------------------------------------------

    /// `query_processes_at`: the snapshot nearest `t_ms` (within 15 s), else the
    /// top-5 minute containing it, else the top-5 15 minutes containing it, else `None`.
    pub fn processes_at(&mut self, host: HostId, t_ms: i64) -> Result<Option<ProcessesAt>> {
        let host_ref = self.host_ref(host)?;
        let snap: Option<(i64, Vec<u8>)> = self
            .conn
            .prepare_cached(
                "SELECT ts, blob FROM proc_snap
                 WHERE host_id = ?1 AND ts BETWEEN ?2 - ?3 AND ?2 + ?3
                 ORDER BY abs(ts - ?2), ts LIMIT 1",
            )?
            .query_row(params![host_ref, t_ms, PROC_SNAP_TOLERANCE_MS], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        let (ts, packed, resolution) = match snap {
            Some((ts, b)) => (ts, b, ProcResolution::Snapshot),
            None => {
                let minute = Tier::M1.bucket_start(t_ms).unwrap_or(t_ms);
                let top: Option<Vec<u8>> = self
                    .conn
                    .prepare_cached(
                        "SELECT blob FROM proc_top_1m WHERE host_id = ?1 AND bucket_ts = ?2",
                    )?
                    .query_row(params![host_ref, minute], |r| r.get(0))
                    .optional()?;
                match top {
                    Some(b) => (minute, b, ProcResolution::Top5PerMinute),
                    None => {
                        let quarter = Tier::M15.bucket_start(t_ms).unwrap_or(t_ms);
                        let top: Option<Vec<u8>> = self
                            .conn
                            .prepare_cached(
                                "SELECT blob FROM proc_top_15m WHERE host_id = ?1 AND bucket_ts = ?2",
                            )?
                            .query_row(params![host_ref, quarter], |r| r.get(0))
                            .optional()?;
                        match top {
                            Some(b) => (quarter, b, ProcResolution::Top5Per15Minutes),
                            None => return Ok(None),
                        }
                    }
                }
            }
        };
        let mut names: HashMap<u32, String> = HashMap::new();
        let mut rows = Vec::new();
        for p in blob::unpack_procs(&packed)? {
            let name = match names.get(&p.name_id) {
                Some(n) => n.clone(),
                None => {
                    let n: String = self
                        .conn
                        .prepare_cached("SELECT name FROM proc_names WHERE id = ?1")?
                        .query_row([p.name_id], |r| r.get(0))
                        .optional()?
                        .ok_or_else(|| {
                            StoreError::Corrupt(format!("missing process name {}", p.name_id))
                        })?;
                    names.insert(p.name_id, n.clone());
                    n
                }
            };
            rows.push(ProcRow {
                name,
                pid: p.pid,
                cpu_pct: p.cpu,
                mem_bytes: u64::from(p.mem_kib) * 1024,
                threads: p.threads,
                idle_wakeups_per_s: p.wakeups,
                energy: p.energy,
            });
        }
        Ok(Some(ProcessesAt {
            ts_ms: ts,
            resolution,
            rows,
        }))
    }

    // --- cursors --------------------------------------------------------------------------

    /// Exporter side of sync: up to `max_rows` persisted rows of `host` in `tier` with
    /// `seq` after the cursor, in `seq` order. `M1` pages also carry gaps and events
    /// (D-041). Only the row kinds in `kinds` (what the connection negotiated) are read;
    /// the page's `last_seq` still moves past the others, which is why the receiver keeps
    /// `kinds` with its cursor (D-064). `M15` rows travel only with
    /// [`SyncRowKind::M15`]. A cursor from another database (`epoch` differs) reads from
    /// the start; a cursor that pruning has passed gets [`CursorRead::Truncated`]. For a
    /// receiver without `M15`, minutes rolled down into M15 count as pruned from its M1
    /// cursor, so it records a gap rather than a silent hole (D-076).
    pub fn read_after(
        &mut self,
        host: HostId,
        tier: Tier,
        after: Option<Cursor>,
        max_rows: u32,
        kinds: SyncKinds,
    ) -> Result<CursorRead> {
        let host_ref = self.host_ref(host)?;
        let table = tier_table(tier)?;
        let after_seq = match after {
            Some(c) if c.epoch == self.epoch => {
                let mut earliest = None;
                if let Some((pruned_seq, cutoff)) = self.pruned(host_ref, tier)?
                    && pruned_seq > c.seq
                {
                    earliest = Some(cutoff);
                }
                if tier == Tier::M1
                    && !kinds.contains(SyncRowKind::M15)
                    && let Some((rolled_seq, cutoff)) = self.pruned_key(host_ref, db::M1_ROLLED)?
                    && rolled_seq > c.seq
                {
                    earliest = earliest.max(Some(cutoff));
                }
                if let Some(earliest_ts_ms) = earliest {
                    return Ok(CursorRead::Truncated {
                        tier,
                        earliest_ts_ms,
                        epoch: self.epoch,
                    });
                }
                c.seq
            }
            _ => 0,
        };
        let limit = i64::from(max_rows) + 1;

        enum Item {
            Row(i64, u32, Vec<u8>),
            Gap(Gap),
            Event(i64, String, Vec<u8>),
        }
        let mut items: Vec<(i64, Item)> = Vec::new();
        let bucket_kind = if tier == Tier::M15 {
            SyncRowKind::M15
        } else {
            SyncRowKind::Buckets
        };
        if kinds.contains(bucket_kind) {
            items = self
                .conn
                .prepare_cached(&format!(
                    "SELECT seq, bucket_ts, layout_id, blob FROM {table}
                     WHERE host_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3"
                ))?
                .query_map(params![host_ref, after_seq, limit], |r| {
                    Ok((r.get(0)?, Item::Row(r.get(1)?, r.get(2)?, r.get(3)?)))
                })?
                .collect::<rusqlite::Result<_>>()?;
        }
        if tier == Tier::M1 && kinds.contains(SyncRowKind::Gaps) {
            let gaps = self
                .conn
                .prepare_cached(
                    "SELECT start_ts, end_ts, module, reason, seq FROM gaps
                     WHERE host_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
                )?
                .query_map(params![host_ref, after_seq, limit], |r| {
                    Ok((r.get(4)?, Item::Gap(gap_from_row(r)?)))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            items.extend(gaps);
        }
        if tier == Tier::M1 && kinds.contains(SyncRowKind::Events) {
            let events = self
                .conn
                .prepare_cached(
                    "SELECT seq, ts, kind, payload FROM events
                     WHERE host_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
                )?
                .query_map(params![host_ref, after_seq, limit], |r| {
                    Ok((r.get(0)?, Item::Event(r.get(1)?, r.get(2)?, r.get(3)?)))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            items.extend(events);
        }
        items.sort_by_key(|(seq, _)| *seq);
        let more = items.len() > max_rows as usize;
        items.truncate(max_rows as usize);

        let mut page = CursorPage {
            tier,
            epoch: self.epoch,
            kinds,
            layouts: Vec::new(),
            rows: Vec::new(),
            gaps: Vec::new(),
            events: Vec::new(),
            last_seq: items.last().map_or(after_seq, |(seq, _)| *seq),
            more,
        };
        let mut layout_nos: HashMap<u32, u32> = HashMap::new();
        for (seq, item) in items {
            match item {
                Item::Row(bucket_ts, layout_id, stats) => {
                    let layout_no = match layout_nos.get(&layout_id) {
                        Some(&n) => n,
                        None => {
                            let n = page.layouts.len() as u32;
                            page.layouts.push(PageLayout {
                                layout_no: n,
                                series: self.layout(layout_id)?.to_vec(),
                            });
                            layout_nos.insert(layout_id, n);
                            n
                        }
                    };
                    page.rows.push(PageRow {
                        seq,
                        bucket_ts,
                        layout_no,
                        stats: blob::unpack_f32s(&stats)?,
                    });
                }
                Item::Gap(gap) => page.gaps.push(PageGap { seq, gap }),
                Item::Event(ts_ms, kind, payload) => page.events.push(PageEvent {
                    seq,
                    ts_ms,
                    kind,
                    payload,
                }),
            }
        }
        Ok(CursorRead::Page(page))
    }

    /// Controller side: the stored sync position for `(host, tier)` and the row kinds it
    /// covered, if any.
    pub fn cursor(&mut self, host: HostId, tier: Tier) -> Result<Option<(Cursor, SyncKinds)>> {
        let host_ref = self.host_ref(host)?;
        let row: Option<(String, i64, String)> = self
            .conn
            .prepare_cached(
                "SELECT epoch, seq, kinds FROM cursors WHERE host_id = ?1 AND tier = ?2",
            )?
            .query_row(params![host_ref, tier.as_str()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        row.map(|(epoch, seq, kinds)| {
            let epoch = Uuid::parse_str(&epoch)
                .map_err(|e| StoreError::Corrupt(format!("cursor epoch {epoch}: {e}")))?;
            Ok((Cursor { epoch, seq }, SyncKinds::from_stored(&kinds)))
        })
        .transpose()
    }

    /// Controller side: the cursor to send in a `SyncRequest` to a remote database whose
    /// current epoch is `remote_epoch`, on a connection that negotiated `kinds`. `None`
    /// (start from the earliest row) when there is no cursor, it belongs to an earlier
    /// incarnation of that database, or it was built without one of `kinds`: the sender
    /// moved it past rows of that kind it could not send then (D-064). Replaying is
    /// harmless, ingest is idempotent.
    pub fn resume_cursor(
        &mut self,
        host: HostId,
        tier: Tier,
        remote_epoch: Uuid,
        kinds: SyncKinds,
    ) -> Result<Option<Cursor>> {
        Ok(self
            .cursor(host, tier)?
            .filter(|(c, covered)| c.epoch == remote_epoch && covered.covers(kinds))
            .map(|(c, _)| c))
    }
}

/// `query_history` from the engine's recent bucket rows alone (D-092): what a history
/// query can still answer while the store is unavailable and the app runs live-only.
/// `Auto` reads the 10 s tier when the range fits in its window, else minutes. No gaps
/// are on record.
pub fn history_recent(q: &HistoryQuery, recent: &[BucketRow]) -> Result<HistoryResult> {
    let tier = match q.tier {
        TierChoice::Fixed(t) => t,
        TierChoice::Auto if q.to_ms - q.from_ms <= Retention::default().s10_ms => Tier::S10,
        TierChoice::Auto => Tier::M1,
    };
    let width = tier_width(tier)?;
    if q.to_ms <= q.from_ms || q.selectors.is_empty() {
        return Ok(HistoryResult {
            tier,
            bucket_ms: width,
            series: Vec::new(),
            gaps: Vec::new(),
        });
    }
    let mut merge = Merge::new(q, width);
    let recent = merge.recent(recent, tier);
    merge.add_recent(&recent)?;
    Ok(HistoryResult {
        tier,
        bucket_ms: merge.slot_w,
        series: merge.finish(),
        gaps: Vec::new(),
    })
}

/// A history read in progress: slots of whole buckets, per-series accumulators by slot.
struct Merge<'q> {
    q: &'q HistoryQuery,
    /// Start of the bucket holding `from`.
    base: i64,
    /// The width each returned point averages over, which the client labels
    /// ("10 MIN AVG"): as few whole buckets as keep the slot count at or under
    /// `max_points`.
    slot_w: i64,
    /// Result series in key order.
    index: BTreeMap<SeriesKey, usize>,
    /// Per series, accumulators by slot index.
    accs: Vec<BTreeMap<i64, Acc>>,
}

impl<'q> Merge<'q> {
    fn new(q: &'q HistoryQuery, width: i64) -> Self {
        let base = q.from_ms - q.from_ms.rem_euclid(width);
        let buckets = (q.to_ms - base + width - 1) / width;
        let per_slot =
            (buckets + i64::from(q.max_points.max(1)) - 1) / i64::from(q.max_points.max(1));
        Self {
            q,
            base,
            slot_w: width * per_slot.max(1),
            index: BTreeMap::new(),
            accs: Vec::new(),
        }
    }

    /// The recent rows a read of `tier` takes: that tier's (minutes for an M15 read),
    /// inside the range.
    fn recent<'r>(&self, rows: &'r [BucketRow], tier: Tier) -> Vec<&'r BucketRow> {
        let want = if tier == Tier::M15 { Tier::M1 } else { tier };
        rows.iter()
            .filter(|r| r.tier == want && r.bucket_ts >= self.base && r.bucket_ts < self.q.to_ms)
            .collect()
    }

    /// (result index, position in layout) of each series in `layout` a selector matches.
    fn positions(&mut self, layout: &[SeriesKey]) -> Positions {
        let mut m = Vec::new();
        for (pos, key) in layout.iter().enumerate() {
            if self.q.selectors.iter().any(|s| s.matches(key)) {
                let next = self.index.len();
                let i = *self.index.entry(key.clone()).or_insert(next);
                if i == self.accs.len() {
                    self.accs.push(BTreeMap::new());
                }
                m.push((i, pos));
            }
        }
        m
    }

    /// Adds one bucket row; `stat(i)` is the `i`th value of its `(min, max, avg)` triples.
    fn add(
        &mut self,
        ts: i64,
        positions: &[(usize, usize)],
        stat: impl Fn(usize) -> Option<f32>,
        weight: u32,
    ) -> Result<()> {
        let slot = (ts - self.base) / self.slot_w;
        for &(i, pos) in positions {
            let (Some(min), Some(max), Some(avg)) =
                (stat(pos * 3), stat(pos * 3 + 1), stat(pos * 3 + 2))
            else {
                return Err(StoreError::Corrupt(format!(
                    "bucket at {ts} is shorter than its layout"
                )));
            };
            // NaN: in the layout but never sampled in this bucket. Not a value.
            if avg.is_nan() {
                continue;
            }
            if let Some(a) = self.accs.get_mut(i) {
                a.entry(slot).or_default().add(min, max, avg, weight);
            }
        }
        Ok(())
    }

    /// Adds the recent rows, each weighing one bucket of its tier.
    fn add_recent(&mut self, rows: &[&BucketRow]) -> Result<()> {
        // Rows of one layout share one `Arc`.
        let mut matches: Vec<(Arc<[SeriesKey]>, Positions)> = Vec::new();
        for r in rows {
            let k = match matches.iter().position(|(l, _)| Arc::ptr_eq(l, &r.series)) {
                Some(k) => k,
                None => {
                    let m = self.positions(&r.series);
                    matches.push((Arc::clone(&r.series), m));
                    matches.len() - 1
                }
            };
            let positions = matches.get(k).map_or(&[][..], |(_, m)| m.as_slice());
            self.add(r.bucket_ts, positions, |i| r.stats.get(i).copied(), 1)?;
        }
        Ok(())
    }

    fn finish(mut self) -> Vec<SeriesPoints> {
        let mut series = Vec::new();
        for (key, i) in std::mem::take(&mut self.index) {
            let Some(slots) = self.accs.get_mut(i) else {
                continue;
            };
            let points: Vec<Point> = std::mem::take(slots)
                .into_iter()
                .map(|(slot, a)| Point {
                    t: self.base + slot * self.slot_w,
                    min: a.min,
                    max: a.max,
                    avg: (a.sum / f64::from(a.weight)) as f32,
                })
                .collect();
            if !points.is_empty() {
                series.push(SeriesPoints { key, points });
            }
        }
        series
    }
}

/// (result index, position in layout) of each series of a layout a read selects.
type Positions = Vec<(usize, usize)>;

/// The series of layout `id`, through `cache`. A free function so a caller can hold a
/// statement on `conn` while it resolves layouts (the borrows are disjoint).
fn load_layout(
    conn: &Connection,
    cache: &mut HashMap<u32, Arc<[SeriesKey]>>,
    id: u32,
) -> Result<Arc<[SeriesKey]>> {
    if let Some(l) = cache.get(&id) {
        return Ok(Arc::clone(l));
    }
    let packed: Vec<u8> = conn
        .prepare_cached("SELECT series_ids FROM layouts WHERE id = ?1")?
        .query_row([id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| StoreError::Corrupt(format!("row refers to missing layout {id}")))?;
    let mut keys = Vec::new();
    for sid in blob::unpack_u32s(&packed)? {
        let (metric, labels): (String, String) = conn
            .prepare_cached("SELECT metric_id, labels FROM series WHERE id = ?1")?
            .query_row([sid], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .ok_or_else(|| {
                StoreError::Corrupt(format!("layout {id} names missing series {sid}"))
            })?;
        let labels = Labels::parse_canonical(&labels)
            .map_err(|e| StoreError::Corrupt(format!("series {sid} labels: {e}")))?;
        // Not validated: a metric from a newer peer is kept as-is and ignored by
        // readers that do not know it.
        keys.push(SeriesKey::new(MetricId(Cow::Owned(metric)), labels));
    }
    let layout: Arc<[SeriesKey]> = keys.into();
    cache.insert(id, Arc::clone(&layout));
    Ok(layout)
}

#[derive(Default)]
struct Acc {
    min: f32,
    max: f32,
    /// Sum of averages times their weights.
    sum: f64,
    weight: u32,
}

impl Acc {
    /// Adds one bucket's stats; `weight` is its width in tier units (minutes for an M15
    /// read, 1 otherwise).
    fn add(&mut self, min: f32, max: f32, avg: f32, weight: u32) {
        if self.weight == 0 {
            self.min = min;
            self.max = max;
        } else {
            self.min = self.min.min(min);
            self.max = self.max.max(max);
        }
        self.sum += f64::from(avg) * f64::from(weight);
        self.weight += weight;
    }
}

fn f32_at(blob: &[u8], i: usize) -> Option<f32> {
    let bytes = blob.get(i * 4..i * 4 + 4)?;
    Some(f32::from_le_bytes(bytes.try_into().ok()?))
}

/// Builds a [`Gap`] from `(start_ts, end_ts, module, reason)`. Text this build does not
/// know reads as `Unknown` (D-040).
fn gap_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Gap> {
    let module: Option<String> = r.get(2)?;
    let reason: String = r.get(3)?;
    Ok(Gap {
        start_ms: r.get(0)?,
        end_ms: r.get(1)?,
        module: module.map(|m| Module::parse(&m).unwrap_or(Module::Unknown)),
        reason: GapReason::from_stored(&reason),
    })
}

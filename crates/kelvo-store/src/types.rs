//! Public data types of the store API. Plain Rust: the app shell maps them to its
//! specta-typed IPC types, the sync driver maps them to `kelvo-proto` messages.

use std::sync::Arc;

use kelvo_schema::{Gap, HostId, SeriesKey, SeriesSelector, SyncKinds, Tier};
use uuid::Uuid;

/// One closed bucket of a persisted tier, as the engine's accumulators emit it.
#[derive(Clone, Debug, PartialEq)]
pub struct BucketRow {
    pub host: HostId,
    /// [`Tier::S10`] or [`Tier::M1`]. [`Tier::M15`] rows come from the store's own
    /// roll-down (or a peer's, through sync ingest), never from the engine.
    pub tier: Tier,
    /// Bucket start, ms epoch, host clock.
    pub bucket_ts: i64,
    /// The layout: series in blob order. Share one `Arc` per layout; the writer interns it
    /// once and caches the result.
    pub series: Arc<[SeriesKey]>,
    /// `(min, max, avg)` per series in `series` order. A series present in the layout but
    /// never sampled in the bucket is a `NaN` triple, never zeros.
    pub stats: Vec<f32>,
}

/// One process in a snapshot (v1-local-monitor.md 6.2, the fields the store keeps).
#[derive(Clone, Debug, PartialEq)]
pub struct ProcRow {
    pub name: String,
    pub pid: i32,
    /// Percent of one core.
    pub cpu_pct: f32,
    /// Stored in KiB, so the value read back is rounded down to a multiple of 1024 and
    /// capped at 4 TiB.
    pub mem_bytes: u64,
    pub threads: u32,
    pub idle_wakeups_per_s: f32,
    pub energy: f32,
}

/// How [`crate::Reader::processes_at`] found its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcResolution {
    /// A `proc_snap` row: the top 30 by CPU at one instant, kept 72 hours. One per 10 s,
    /// or per 30 s while Performance mode was on (D-088).
    Snapshot,
    /// A `proc_top_1m` row: the top 5 by CPU averaged over one minute, kept for the M1
    /// window (7 days).
    Top5PerMinute,
    /// A `proc_top_15m` row: the top 5 by CPU averaged over 15 minutes, for history older
    /// than the M1 window (D-076).
    Top5Per15Minutes,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessesAt {
    /// Timestamp of the snapshot or start of the minute.
    pub ts_ms: i64,
    pub resolution: ProcResolution,
    pub rows: Vec<ProcRow>,
}

/// One app's bytes in a network bucket or range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetApp {
    /// The app identity. `None` is "other apps": apps below a stored bucket's top 20,
    /// bytes whose app had no name, and new names past the interning cap.
    pub name: Option<String>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

impl NetApp {
    pub fn total(&self) -> u64 {
        self.rx_bytes.saturating_add(self.tx_bytes)
    }
}

/// One 10 s bucket of per-app network bytes, as the engine's accumulator closes it
/// (D-089). Interface counters cover the same span as the app bytes, so the remainder
/// (interface minus apps) is computed from one row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetBucket {
    /// How much of the bucket was measured, ms (at most 10,000). Less than the width when
    /// sampling started, stopped or reset inside it.
    pub measured_ms: u32,
    pub iface_rx_bytes: u64,
    pub iface_tx_bytes: u64,
    pub iface_rx_pkts: u64,
    pub iface_tx_pkts: u64,
    /// Bytes per app. Any number; the store keeps the top 20 by rx + tx and folds the rest
    /// into "other apps". Repeated names are summed.
    pub apps: Vec<NetApp>,
}

/// Which tier answered one sub-range of a [`NetByApp`] read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetSpan {
    pub from_ms: i64,
    pub to_ms: i64,
    /// `None`: nothing was recorded there (before collection, sleep, setting off).
    pub tier: Option<Tier>,
}

/// Per-app network bytes over a range ([`crate::Reader::net_by_app`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetByApp {
    /// The range the sums cover: the request widened to whole buckets of the tiers used.
    pub from_ms: i64,
    pub to_ms: i64,
    /// `[from_ms, to_ms)` split into consecutive sub-ranges by the tier that answered
    /// each, adjacent ones of the same tier merged. Spans with `tier: None` had no row.
    pub coverage: Vec<NetSpan>,
    /// Width of the coarsest bucket used, ms: the precision the range can honestly be
    /// labelled with. 10,000 when nothing was found.
    pub resolution_ms: i64,
    /// Sum of the buckets' measured spans. Below `to_ms - from_ms` when part of the range
    /// was not measured.
    pub measured_ms: u64,
    pub iface_rx_bytes: u64,
    pub iface_tx_bytes: u64,
    pub iface_rx_pkts: u64,
    pub iface_tx_pkts: u64,
    /// Every app with bytes in the range, "other apps" (`name: None`) included, largest
    /// rx + tx first.
    pub apps: Vec<NetApp>,
}

/// Which tier a history query reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TierChoice {
    /// `S10` when the 10 s tier still covers the start of the range; otherwise `M1` when
    /// minutes still cover it, else `M15`.
    Auto,
    Fixed(Tier),
}

/// `query_history` (v1-local-monitor.md 6.3).
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryQuery {
    pub host: HostId,
    pub selectors: Vec<SeriesSelector>,
    /// Inclusive start, ms epoch.
    pub from_ms: i64,
    /// Exclusive end, ms epoch.
    pub to_ms: i64,
    pub tier: TierChoice,
    /// Upper bound on points per series. Buckets are merged (min of mins, max of maxes,
    /// mean of averages) into equal slots to stay under it.
    pub max_points: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HistoryResult {
    /// The tier the points came from.
    pub tier: Tier,
    /// The width each point covers, ms: the tier's bucket times the buckets merged into
    /// one slot to stay under `max_points`. A point at `t` averages `[t, t + bucket_ms)`.
    pub bucket_ms: i64,
    /// One entry per series that matched a selector and has at least one point, in key
    /// order.
    pub series: Vec<SeriesPoints>,
    /// Gaps overlapping the range. Charts split series at these; never interpolate.
    pub gaps: Vec<Gap>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SeriesPoints {
    pub key: SeriesKey,
    pub points: Vec<Point>,
}

/// One merged slot. Slots with no measured value are absent, never zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Slot start, ms epoch.
    pub t: i64,
    pub min: f32,
    pub max: f32,
    pub avg: f32,
}

/// Retention per table (architecture.md, Store), plus the hard cap on the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retention {
    pub s10_ms: i64,
    /// How long `tier_1m` and `proc_top_1m` keep minutes before pruning rolls them into
    /// `tier_15m` and `proc_top_15m` (D-076). Capped by `history_ms`.
    pub m1_ms: i64,
    /// How long history is kept at all, from settings (`history.retention_days`):
    /// `tier_15m`, `proc_top_15m`, gaps and events, and the minute tables when this is
    /// not longer than `m1_ms`.
    pub history_ms: i64,
    /// How long `proc_snap` and `proc_net_10s` keep their 10 s rows.
    pub proc_snap_ms: i64,
    /// Hard cap on bytes on disk, WAL included (D-057). When time-based pruning leaves
    /// the file above it, the oldest history is trimmed until the live pages fit in
    /// [`Retention::low_water_bytes`].
    pub max_bytes: u64,
}

impl Retention {
    pub const DAY_MS: i64 = 24 * 3_600_000;
    /// The default cap: the 150 MB budget for 30 days (architecture.md, Budget math).
    pub const DEFAULT_MAX_BYTES: u64 = 150 * 1_000_000;

    /// Minutes are kept this long before they are rolled into 15-minute buckets.
    pub const M1_WINDOW_MS: i64 = 7 * Self::DAY_MS;

    /// The defaults with the history retention set from settings
    /// (`history.retention_days`).
    pub const fn with_days(days: u16) -> Self {
        Self {
            s10_ms: Self::DAY_MS,
            m1_ms: Self::M1_WINDOW_MS,
            history_ms: days as i64 * Self::DAY_MS,
            proc_snap_ms: 3 * Self::DAY_MS,
            max_bytes: Self::DEFAULT_MAX_BYTES,
        }
    }

    /// How long minutes are kept: the M1 window, or all of history when that is shorter.
    pub const fn m1_keep_ms(&self) -> i64 {
        if self.m1_ms < self.history_ms {
            self.m1_ms
        } else {
            self.history_ms
        }
    }

    /// Where a cap trim stops: 90% of the cap, so the file has room to grow for a day or
    /// two before the next trim instead of trimming a sliver every hour.
    pub const fn low_water_bytes(&self) -> u64 {
        self.max_bytes / 10 * 9
    }
}

/// One run of the 30-day fill test (`tests/fill.rs`): bytes on disk after close, 16 KiB
/// pages, for `series` persisted series over `days` days.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillMeasurement {
    pub series: u32,
    pub days: u32,
    pub bytes: u64,
}

/// The fill test before the roll-down (D-057): every day kept at 1-minute resolution. The
/// minute rows have not changed since, so with [`FILL_ROLLED`] it gives the fixed part of
/// a store and the cost of a minute day (Settings' size projection).
pub const FILL_MINUTES: [FillMeasurement; 2] = [
    FillMeasurement {
        series: 150,
        days: 30,
        bytes: 139_700_000,
    },
    FillMeasurement {
        series: 250,
        days: 30,
        bytes: 203_400_000,
    },
];

/// The fill test with the roll-down (D-076): 7 days of minutes and 23 of quarters.
pub const FILL_ROLLED: [FillMeasurement; 2] = [
    FillMeasurement {
        series: 150,
        days: 30,
        bytes: 69_900_000,
    },
    FillMeasurement {
        series: 250,
        days: 30,
        bytes: 95_700_000,
    },
];

impl Default for Retention {
    fn default() -> Self {
        Self::with_days(30)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub s10_rows: u64,
    /// M1 rows deleted past retention (not the ones rolled into M15).
    pub m1_rows: u64,
    /// M1 rows folded into M15 rows and deleted.
    pub m1_rolled: u64,
    /// M15 rows written by the roll-down.
    pub m15_written: u64,
    /// M15 rows deleted past retention.
    pub m15_rows: u64,
    pub gaps: u64,
    pub events: u64,
    /// `proc_snap` rows rolled into `proc_top_1m` and deleted.
    pub proc_snaps_rolled: u64,
    /// `proc_top_1m` rows written by the snapshot roll-down, plus `proc_top_1m` and
    /// `proc_top_15m` rows deleted past retention.
    pub proc_top_rows: u64,
    /// `proc_top_1m` rows rolled into `proc_top_15m` and deleted.
    pub proc_top_rolled: u64,
    /// `proc_net_10s` rows past 72 hours, plus `proc_net_1m` and `proc_net_15m` rows past
    /// retention, deleted.
    pub net_rows: u64,
    /// `proc_net_1m` rows rolled into `proc_net_15m` and deleted.
    pub net_rolled: u64,
    /// Present when retention alone left the file over [`Retention::max_bytes`] and the
    /// oldest history was trimmed. Its rows are not counted in the fields above.
    pub cap_trim: Option<CapTrim>,
    /// Bytes on disk (database, WAL and shared memory) after pruning and checkpointing.
    pub size_bytes: u64,
}

/// What the byte cap trimmed: everything before `earliest_ts_ms`, on every host. The
/// trimmed span reads as "no data yet", like the start of history; nothing marks it as a
/// gap, and every cursor behind it gets `Truncated`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CapTrim {
    /// The new start of history, ms epoch.
    pub earliest_ts_ms: i64,
    /// Bytes on disk before the trim (after time-based pruning).
    pub size_before: u64,
    pub m1_rows: u64,
    pub m15_rows: u64,
    pub s10_rows: u64,
    /// `proc_snap`, `proc_top_1m` and `proc_top_15m` rows.
    pub proc_rows: u64,
    /// `proc_net_10s`, `proc_net_1m` and `proc_net_15m` rows.
    pub net_rows: u64,
    pub gaps: u64,
    pub events: u64,
    /// False when the cap could not be met without trimming into the last 24 hours,
    /// which the trim never does.
    pub cap_met: bool,
}

/// What [`crate::Writer::begin_session`] repaired on startup.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionStart {
    /// Gaps left open by the previous run (a crash while asleep or paused), now closed.
    pub closed_gaps: u32,
    /// The `app_not_running` gap from the last persisted bucket to now, if any.
    pub app_not_running: Option<Gap>,
    /// End of the newest `proc_net_10s` row, ms epoch: the previous run flushed its open
    /// per-app bucket there at shutdown, so this run counts only after it (D-089).
    pub net_written_to_ms: Option<i64>,
}

/// A layout inside a [`CursorPage`]: page-local number and series keys in blob order.
#[derive(Clone, Debug, PartialEq)]
pub struct PageLayout {
    pub layout_no: u32,
    pub series: Vec<SeriesKey>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageRow {
    pub seq: i64,
    pub bucket_ts: i64,
    pub layout_no: u32,
    /// `(min, max, avg)` per series in the layout.
    pub stats: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageGap {
    pub seq: i64,
    pub gap: Gap,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageEvent {
    pub seq: i64,
    pub ts_ms: i64,
    pub kind: String,
    /// CBOR, opaque to the store.
    pub payload: Vec<u8>,
}

/// Persisted rows after a cursor, in `seq` order: the store-side twin of the proto
/// `SyncPage`. Gaps and events ride the `M1` cursor only (D-041); an `S10` or `M15` page
/// carries rows alone. Only the row kinds in `kinds` are present (D-064).
#[derive(Clone, Debug, PartialEq)]
pub struct CursorPage {
    pub tier: Tier,
    /// The exporting database's `db_instance_uuid`.
    pub epoch: Uuid,
    /// The row kinds negotiated for the connection the page travels on. Not on the wire
    /// (both sides know it from the handshake); the receiver stores it with its cursor,
    /// so a kind it did not cover is never skipped past.
    pub kinds: SyncKinds,
    pub layouts: Vec<PageLayout>,
    pub rows: Vec<PageRow>,
    pub gaps: Vec<PageGap>,
    pub events: Vec<PageEvent>,
    /// Highest `seq` in the page, or the request's cursor when the page is empty.
    pub last_seq: i64,
    pub more: bool,
}

/// Answer to [`crate::Reader::read_after`].
#[derive(Clone, Debug, PartialEq)]
pub enum CursorRead {
    Page(CursorPage),
    /// Rows after the cursor were pruned. The requester writes a `truncated` gap up to
    /// `earliest_ts_ms` and asks again from the start.
    Truncated {
        tier: Tier,
        earliest_ts_ms: i64,
        epoch: Uuid,
    },
}

/// What [`crate::Writer::ingest_page`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IngestReport {
    /// Rows inserted or changed.
    pub rows_written: u32,
    /// Rows already present with identical values (a replayed page).
    pub rows_unchanged: u32,
    pub gaps_written: u32,
    /// Gaps for a module this build does not know (D-040).
    pub gaps_skipped: u32,
    pub events_written: u32,
}

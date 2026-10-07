//! IPC-only types: what commands take and return, what live channels carry, and the
//! events Rust emits (v1-local-monitor.md 6.3). The data vocabulary itself (`HostRecord`,
//! `Capabilities`, `Settings`, `SeriesKey`, `Gap`) comes from `kelvo-schema`; the types
//! here wrap it for the webview. Exported to `src/core/generated/` by tauri-specta.
//!
//! Every `i64` timestamp carries `#[specta(type = JsSafeInt)]` (D-039).

use kelvo_schema::settings::Appearance;
use kelvo_schema::{
    Capabilities, Event, Gap, HostId, HostRecord, JsSafeInt, MetricKind, PerformanceReason,
    PowerSource, SeriesKey, SeriesSelector, Settings, Tier,
};
use serde::{Deserialize, Serialize};

use crate::process_signal::SignalRefusal;

/// A millisecond epoch timestamp passed as a plain JS `number` (D-039).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct Millis(#[specta(type = JsSafeInt)] pub i64);

/// A byte count as a plain JS `number`; exact below 2^53 (8 PiB).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(transparent)]
pub struct ByteCount(#[specta(type = JsSafeInt)] pub u64);

/// What history costs on this machine, measured from the database (`history_growth`):
/// the whole file, every host together, like `history_size`. The Settings projection
/// uses it in place of the fill-test model once present.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct HistoryGrowth {
    /// Recorded time the rates are measured over, ms.
    #[specta(type = JsSafeInt)]
    pub measured_ms: i64,
    /// Bytes that do not depend on retention once full: 24 h of 10 s rows and the
    /// process snapshot window, plus the small tables at their current size.
    pub fixed_bytes: ByteCount,
    /// Bytes one day of 1-minute history costs.
    pub minute_day_bytes: ByteCount,
}

impl From<kelvo_store::HistoryGrowth> for HistoryGrowth {
    fn from(g: kelvo_store::HistoryGrowth) -> Self {
        Self {
            measured_ms: g.measured_ms,
            fixed_bytes: ByteCount(g.fixed_bytes),
            minute_day_bytes: ByteCount(g.minute_day_bytes),
        }
    }
}

// --- live channel ---------------------------------------------------------------------------

/// One message on a window's live channel (D-049, D-066).
///
/// Order on a fresh subscription: the current `Caps` and `Status`; a `Layout` for every
/// layout the requested history uses; the most recent two minutes as `Backfill` segments
/// (each after its `Layout` when that differs from the last one sent); then frames, each
/// preceded by a `Layout` when the layout changes. History older than two minutes follows
/// as `BackfillEarlier` chunks, newest first, once the first frame is out: each chunk is
/// older than everything the channel sent before it.
///
/// After the channel resumes (window shown, display awake, the stream fell behind the
/// bus): `Caps` and `Status` if they changed, then `Backfill` for exactly the span missed,
/// in time order and in segments of at most 600 rows (one message each, so applying them
/// never blocks the webview for long), so `Backfill` rows come after everything already
/// received.
///
/// `timeline` on `Backfill` and `Frame` numbers the runs of the host's wall clock: the
/// host bumps it when its clock is stepped (D-064) and for nothing else. Rows with a
/// `timeline` other than the one held replace everything held at or after their first
/// row's time. After a step the channel sends the new timeline from its first row (as far
/// back as the window's backfill span reaches), so that time is where the step happened.
///
/// A subscription that named `series` gets only those series: `Layout.series` lists the
/// matching keys and every row and frame carries values for them alone.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LiveMsg {
    /// The series of every following `Backfill` and `Frame` with this `layout_no`, in
    /// value order, and each one's catalog kind (`gauge` for a metric this build does not
    /// know). `layout_no` numbers layouts within one engine run.
    Layout {
        layout_no: u32,
        series: Vec<SeriesKey>,
        kinds: Vec<MetricKind>,
    },
    /// Raw measured rows from the engine's one-hour ring: row `i` was taken at about
    /// `start_ms + i * interval_ms`. `null` where a series was not sampled. Segments never
    /// span a hole (sleep, pause, skipped ticks), so a reader never draws across one.
    /// `holds_ms` says, per series, how long a sample in these rows stays current (D-090).
    /// After a restart the ring starts with up to an hour of stored 10 s bucket averages
    /// under their own layout (`layout_no` 4294967295, D-097), so these rows can be that.
    Backfill {
        layout_no: u32,
        #[specta(type = JsSafeInt)]
        start_ms: i64,
        interval_ms: u32,
        timeline: u32,
        rows: Vec<Vec<Option<f32>>>,
        holds_ms: Vec<u32>,
    },
    /// History older than everything sent so far on this channel, in the same shape as
    /// `Backfill`. Its `layout_no` was announced by an earlier `Layout`.
    BackfillEarlier {
        layout_no: u32,
        #[specta(type = JsSafeInt)]
        start_ms: i64,
        interval_ms: u32,
        rows: Vec<Vec<Option<f32>>>,
        holds_ms: Vec<u32>,
    },
    /// How long each series' sample stays current in the frames that follow, in
    /// `layout_no` order (D-090). Sent before the first frame and whenever it changes: a
    /// collector slowing down with no window open, back-off on battery, a thinned
    /// stream. Two samples of a series further apart than the earlier one's hold have a
    /// gap between them; closer ones are one run.
    Holds {
        layout_no: u32,
        holds_ms: Vec<u32>,
    },
    /// One tick. `values` is what was measured on this tick (`null` for a series not
    /// sampled now); draw charts from it. `held` is the latest value of each series while
    /// it is still current (`null` once stale, D-047); read current numbers and build the
    /// Snapshot from it. Both are in `layout_no` order.
    Frame {
        #[specta(type = JsSafeInt)]
        ts_ms: i64,
        layout_no: u32,
        timeline: u32,
        values: Vec<Option<f32>>,
        held: Vec<Option<f32>>,
    },
    /// Process rows, sent only while this window has process interest for the host:
    /// every readable process, or the top rows its [`ProcessView`] asked for, sorted by
    /// the view's first sort key.
    Processes {
        #[specta(type = JsSafeInt)]
        ts_ms: i64,
        rows: Vec<LiveProcess>,
    },
    Caps {
        capabilities: Capabilities,
    },
    /// Engine state, sent when it changes.
    Status(LiveStatus),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct LiveStatus {
    /// The base tick in effect, after battery back-off.
    pub interval_ms: u32,
    /// How often this channel sends frames: the base tick, or the multiple of it the
    /// subscription's `min_period_ms` thins frames to. Stale is measured in these.
    pub frame_period_ms: u32,
    pub paused: bool,
    /// The display is asleep or the screen locked: no frames are sent until it wakes,
    /// then the missed span arrives as `Backfill`. Not stale.
    pub display_idle: bool,
    pub on_battery: bool,
    /// Whether Performance mode is in effect, and why: the setting or Low Power Mode
    /// (D-088). Frames are paced to at least 2 s while it is.
    pub performance: PerformanceReason,
    /// Battery, adapter or charging, read with `on_battery`; a Mac without a battery is
    /// `adapter` (D-092).
    pub power_source: PowerSource,
    /// The `iface` label of the reported interface carrying the default route, `null`
    /// when the route is on an interface Kelvo does not report (a VPN tunnel) or there is
    /// none. Re-read on interface changes and every 60 s (D-092).
    pub primary_iface: Option<String>,
}

/// One process at one sample (v1-local-monitor.md 6.2).
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct LiveProcess {
    pub pid: i32,
    /// Process start, microseconds since the Unix epoch. With `pid` it identifies the
    /// process, since pids are reused.
    #[specta(type = JsSafeInt)]
    pub start_time_us: i64,
    pub name: String,
    /// Percent of one core; can exceed 100.
    pub cpu_pct: f32,
    #[specta(type = JsSafeInt)]
    pub mem_bytes: u64,
    /// `null` when the platform does not say.
    #[specta(type = Option<JsSafeInt>)]
    pub compressed_bytes: Option<u64>,
    pub threads: u32,
    pub idle_wakeups_per_s: f32,
    pub energy: f32,
    pub disk_read_bps: f32,
    pub disk_write_bps: f32,
    /// Bytes per second received, loopback excluded. `null` when per-process network was
    /// not sampled: no visible view asked for it (`ProcessView.network`), or
    /// `Capabilities.process_network` is false. Covers the current user's processes only
    /// (D-081).
    pub net_rx_bps: Option<f32>,
    /// Bytes per second sent; see `net_rx_bps`.
    pub net_tx_bps: Option<f32>,
    /// Share of the whole GPU over the interval, 0 to 100: the GPU time the process's
    /// work used divided by wall time. `null` when not sampled: no visible view asked for
    /// it (`ProcessView.gpu`), or `Capabilities.process_gpu` is false. Approximate while
    /// a long compute job runs: time is charged when a command buffer completes (D-085).
    pub gpu_pct: Option<f32>,
    /// TCP ports the process listens on, ascending; empty when none. `null` when not read:
    /// no visible view asked for them (`ProcessView.ports`). Covers the current user's
    /// processes only, like every row.
    pub ports: Option<Vec<u16>>,
    pub user: String,
    /// Why Quit and Force Quit refuse this process, `null` when they may signal it: the
    /// rule `process_signal` checks, with Kelvo's own processes from the self-CPU
    /// collector's list (D-092).
    pub refusal: Option<SignalRefusal>,
}

/// How a window wants process rows (`set_process_interest`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ProcessView {
    /// Rows per sort key: the batch is the union of the top `limit` processes by each
    /// key in `sort`. `null` sends every readable process (the Processes table).
    pub limit: Option<u16>,
    /// The keys the window ranks by. Empty means CPU.
    pub sort: Vec<ProcessSort>,
    /// At most one batch per this many ms (`null`: every sample, each base tick). The
    /// collector itself slows down to the shortest period any visible window asks for.
    pub period_ms: Option<u32>,
    /// The window shows per-process network rates. While a visible window does, the host
    /// samples NetworkStatistics on the process ticks; otherwise it holds nothing open
    /// and rows carry `null` rates (D-081).
    #[serde(default)]
    pub network: bool,
    /// The window shows per-process GPU time. While a visible window does, the host reads
    /// the GPU's IORegistry clients on the process ticks; otherwise rows carry `null`.
    #[serde(default)]
    pub gpu: bool,
    /// The window shows the TCP ports processes listen on. While a visible window does,
    /// the host reads each process's sockets on the process ticks (each process at most
    /// every 5 s); otherwise rows carry `null`.
    #[serde(default)]
    pub ports: bool,
}

/// A descending sort key for [`ProcessView`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProcessSort {
    Cpu,
    Memory,
    Threads,
    Wakeups,
    Energy,
    DiskRead,
    DiskWrite,
    /// Read plus write.
    DiskTotal,
    /// Network receive rate. Rows without a rate rank last.
    NetRx,
    /// Network send rate.
    NetTx,
    /// Receive plus send.
    NetTotal,
    /// Share of the GPU. Rows without a value rank last.
    Gpu,
}

/// What `subscribe_live` did before returning.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct SubscriptionInfo {
    pub host: HostId,
    /// This subscription's id (the channel's): pass it to `set_process_interest` so the
    /// interest belongs to this page load and ends with it.
    pub stream: u32,
    /// Start of the backfill already sent on the channel, `null` when none was (the ring is
    /// empty, or the window is hidden and the channel starts when it is shown).
    #[specta(type = Option<JsSafeInt>)]
    pub backfill_start_ms: Option<i64>,
    /// Backfill rows sent, over all segments.
    pub backfill_rows: u32,
    /// Start of the older history that follows as `BackfillEarlier` chunks, `null` when
    /// none does.
    #[specta(type = Option<JsSafeInt>)]
    pub earlier_start_ms: Option<i64>,
    /// Rows still to come in `BackfillEarlier` chunks.
    pub earlier_rows: u32,
}

// --- history --------------------------------------------------------------------------------

/// Which tier `query_history` reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TierRequest {
    /// 10 s buckets while they cover the start of the range (24 h); otherwise 1 min while
    /// minutes cover it (7 days), else 15 min.
    Auto,
    S10,
    M1,
}

#[derive(Clone, Debug, PartialEq, Deserialize, specta::Type)]
pub struct HistoryRequest {
    pub host: HostId,
    pub selectors: Vec<SeriesSelector>,
    /// Inclusive start, ms epoch.
    #[specta(type = JsSafeInt)]
    pub from_ms: i64,
    /// Exclusive end, ms epoch.
    #[specta(type = JsSafeInt)]
    pub to_ms: i64,
    pub tier: TierRequest,
    /// Upper bound on points per series; buckets are merged to stay under it.
    pub max_points: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct HistoryPage {
    /// The tier the points came from.
    pub tier: Tier,
    /// The width each point averages over, ms: the tier's bucket times the buckets merged
    /// to stay under `max_points` (a 7d range at 1,008 points: 600,000, "10 MIN AVG"). A
    /// point at `t` covers `[t, t + bucket_ms)`.
    #[specta(type = JsSafeInt)]
    pub bucket_ms: i64,
    pub series: Vec<HistorySeries>,
    /// Gaps overlapping the range. Split lines at these; never interpolate.
    pub gaps: Vec<Gap>,
}

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct HistorySeries {
    pub key: SeriesKey,
    pub points: Vec<HistoryPoint>,
    /// How long a point stays current, ms (D-092): the hold of the slowest period the
    /// series is sampled at, or the page's `bucket_ms` if that is larger. Consecutive
    /// points no more than this apart are one line; further apart, there is a hole
    /// between them. `gaps` stays the list of explicit holes.
    ///
    /// The period is the slowest under the current settings (the backed-off base tick,
    /// the collector's idle cadence, Performance mode's slowdown), or, when the range
    /// starts before the last time a period got faster in this app run (a shorter
    /// sampling interval), the slowest under any settings this run, since the range's
    /// earlier points were sampled under those. Settings persist across launches, so
    /// earlier runs count as sampled under the settings this one started with.
    #[specta(type = JsSafeInt)]
    pub hold_ms: i64,
}

/// One merged slot. Slots with no measured value are absent, never zero.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, specta::Type)]
pub struct HistoryPoint {
    #[specta(type = JsSafeInt)]
    pub t: i64,
    pub min: f32,
    pub max: f32,
    pub avg: f32,
}

/// One local hour of `battery_hours`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, specta::Type)]
pub struct BatteryHour {
    /// The hour's start, as the request gave it.
    #[specta(type = JsSafeInt)]
    pub start_ms: i64,
    /// `battery.charge` in the hour's last minute that sampled it; `null` when none did,
    /// never 0.
    pub charge: Option<f32>,
    /// Whether `battery.charging` was on in any minute of the hour.
    pub charging: bool,
}

/// The series a heatmap shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum HeatmapMetric {
    /// `cpu.total`.
    Cpu,
    /// `thermal.hottest`.
    Temp,
}

/// `query_heatmap`: hourly averages for local days. The store knows no time zones: the
/// frontend sends each local day's hour boundaries in UTC, with DST applied, and Rust
/// averages whatever buckets start inside each hour.
#[derive(Clone, Debug, PartialEq, Deserialize, specta::Type)]
pub struct HeatmapRequest {
    pub host: HostId,
    pub metric: HeatmapMetric,
    /// Oldest first, at most 92.
    pub days: Vec<HeatmapDaySpec>,
}

/// One local day of a [`HeatmapRequest`].
#[derive(Clone, Debug, PartialEq, Deserialize, specta::Type)]
pub struct HeatmapDaySpec {
    /// The local date (`2026-10-04`), echoed back in [`HeatmapDay`].
    pub date: String,
    /// 25 instants: the start of each local hour 00 to 23, then the next local midnight.
    /// Never decreasing. A local hour that does not exist (spring forward) starts where
    /// the next one does, so it is empty; a local hour that happens twice (fall back) runs
    /// to the start of the hour after, two hours later.
    pub hour_starts: Vec<Millis>,
}

/// One local day of the heatmap.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct HeatmapDay {
    pub date: String,
    /// 24 hourly averages (percent for `cpu`, °C for `temp`). `null` when no bucket in
    /// the hour sampled the series (asleep, off, not yet, or an hour that did not exist),
    /// never 0.
    pub hours: Vec<Option<f32>>,
}

/// `export_csv`: what to write. Rust asks where with a save dialog.
#[derive(Clone, Debug, PartialEq, Deserialize, specta::Type)]
pub struct ExportRequest {
    pub host: HostId,
    pub selectors: Vec<SeriesSelector>,
    /// Inclusive start, ms epoch.
    #[specta(type = JsSafeInt)]
    pub from_ms: i64,
    /// Exclusive end, ms epoch.
    #[specta(type = JsSafeInt)]
    pub to_ms: i64,
    /// `auto` writes the tier a history read of the range would use, one row per bucket
    /// (no merging).
    pub tier: TierRequest,
    /// The file name the dialog proposes (`kelvo-2026-10-04.csv`); local dates are the
    /// frontend's to format. Path separators are dropped. Default `kelvo-history.csv`.
    #[specta(optional)]
    pub file_name: Option<String>,
}

/// What `export_csv` did. Cancelling the dialog is not an error.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExportOutcome {
    Saved {
        /// Where the file went.
        path: String,
        /// Bucket rows written, not counting the header or gap rows.
        #[specta(type = JsSafeInt)]
        rows: u64,
        /// Gap rows written.
        #[specta(type = JsSafeInt)]
        gap_rows: u64,
        /// File size.
        #[specta(type = JsSafeInt)]
        bytes: u64,
    },
    /// The user closed the dialog. Nothing was written.
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProcessResolution {
    /// The top 30 by CPU at one instant (every 10 s, or 30 s in Performance mode; kept
    /// 72 hours).
    Snapshot,
    /// The top 5 by CPU averaged over one minute (the last 7 days).
    Top5PerMinute,
    /// The top 5 by CPU averaged over 15 minutes (older than 7 days, for the rest of the
    /// retention period).
    #[serde(rename = "top5_per_15_minutes")]
    Top5Per15Minutes,
}

/// `query_processes_at`: the stored processes nearest a moment.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct ProcessesAt {
    /// The snapshot's time, or the start of the minute or 15 minutes.
    #[specta(type = JsSafeInt)]
    pub ts_ms: i64,
    pub resolution: ProcessResolution,
    pub rows: Vec<StoredProcess>,
}

/// A process as the store keeps it: fewer fields than [`LiveProcess`].
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct StoredProcess {
    pub name: String,
    pub pid: i32,
    pub cpu_pct: f32,
    #[specta(type = JsSafeInt)]
    pub mem_bytes: u64,
    pub threads: u32,
    pub idle_wakeups_per_s: f32,
    pub energy: f32,
}

/// One sub-range of a `query_network_by_app` range and the tier that answered it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct NetworkSpan {
    #[specta(type = JsSafeInt)]
    pub from_ms: i64,
    #[specta(type = JsSafeInt)]
    pub to_ms: i64,
    /// `s10`, `m1` or `m15`; `null` when nothing was recorded there (before collection
    /// started, asleep, paused, Network history off).
    pub tier: Option<Tier>,
}

/// One app's bytes over a range. Bytes are exact below 2^53.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct AppBytes {
    /// The app as a user names it ("Google Chrome", "Safari", `curl`), D-089.
    pub name: String,
    #[specta(type = JsSafeInt)]
    pub rx_bytes: u64,
    #[specta(type = JsSafeInt)]
    pub tx_bytes: u64,
}

/// `query_network_by_app`: which apps moved the interface's bytes over a range (D-089).
///
/// Per direction, outside a clamp, the parts add up to the interface total exactly:
/// `sum(apps) + other_apps + overhead + system == iface`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct NetworkByApp {
    /// The range the sums cover: the request widened to whole buckets (10 s, or 1 m and
    /// 15 m for older history), so the UI should draw this, not what it asked for.
    #[specta(type = JsSafeInt)]
    pub from_ms: i64,
    #[specta(type = JsSafeInt)]
    pub to_ms: i64,
    /// Buckets before this are complete; from here to `to_ms` they are still open in the
    /// engine (one of the per-app and interface streams has not reported past them) or
    /// not reached yet, so they still grow and their remainder is not meaningful yet.
    /// With nothing open, the end of what the engine closed, or the start of the 10 s
    /// bucket it last (re)started in. Equal to `to_ms` when
    /// the whole range is complete, and may be before `from_ms` when none of it is. A
    /// range is final, and can be cached, only once this reaches `to_ms`.
    #[specta(type = JsSafeInt)]
    pub complete_to_ms: i64,
    /// Width of the coarsest bucket used, ms: the precision the range can be labelled
    /// with. 10,000 when nothing was recorded.
    #[specta(type = JsSafeInt)]
    pub resolution_ms: i64,
    /// How much of `[from_ms, to_ms)` was measured, ms ("measured for 40 of 90 s").
    #[specta(type = JsSafeInt)]
    pub measured_ms: u64,
    /// `[from_ms, to_ms)` in consecutive spans by the tier that answered each.
    pub coverage: Vec<NetworkSpan>,
    /// Named apps, largest rx + tx first. "Other apps" is not among them.
    pub apps: Vec<AppBytes>,
    /// Apps folded together: below a stored bucket's top 20, nameless, or past the cap
    /// on new names per hour.
    #[specta(type = JsSafeInt)]
    pub other_apps_rx_bytes: u64,
    #[specta(type = JsSafeInt)]
    pub other_apps_tx_bytes: u64,
    /// Interface totals over the measured spans.
    #[specta(type = JsSafeInt)]
    pub iface_rx_bytes: u64,
    #[specta(type = JsSafeInt)]
    pub iface_tx_bytes: u64,
    /// "Protocol overhead (est.)": interface packets times a per-packet header size,
    /// shrunk to what the apps leave of the interface total.
    #[specta(type = JsSafeInt)]
    pub overhead_rx_bytes: u64,
    #[specta(type = JsSafeInt)]
    pub overhead_tx_bytes: u64,
    /// "System and other": interface minus apps, other apps and overhead, never below 0.
    /// Root daemons (software update, mDNSResponder, backups) that Kelvo cannot see.
    #[specta(type = JsSafeInt)]
    pub system_rx_bytes: u64,
    #[specta(type = JsSafeInt)]
    pub system_tx_bytes: u64,
    /// The apps moved more than the interface did: late bytes landing in a later bucket,
    /// or history from before per-app bytes counted only the reported interfaces.
    /// `system_*` is then 0 and the parts exceed the total.
    pub clamped: bool,
}

/// What `query_usage_by_app` sorts apps by (D-099).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UsageKey {
    /// Average CPU.
    Cpu,
    /// Average GPU.
    Gpu,
    /// Peak footprint.
    Memory,
    /// Bytes read plus written.
    Disk,
    /// Joules.
    Energy,
}

/// One process's use over a `query_usage_by_app` range: an expanded app row. Kept only
/// above a floor, so an app's processes need not add up to it.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct ProcessUsage {
    pub pid: i32,
    /// With `pid`, the process's identity, as on `LiveProcess`.
    #[specta(type = JsSafeInt)]
    pub start_time_us: i64,
    pub name: String,
    /// Average percent of one core over the covered time.
    pub cpu_avg_pct: f64,
    /// Average percent of the GPU over the time GPU was measured; `null` when it never
    /// was.
    pub gpu_avg_pct: Option<f64>,
    /// Largest footprint at one sample.
    #[specta(type = JsSafeInt)]
    pub mem_peak_bytes: u64,
    pub read_bytes: f64,
    pub write_bytes: f64,
    pub energy_j: f64,
    pub avg_w: f64,
    /// It was in the newest process sample. An exited process cannot be quit.
    pub running: bool,
    /// Why Quit and Force Quit refuse it, as on `LiveProcess`; `null` for an exited
    /// process too, which has nothing to quit.
    pub refusal: Option<SignalRefusal>,
}

/// One app's use over a range: every one of its processes, summed at each sample, with
/// no floor (D-099). Helpers count toward their app by the identity rule per-app network
/// uses (D-089).
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct AppUsage {
    /// "Google Chrome", "Safari", `node`; the process name when no app was resolved.
    pub name: String,
    /// Average percent of one core over the covered time.
    pub cpu_avg_pct: f64,
    /// Average percent of the GPU over `gpu_covered_ms`; `null` when GPU was never
    /// measured.
    pub gpu_avg_pct: Option<f64>,
    /// The largest footprint of its processes summed at one sample.
    #[specta(type = JsSafeInt)]
    pub mem_peak_bytes: u64,
    /// Average footprint over the time it was running.
    #[specta(type = JsSafeInt)]
    pub mem_avg_bytes: u64,
    pub read_bytes: f64,
    pub write_bytes: f64,
    pub energy_j: f64,
    pub avg_w: f64,
    /// The process that Quit on the app's row acts on: its running main executable,
    /// or, for an app without one (a CLI), its only running process. `null` when
    /// neither exists; each running process can still be quit on its own.
    pub quit_pid: Option<i32>,
    /// Ordered like the apps.
    pub processes: Vec<ProcessUsage>,
}

/// Every process's use over the range, before any floor: what shares are of.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct UsageTotal {
    pub cpu_avg_pct: f64,
    pub gpu_avg_pct: Option<f64>,
    pub read_bytes: f64,
    pub write_bytes: f64,
    pub energy_j: f64,
    pub avg_w: f64,
}

/// What the host measured beyond Kelvo's processes ("System and other"): the host's
/// own series over the same range less `UsageByApp.total`. It holds other users' and
/// root's processes and those that lived less than one sample. `null` fields where the
/// series was not recorded (module off, GPU never measured).
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct UsageOther {
    /// Percent of one core: `cpu.total` times the cores, less the apps.
    pub cpu_avg_pct: Option<f64>,
    /// `gpu.util` less the apps.
    pub gpu_avg_pct: Option<f64>,
    /// `disk.read_total` bytes less the apps'.
    pub read_bytes: Option<f64>,
    pub write_bytes: Option<f64>,
    /// The keys whose remainder was clamped to 0: the apps exceeded the host total
    /// there (the two are sampled differently).
    pub clamped: Vec<UsageKey>,
}

/// `query_usage_by_app`: which apps used CPU, GPU, memory, disk and energy over a range,
/// from the last hour of process samples Kelvo keeps in memory (D-093, D-099). Your
/// processes only; `other` holds the rest.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct UsageByApp {
    /// The range the sums cover: the request widened to whole 10 s buckets.
    #[specta(type = JsSafeInt)]
    pub from_ms: i64,
    #[specta(type = JsSafeInt)]
    pub to_ms: i64,
    /// When Kelvo started counting (this launch, or the last history clear); `null`
    /// before the first process sample. After `from_ms`, the range is covered only from
    /// here.
    #[specta(type = Option<JsSafeInt>)]
    pub since_ms: Option<i64>,
    /// Buckets ending at or before this no longer change; a range reaching past it is
    /// still being measured. `null` before the first process sample.
    #[specta(type = Option<JsSafeInt>)]
    pub complete_to_ms: Option<i64>,
    /// Time inside the range a process sample covered, ms. Averages divide by it; 0 means
    /// not measured, not zero use.
    #[specta(type = JsSafeInt)]
    pub covered_ms: i64,
    /// The part of `covered_ms` whose samples measured GPU.
    #[specta(type = JsSafeInt)]
    pub gpu_covered_ms: i64,
    pub total: UsageTotal,
    pub other: UsageOther,
    /// The largest `limit` by the requested key, largest first.
    pub apps: Vec<AppUsage>,
}

/// One metric of a `query_series_stats` answer.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct MetricStat {
    pub metric: String,
    /// Time inside the range with a reading, outside the gaps that apply to the metric.
    #[specta(type = JsSafeInt)]
    pub measured_ms: u64,
    /// Span-weighted average over the measured time; `null` when nothing was measured.
    pub avg: Option<f64>,
    /// The largest sample; `null` when nothing was measured.
    pub max: Option<f64>,
    /// `avg` times the measured seconds: bytes for a bytes-per-second metric.
    pub integral: f64,
}

/// `query_series_stats`: unlabelled metrics over a range, from their rollups through now.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct SeriesStats {
    /// The range the stats cover: the request widened to whole buckets of the tier that
    /// answered and cut at now.
    #[specta(type = JsSafeInt)]
    pub from_ms: i64,
    #[specta(type = JsSafeInt)]
    pub to_ms: i64,
    /// One per metric asked for, in order.
    pub metrics: Vec<MetricStat>,
}

/// The primary interface's addresses (`get_network_addresses`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct NetworkAddresses {
    /// The interface carrying the default route, as `LiveStatus.primary_iface`.
    pub iface: Option<String>,
    /// Dotted quads, in the order the system lists them.
    pub ipv4: Vec<String>,
    /// Link-local addresses are left out.
    pub ipv6: Vec<String>,
}

/// What the history store can tell the UI about itself (D-057, D-059): the low-disk pause
/// and the size-limit trim. Shared by every host, since they share one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct HistoryHealth {
    /// The volume is almost full: 10 s history is not being written ("History paused:
    /// disk almost full"). Minute history and live values continue.
    pub low_disk_paused: bool,
    /// Set once the size limit has trimmed history: it now starts here, ms epoch
    /// ("History trimmed to stay under 150 MB"). Cleared once retention alone would have
    /// dropped that history, and when history is cleared.
    #[specta(type = Option<JsSafeInt>)]
    pub trimmed_before_ms: Option<i64>,
    /// The size limit in effect at that trim, bytes.
    #[specta(type = Option<JsSafeInt>)]
    pub trimmed_limit_bytes: Option<u64>,
    /// False when even the last day does not fit under the limit.
    pub cap_met: bool,
}

impl Default for HistoryHealth {
    fn default() -> Self {
        Self {
            low_disk_paused: false,
            trimmed_before_ms: None,
            trimmed_limit_bytes: None,
            cap_met: true,
        }
    }
}

// --- settings -------------------------------------------------------------------------------

/// Settings with the revision they were current at. Revisions increase by one per change
/// within an app run; a window drops a `settings-changed` event older than what it holds.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct SettingsSnapshot {
    #[specta(type = JsSafeInt)]
    pub revision: u64,
    pub settings: Settings,
}

// --- window appearance ----------------------------------------------------------------------

/// What every window root reflects as attributes: `data-performance`,
/// `data-reduce-transparency` and the theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct WindowAppearance {
    /// Whether Performance mode is in effect, and why (D-088). Any reason but `off`
    /// turns all motion off.
    pub performance: PerformanceReason,
    /// The macOS Reduce Transparency accessibility setting.
    pub reduce_transparency: bool,
    /// The user's Appearance setting. `system` follows `prefers-color-scheme`; Rust also
    /// sets the native window theme, so vibrancy and the CSS media query agree.
    pub theme: Appearance,
}

// --- misc commands --------------------------------------------------------------------------

/// `sensor_dump` (v1-local-monitor.md 4.17). No serial numbers, user names, host names or
/// network identifiers.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct SensorDump {
    pub model: Option<String>,
    pub chip: Option<String>,
    pub os_version: String,
    pub chip_known: bool,
    pub capabilities: Capabilities,
    /// Every Power and Sensors series in the current layout with its latest value
    /// (`null` when stale), in layout order.
    pub sensors: Vec<SensorReading>,
}

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct SensorReading {
    pub key: SeriesKey,
    pub value: Option<f32>,
}

/// `check_for_updates`. The updater lands in phase 6; until then the command reports that
/// no update source is configured, and never makes a network request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateStatus {
    /// "Check for updates automatically" is off; nothing was requested.
    Disabled,
    /// This build has no update source yet.
    NotConfigured,
}

// --- events ---------------------------------------------------------------------------------

/// Emitted after every settings change, with the full new settings.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type, tauri_specta::Event)]
pub struct SettingsChanged {
    #[specta(type = JsSafeInt)]
    pub revision: u64,
    pub settings: Settings,
}

/// Emitted when a host's capabilities change (a module appears or goes away).
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type, tauri_specta::Event)]
pub struct CapabilitiesChanged {
    pub host: HostId,
    pub capabilities: Capabilities,
}

/// Emitted when Performance mode, Reduce Transparency or the theme changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type, tauri_specta::Event)]
pub struct WindowAppearanceChanged {
    pub appearance: WindowAppearance,
}

/// Emitted when [`HistoryHealth`] changes: the low-disk pause starts or ends, or a trim
/// happens or ages out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type, tauri_specta::Event)]
pub struct HistoryHealthChanged {
    pub health: HistoryHealth,
}

/// Emitted when a host's record changes (the local host learns `chip_known` from its
/// first capabilities), with every host, local first: what `list_hosts` returns now.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type, tauri_specta::Event)]
pub struct HostsChanged {
    pub hosts: Vec<HostRecord>,
}

/// Emitted when a detector or alert rule records an event (v1.2). The event is queued
/// to the store with a commit request behind it, not yet committed: `query_events` returns
/// it within one writer round trip, so a query racing this event can miss it. Windows
/// merge it into what they hold instead of fetching again.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type, tauri_specta::Event)]
pub struct EventRecorded {
    pub host: HostId,
    pub event: Event,
}

/// Sent to the `dashboard` window only, when `open_dashboard` asks an existing window to
/// show `route` (for example `/dashboard/settings`). A newly created dashboard loads its
/// route as the URL path instead.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type, tauri_specta::Event)]
pub struct NavigateRequested {
    pub route: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The JSON the channel carries, which the generated `LiveMsg` type must describe: a
    /// flat object with a `kind` tag (D-048).
    #[test]
    fn live_msg_is_internally_tagged_json() {
        let frame = LiveMsg::Frame {
            ts_ms: 1_700_000_000_000,
            layout_no: 3,
            timeline: 0,
            values: vec![None, Some(1.5)],
            held: vec![Some(1.0), Some(1.5)],
        };
        assert_eq!(
            serde_json::to_value(&frame).unwrap(),
            serde_json::json!({
                "kind": "frame",
                "ts_ms": 1_700_000_000_000_i64,
                "layout_no": 3,
                "timeline": 0,
                "values": [null, 1.5],
                "held": [1.0, 1.5],
            })
        );
        let status = LiveMsg::Status(LiveStatus {
            interval_ms: 1000,
            frame_period_ms: 1000,
            paused: false,
            display_idle: false,
            on_battery: true,
            performance: PerformanceReason::LowPowerMode,
            power_source: PowerSource::Battery,
            primary_iface: Some("en0".into()),
        });
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            serde_json::json!({
                "kind": "status",
                "interval_ms": 1000,
                "frame_period_ms": 1000,
                "paused": false,
                "display_idle": false,
                "on_battery": true,
                "performance": "low_power_mode",
                "power_source": "battery",
                "primary_iface": "en0",
            })
        );
        let caps = LiveMsg::Caps {
            capabilities: Capabilities::default(),
        };
        assert_eq!(
            serde_json::to_value(&caps).unwrap()["kind"],
            serde_json::json!("caps")
        );
    }

    #[test]
    fn command_error_is_tagged_by_kind() {
        let e = crate::error::CommandError::history_unavailable();
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({ "kind": "history_unavailable", "reason": null })
        );
    }

    /// The bindings describe what serde sends: a regression guard for D-048 (phased
    /// export turned `LiveMsg` into an externally tagged shape).
    #[test]
    fn generated_bindings_describe_flat_live_msg() {
        let ts = std::fs::read_to_string(crate::BINDINGS_PATH).unwrap();
        assert!(
            ts.contains(r#"({ kind: "frame"; ts_ms: number;"#),
            "LiveMsg frame shape"
        );
        assert!(!ts.contains("_Serialize"), "phased types are off");
    }
}

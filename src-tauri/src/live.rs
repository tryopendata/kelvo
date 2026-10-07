//! Live channels: one per (window label, host), owned and driven by Rust (architecture.md,
//! Window and channel lifecycle; infra 8; D-049, D-066).
//!
//! A window asks for a stream with `subscribe_live`, passing a `Channel<LiveMsg>` and,
//! optionally, the series it draws and how often it wants frames. Rust keeps the stream
//! here and decides when it runs. Hidden WKWebView timers are throttled, so a webview
//! cannot be trusted to notice it is hidden; the window code calls
//! [`LiveRegistry::window_visible`] instead, and the stream stops receiving from the bus
//! until the window is visible again. It then resumes with a backfill of exactly the span
//! it missed. Display sleep and screen lock (`EngineStatus::display_idle`) pause the frames
//! the same way, and so does falling behind the bus. A window the registry has not been
//! told is visible is hidden. The resume sends the missed span in time order, in
//! segments of at most [`EARLIER_CHUNK_ROWS`], one message each: an hour hidden is six
//! messages the webview applies one task at a time, not one long task.
//!
//! A stepped wall clock (D-064) shows as a new `timeline` on the source's frames. A
//! stream that sees one starts its timeline over and sends the new timeline from its
//! first row, so the window drops what it held from that time on.
//!
//! A fresh subscription sends the last two minutes of the host's ring before
//! `subscribe_live` returns and the older history afterwards, in chunks, newest first, so
//! the first paint never waits for an hour of rows. Everything is projected to the
//! window's series before it is serialized.
//!
//! Process interest is a per-window [`ProcessView`] (how many rows, ranked by what, how
//! often), tied to the page load that asked for it through the stream id. It counts only
//! while the window is visible, and the registry tells each host the shortest period any
//! visible window wants, which is how often its collector runs. Which rows a view gets is
//! [`kelvo_engine::select_processes`]; this maps the IPC view to the engine's.
//!
//! Detail interest works the same way without being asked for: a visible window with a
//! live stream for a host counts once toward that engine's detail interest, which samples
//! IOReport every tick instead of every 10 s (D-061).
//!
//! The registry is generic over [`LiveFeed`] (one host's hub and controls) and
//! [`LiveSink`] (the window's channel), so tests drive it without Tauri or an engine.
//! Streams are tokio tasks: call [`LiveRegistry::subscribe`] from inside the runtime
//! (the async `subscribe_live` command does).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use kelvo_engine::process_control::{self, OwnProcesses, ProcessOs, SystemProcessOs};
use kelvo_engine::{
    BackfillSegment, BusMsg, EngineStatus, FrameLayout, LiveFrame, LiveHub, ProcessSample,
    ProcessView as EngineView, Recv, Subscriber,
};
use kelvo_schema::{Capabilities, Catalog, HostId, MetricKind, PerformanceReason, SeriesSelector};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::ipc::{LiveMsg, LiveProcess, LiveStatus, ProcessSort, ProcessView, SubscriptionInfo};
use crate::process_signal::SignalRefusal;

/// One host's live data, as a stream needs it.
pub trait LiveFeed: Send + Sync + 'static {
    fn host(&self) -> HostId;
    /// The host's hub: bus, ring buffer, latest frame and status. Times in it are on the
    /// source's clock, so a remote host's backfill window is measured on its own clock.
    fn hub(&self) -> &LiveHub;
    fn capabilities(&self) -> Capabilities;
    /// The latest status the source published.
    fn status(&self) -> EngineStatus {
        self.hub().status()
    }
    /// The shortest period any visible window wants process rows at (`None`: none does).
    fn set_process_interest(&self, period_ms: Option<u32>);
    /// Whether any visible window with process interest shows network rates (D-081).
    fn set_network_process_interest(&self, interested: bool);
    /// Whether any visible window with process interest shows GPU time.
    fn set_gpu_process_interest(&self, interested: bool);
    fn set_port_process_interest(&self, interested: bool);
    /// Adds (`true`) or removes (`false`) one unit of detail interest.
    fn set_detail_interest(&self, interested: bool);
}

/// Where a stream's messages go: the window's `Channel<LiveMsg>` in the app.
pub trait LiveSink: Send + Sync + 'static {
    /// `false` when the receiver is gone; the stream then ends.
    fn send(&self, msg: LiveMsg) -> bool;
    /// Identifies the page load that subscribed (the channel id).
    fn id(&self) -> u32;
}

impl LiveSink for tauri::ipc::Channel<LiveMsg> {
    fn send(&self, msg: LiveMsg) -> bool {
        tauri::ipc::Channel::send(self, msg).is_ok()
    }
    fn id(&self) -> u32 {
        tauri::ipc::Channel::id(self)
    }
}

/// The default backfill span when the window does not ask for one (the popover's 60 s
/// charts).
pub const DEFAULT_BACKFILL_MS: i64 = 60_000;

/// Upper bound on the backfill a window can ask for: the ring's span.
pub const MAX_BACKFILL_MS: i64 = kelvo_engine::RING_SPAN_MS;

/// History a fresh subscription sends before `subscribe_live` returns; the rest follows
/// as `BackfillEarlier` chunks.
pub const RECENT_MS: i64 = 120_000;

/// Rows per `BackfillEarlier` chunk and per resume `Backfill` message (10 minutes at
/// 1 s).
pub const EARLIER_CHUNK_ROWS: usize = 600;

/// Older history starts after the first frame, or this long after the stream started if
/// no frame comes (paused, or a slow interval).
pub const EARLIER_AFTER: Duration = Duration::from_secs(1);

/// What a window asks of its stream.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveRequest {
    /// How much ring history to send first, ms (clamped to the ring's span).
    pub backfill_ms: i64,
    /// Only these series (`None`: every series in the layout).
    pub series: Option<Vec<SeriesSelector>>,
    /// At most one frame per this many ms (0: every tick). Charts drawn from such a
    /// stream should read `held`: `values` of the frames skipped in between are not sent.
    pub min_period_ms: u32,
}

impl Default for LiveRequest {
    fn default() -> Self {
        Self {
            backfill_ms: DEFAULT_BACKFILL_MS,
            series: None,
            min_period_ms: 0,
        }
    }
}

/// How often a stream paced to `min_period_ms` sends frames at `interval_ms`: the
/// smallest multiple of the interval that [`Stream::on_frame`]'s half-interval tolerance
/// lets through.
pub(crate) fn frame_period_ms(interval_ms: u32, min_period_ms: i64) -> u32 {
    let interval = i64::from(interval_ms.max(1));
    if min_period_ms <= interval {
        return interval_ms;
    }
    let ticks = (min_period_ms - interval / 2 + interval - 1) / interval;
    u32::try_from(ticks.max(1) * interval).unwrap_or(u32::MAX)
}

/// The shortest frame period a visible window gets in Performance mode (D-088).
const PERFORMANCE_MIN_PERIOD_MS: i64 = kelvo_engine::PERFORMANCE_VISIBLE_MS as i64;

/// A stream's frame pacing: what it asked for, raised to 2 s in Performance mode.
pub(crate) fn effective_min_period_ms(requested: i64, performance: PerformanceReason) -> i64 {
    if performance.is_on() {
        requested.max(PERFORMANCE_MIN_PERIOD_MS)
    } else {
        requested
    }
}

fn status_msg(s: &EngineStatus, min_period_ms: i64) -> LiveStatus {
    let min_period_ms = effective_min_period_ms(min_period_ms, s.performance);
    LiveStatus {
        interval_ms: s.interval_ms,
        frame_period_ms: frame_period_ms(s.interval_ms, min_period_ms),
        paused: s.paused,
        display_idle: s.display_idle,
        on_battery: s.on_battery,
        performance: s.performance,
        power_source: s.power_source,
        primary_iface: s.primary_iface.as_deref().map(str::to_owned),
    }
}

/// Which values of a layout a stream sends: `None` is all of them.
type Pick = Option<Arc<[usize]>>;

fn pick_for(layout: &FrameLayout, series: Option<&[SeriesSelector]>) -> Pick {
    let sel = series?;
    Some(
        layout
            .series
            .iter()
            .enumerate()
            .filter(|(_, k)| sel.iter().any(|s| s.matches(k)))
            .map(|(i, _)| i)
            .collect(),
    )
}

fn pick_values(pick: &Pick, v: &[f32]) -> Vec<Option<f32>> {
    let one = |x: f32| x.is_finite().then_some(x);
    match pick {
        None => v.iter().map(|&x| one(x)).collect(),
        Some(idx) => idx
            .iter()
            .map(|&i| v.get(i).copied().and_then(one))
            .collect(),
    }
}

fn pick_holds(pick: &Pick, holds: &[u32]) -> Vec<u32> {
    match pick {
        None => holds.to_vec(),
        Some(idx) => idx
            .iter()
            .map(|&i| holds.get(i).copied().unwrap_or(0))
            .collect(),
    }
}

/// One row for the webview. `me` is this process and `own` Kelvo's own processes, both
/// read once per batch.
fn live_process(r: &ProcessSample, me: i32, own: &OwnProcesses) -> LiveProcess {
    LiveProcess {
        pid: r.pid,
        start_time_us: r.start_time_us,
        name: r.name.to_string(),
        cpu_pct: r.cpu_pct,
        mem_bytes: r.mem_bytes,
        compressed_bytes: r.compressed_bytes,
        threads: r.threads,
        idle_wakeups_per_s: r.idle_wakeups_per_s,
        energy: r.energy,
        disk_read_bps: r.disk_read_bps,
        disk_write_bps: r.disk_write_bps,
        net_rx_bps: r.net_rx_bps,
        net_tx_bps: r.net_tx_bps,
        gpu_pct: r.gpu_pct,
        ports: r.ports.as_deref().map(<[u16]>::to_vec),
        user: r.user.to_string(),
        refusal: SignalRefusal::of(r.pid, &r.name, me, own.contains(r.pid, r.start_time_us)),
    }
}

impl From<ProcessView> for kelvo_engine::ProcessView {
    fn from(v: ProcessView) -> Self {
        Self {
            limit: v.limit,
            sort: v.sort.into_iter().map(Into::into).collect(),
            period_ms: v.period_ms,
            network: v.network,
            gpu: v.gpu,
            ports: v.ports,
        }
    }
}

impl From<ProcessSort> for kelvo_engine::ProcessSort {
    fn from(s: ProcessSort) -> Self {
        match s {
            ProcessSort::Cpu => Self::Cpu,
            ProcessSort::Memory => Self::Memory,
            ProcessSort::Threads => Self::Threads,
            ProcessSort::Wakeups => Self::Wakeups,
            ProcessSort::Energy => Self::Energy,
            ProcessSort::DiskRead => Self::DiskRead,
            ProcessSort::DiskWrite => Self::DiskWrite,
            ProcessSort::DiskTotal => Self::DiskTotal,
            ProcessSort::NetRx => Self::NetRx,
            ProcessSort::NetTx => Self::NetTx,
            ProcessSort::NetTotal => Self::NetTotal,
            ProcessSort::Gpu => Self::Gpu,
        }
    }
}

/// The receiver went away.
struct Closed;

#[derive(Default)]
struct CatchUp {
    first: Option<i64>,
    rows: u32,
    earlier_start: Option<i64>,
    earlier_rows: u32,
}

fn len_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn len_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// Splits `seg` at `at_ms`: rows before it, rows at or after it.
fn split_segment(
    seg: BackfillSegment,
    at_ms: i64,
) -> (Option<BackfillSegment>, Option<BackfillSegment>) {
    let interval = i64::from(seg.interval_ms.max(1));
    let before = usize::try_from((at_ms - seg.start_ms + interval - 1).div_euclid(interval))
        .unwrap_or(0)
        .min(seg.rows.len());
    if before == 0 {
        return (None, Some(seg));
    }
    if before == seg.rows.len() {
        return (Some(seg), None);
    }
    let mut head = seg;
    let tail_rows = head.rows.split_off(before);
    let tail = BackfillSegment {
        layout: Arc::clone(&head.layout),
        start_ms: head.start_ms + len_i64(before) * interval,
        interval_ms: head.interval_ms,
        timeline: head.timeline,
        rows: tail_rows,
        holds: Arc::clone(&head.holds),
    };
    (Some(head), Some(tail))
}

/// `seg` in pieces of at most [`EARLIER_CHUNK_ROWS`] rows, oldest first.
fn chunks(seg: BackfillSegment) -> Vec<BackfillSegment> {
    let mut out = Vec::with_capacity(seg.rows.len() / EARLIER_CHUNK_ROWS + 1);
    let mut rest = Some(seg);
    while let Some(seg) = rest.take() {
        let at = seg.start_ms + len_i64(EARLIER_CHUNK_ROWS) * i64::from(seg.interval_ms.max(1));
        let (head, tail) = split_segment(seg, at);
        out.extend(head);
        rest = tail;
    }
    out
}

/// What one stream has sent, so a resume sends only what is missing.
struct Stream {
    feed: Arc<dyn LiveFeed>,
    sink: Arc<dyn LiveSink>,
    backfill_ms: i64,
    series: Option<Vec<SeriesSelector>>,
    min_period_ms: i64,
    /// This window's process view while its interest is active for this stream.
    procs: Arc<Mutex<Option<EngineView>>>,
    /// The last layout a pick was computed for.
    pick: Option<(Arc<FrameLayout>, Pick)>,
    /// The last layout sent.
    layout_no: Option<u32>,
    /// The frame holds the last `Holds` was built from (the engine's `Arc` and this
    /// stream's frame period), and what it sent.
    holds_from: Option<(Arc<[u32]>, u32)>,
    holds_sent: Option<(u32, Vec<u32>)>,
    /// Timestamp of the last row or frame sent.
    last_ts: Option<i64>,
    /// The source's clock timeline of the last row or frame sent.
    timeline: Option<u32>,
    /// Frames at or before this were already sent as backfill rows. Ring rows carry
    /// nominal times (`start + i * interval`) while frames carry the real tick time, so
    /// the bound is the last row plus half an interval.
    sent_until: i64,
    last_frame_ts: Option<i64>,
    last_procs_ts: Option<i64>,
    caps_revision: Option<u64>,
    status: Option<LiveStatus>,
    /// From the last engine status: Performance mode paces frames to at least 2 s.
    performance: PerformanceReason,
    display_idle: bool,
    /// Older history still to send, newest chunk first.
    earlier: VecDeque<BackfillSegment>,
    /// Older history may go: a frame went out since it was queued, or `earlier_at` passed.
    earlier_go: bool,
    earlier_at: Instant,
}

impl Stream {
    fn new(
        feed: Arc<dyn LiveFeed>,
        sink: Arc<dyn LiveSink>,
        req: LiveRequest,
        procs: Arc<Mutex<Option<EngineView>>>,
    ) -> Self {
        Self {
            feed,
            sink,
            backfill_ms: req.backfill_ms.clamp(0, MAX_BACKFILL_MS),
            series: req.series,
            min_period_ms: i64::from(req.min_period_ms),
            procs,
            pick: None,
            layout_no: None,
            holds_from: None,
            holds_sent: None,
            last_ts: None,
            timeline: None,
            sent_until: i64::MIN,
            last_frame_ts: None,
            last_procs_ts: None,
            caps_revision: None,
            performance: PerformanceReason::Off,
            status: None,
            display_idle: false,
            earlier: VecDeque::new(),
            earlier_go: false,
            earlier_at: Instant::now(),
        }
    }

    fn send(&self, msg: LiveMsg) -> Result<(), Closed> {
        if self.sink.send(msg) {
            Ok(())
        } else {
            Err(Closed)
        }
    }

    fn pick(&mut self, layout: &Arc<FrameLayout>) -> Pick {
        match &self.pick {
            Some((l, p)) if Arc::ptr_eq(l, layout) => p.clone(),
            _ => {
                let p = pick_for(layout, self.series.as_deref());
                self.pick = Some((Arc::clone(layout), p.clone()));
                p
            }
        }
    }

    fn send_layout(&mut self, layout: &Arc<FrameLayout>) -> Result<(), Closed> {
        if self.layout_no != Some(layout.layout_no) {
            let series = match self.pick(layout) {
                None => layout.series.to_vec(),
                Some(idx) => idx
                    .iter()
                    .filter_map(|&i| layout.series.get(i).cloned())
                    .collect(),
            };
            let catalog = Catalog::builtin();
            let kinds = series
                .iter()
                .map(|k| {
                    catalog
                        .get(k.metric.as_str())
                        .map_or(MetricKind::Gauge, |d| d.kind)
                })
                .collect();
            self.send(LiveMsg::Layout {
                layout_no: layout.layout_no,
                series,
                kinds,
            })?;
            self.layout_no = Some(layout.layout_no);
        }
        Ok(())
    }

    fn send_caps(&mut self, caps: Capabilities) -> Result<(), Closed> {
        if self.caps_revision != Some(caps.revision) {
            self.caps_revision = Some(caps.revision);
            self.send(LiveMsg::Caps { capabilities: caps })?;
        }
        Ok(())
    }

    fn send_status(&mut self, s: &EngineStatus) -> Result<(), Closed> {
        self.performance = s.performance;
        let status = status_msg(s, self.min_period_ms);
        if self.status.as_ref() != Some(&status) {
            self.status = Some(status.clone());
            self.send(LiveMsg::Status(status))?;
        }
        Ok(())
    }

    /// Sends one segment as `Backfill` (after its layout) and records it as sent.
    fn send_segment(&mut self, seg: &BackfillSegment) -> Result<(), Closed> {
        let Some(n) = seg.rows.len().checked_sub(1) else {
            return Ok(());
        };
        self.send_layout(&seg.layout)?;
        let pick = self.pick(&seg.layout);
        self.send(LiveMsg::Backfill {
            layout_no: seg.layout.layout_no,
            start_ms: seg.start_ms,
            interval_ms: seg.interval_ms,
            timeline: seg.timeline,
            rows: seg.rows.iter().map(|r| pick_values(&pick, r)).collect(),
            holds_ms: pick_holds(&pick, &seg.holds),
        })?;
        let interval = i64::from(seg.interval_ms);
        let last = seg.start_ms + len_i64(n) * interval;
        self.last_ts = Some(last);
        self.timeline = Some(seg.timeline);
        self.sent_until = last + interval / 2;
        Ok(())
    }

    /// Sends what the window does not have yet: capabilities and status if they changed,
    /// then the ring rows since the last thing sent (at most `backfill_ms` back from the
    /// host's latest frame), in time order and in chunks. On a fresh stream only the last
    /// [`RECENT_MS`] go now; the rest is queued as `BackfillEarlier` chunks.
    ///
    /// If the source's clock stepped since the last thing sent (a new timeline, or "now"
    /// is before it), the stream starts over and sends from the first row not on the
    /// window's timeline, which is where the window has to drop what it holds.
    fn catch_up(&mut self) -> Result<CatchUp, Closed> {
        self.send_caps(self.feed.capabilities())?;
        let status = self.feed.status();
        self.display_idle = status.display_idle;
        self.send_status(&status)?;

        let mut out = CatchUp::default();
        let feed = Arc::clone(&self.feed);
        let hub = feed.hub();
        // "Now" on the source's clock: its latest frame (D-066).
        let Some(latest) = hub.latest_frame() else {
            return Ok(out);
        };
        let anchor = latest.ts_ms;
        let mut since = anchor - self.backfill_ms;
        let mut stepped_from = None;
        if let Some(last) = self.last_ts {
            if anchor < last || self.timeline.is_some_and(|t| t != latest.timeline) {
                stepped_from = Some(self.timeline);
                self.restart_timeline();
            } else {
                since = since.max(last + 1);
            }
        }
        let mut segs = hub.backfill(since);
        if let Some(held) = stepped_from {
            let first = segs
                .iter()
                .position(|s| Some(s.timeline) != held)
                .unwrap_or(0);
            segs.drain(..first);
        }
        if stepped_from.is_some() || self.last_ts.is_some() {
            for seg in segs.into_iter().flat_map(chunks) {
                out.first.get_or_insert(seg.start_ms);
                out.rows = out.rows.saturating_add(len_u32(seg.rows.len()));
                self.send_segment(&seg)?;
            }
            return Ok(out);
        }

        let split = anchor - RECENT_MS;
        let mut recent = Vec::new();
        let mut earlier = Vec::new();
        for seg in segs {
            let (old, new) = split_segment(seg, split);
            earlier.extend(old);
            recent.extend(new);
        }
        // Every layout the older chunks use is announced first, so the last `Layout`
        // before the frames is the current one.
        for seg in &earlier {
            self.send_layout(&seg.layout)?;
        }
        for seg in &recent {
            out.first.get_or_insert(seg.start_ms);
            out.rows = out.rows.saturating_add(len_u32(seg.rows.len()));
            self.send_segment(seg)?;
        }
        out.earlier_start = earlier.first().map(|s| s.start_ms);
        for seg in earlier {
            out.earlier_rows = out.earlier_rows.saturating_add(len_u32(seg.rows.len()));
            self.earlier.extend(chunks(seg));
        }
        // Newest first: each chunk is then older than everything sent before it.
        self.earlier.make_contiguous().reverse();
        if !self.earlier.is_empty() {
            self.earlier_go = false;
            self.earlier_at = Instant::now() + EARLIER_AFTER;
        }
        Ok(out)
    }

    fn send_earlier(&mut self) -> Result<(), Closed> {
        let Some(seg) = self.earlier.pop_front() else {
            return Ok(());
        };
        let pick = self.pick(&seg.layout);
        self.send(LiveMsg::BackfillEarlier {
            layout_no: seg.layout.layout_no,
            start_ms: seg.start_ms,
            interval_ms: seg.interval_ms,
            rows: seg.rows.iter().map(|r| pick_values(&pick, r)).collect(),
            holds_ms: pick_holds(&pick, &seg.holds),
        })
    }

    /// The host's clock stepped: what follows is on another timeline.
    fn restart_timeline(&mut self) {
        tracing::info!(host = %self.feed.host(), "live channel restarts after a clock step");
        self.last_ts = None;
        self.timeline = None;
        self.sent_until = i64::MIN;
        self.last_frame_ts = None;
        self.last_procs_ts = None;
        self.earlier.clear();
        self.holds_from = None;
        self.holds_sent = None;
    }

    fn on_frame(&mut self, f: &LiveFrame) -> Result<(), Closed> {
        if self.display_idle {
            return Ok(());
        }
        if self.timeline.is_some_and(|t| t != f.timeline) {
            // The source's clock stepped (D-064); without this the stream would skip
            // every frame until the clock caught up with what it had sent.
            self.restart_timeline();
        } else if f.ts_ms <= self.sent_until {
            // Already sent as a backfill row (published between the subscribe and the
            // ring read).
            return Ok(());
        }
        let tolerance = i64::from(f.interval_ms) / 2;
        let min_period_ms = effective_min_period_ms(self.min_period_ms, self.performance);
        if min_period_ms > 0
            && self.layout_no == Some(f.layout.layout_no)
            && self
                .last_frame_ts
                .is_some_and(|t| f.ts_ms - t + tolerance < min_period_ms)
        {
            return Ok(());
        }
        self.send_layout(&f.layout)?;
        let pick = self.pick(&f.layout);
        self.send_holds(f, &pick)?;
        self.send(LiveMsg::Frame {
            ts_ms: f.ts_ms,
            layout_no: f.layout.layout_no,
            timeline: f.timeline,
            values: pick_values(&pick, &f.values),
            held: pick_values(&pick, &f.held),
        })?;
        self.last_ts = Some(f.ts_ms);
        self.timeline = Some(f.timeline);
        self.sent_until = f.ts_ms;
        self.last_frame_ts = Some(f.ts_ms);
        self.earlier_go = true;
        Ok(())
    }

    /// Sends `Holds` before `f` when its holds differ from the last ones sent. A thinned
    /// stream's frames are a frame period apart, so no hold is shorter than that period's.
    fn send_holds(&mut self, f: &LiveFrame, pick: &Pick) -> Result<(), Closed> {
        let frame_period = self
            .status
            .as_ref()
            .map_or(f.interval_ms, |s| s.frame_period_ms);
        if self
            .holds_from
            .as_ref()
            .is_some_and(|(h, p)| Arc::ptr_eq(h, &f.holds) && *p == frame_period)
            && self.holds_sent.as_ref().map(|(no, _)| *no) == Some(f.layout.layout_no)
        {
            return Ok(());
        }
        self.holds_from = Some((Arc::clone(&f.holds), frame_period));
        let floor = kelvo_engine::hold_ms(frame_period);
        let holds: Vec<u32> = pick_holds(pick, &f.holds)
            .into_iter()
            .map(|h| h.max(floor))
            .collect();
        if self
            .holds_sent
            .as_ref()
            .is_some_and(|(no, sent)| *no == f.layout.layout_no && *sent == holds)
        {
            return Ok(());
        }
        self.send(LiveMsg::Holds {
            layout_no: f.layout.layout_no,
            holds_ms: holds.clone(),
        })?;
        self.holds_sent = Some((f.layout.layout_no, holds));
        Ok(())
    }

    fn on_processes(&mut self, ts_ms: i64, rows: &[ProcessSample]) -> Result<(), Closed> {
        if self.display_idle {
            return Ok(());
        }
        let picked = {
            let view = self.procs.lock().unwrap_or_else(|e| e.into_inner());
            let Some(view) = view.as_ref() else {
                return Ok(());
            };
            if let (Some(period), Some(last)) = (view.period_ms, self.last_procs_ts) {
                // A tenth of slack so jittered ticks do not skip a whole period.
                if (ts_ms - last) * 10 < i64::from(period) * 9 {
                    return Ok(());
                }
            }
            let me = SystemProcessOs.self_pid();
            let own = process_control::own_processes();
            kelvo_engine::select_processes(rows, view, |r| live_process(r, me, &own))
        };
        self.last_procs_ts = Some(ts_ms);
        self.send(LiveMsg::Processes {
            ts_ms,
            rows: picked,
        })
    }

    fn on_msg(&mut self, msg: BusMsg) -> Result<(), Closed> {
        match msg {
            BusMsg::Frame(f) => self.on_frame(&f)?,
            // Frames carry their layout; it is sent with the first frame that uses it.
            BusMsg::Layout(_) => {}
            BusMsg::Caps(c) => self.send_caps((*c).clone())?,
            BusMsg::Status(s) => {
                let was_idle = self.display_idle;
                self.display_idle = s.display_idle;
                self.send_status(&s)?;
                if was_idle && !s.display_idle {
                    self.catch_up()?;
                }
            }
            BusMsg::Processes(p) => self.on_processes(p.ts_ms, &p.rows)?,
            // Events reach windows as the `event-recorded` Tauri event (state.rs).
            BusMsg::Event(_) => {}
        }
        Ok(())
    }

    /// Runs until the receiver is gone, the window closes, or the bus closes. While the
    /// window is hidden it holds no bus subscriber, so it costs nothing per tick.
    async fn run(mut self, mut visible: watch::Receiver<bool>, mut sub: Option<Subscriber>) {
        loop {
            let Some(s) = sub.as_mut() else {
                if visible.wait_for(|v| *v).await.is_err() {
                    return;
                }
                let s = self.feed.hub().subscribe();
                if self.catch_up().is_err() {
                    return;
                }
                sub = Some(s);
                continue;
            };
            let pending = !self.earlier.is_empty() && !self.display_idle;
            let go = self.earlier_go;
            let earlier_at = self.earlier_at;
            tokio::select! {
                // The bus first: a frame is never queued behind old history.
                biased;
                ev = s.recv_event() => match ev {
                    // Hidden since this was published (the bus goes first), or hidden
                    // and shown again before this task ran: stop here. The resume's
                    // catch-up sends what was missed with the status in effect then,
                    // not the background tick hiding put in (D-094).
                    Recv::Msg(_) if !*visible.borrow() || visible.has_changed().unwrap_or(true) => {
                        tracing::debug!(host = %self.feed.host(), "live channel stopped");
                        sub = None;
                    }
                    Recv::Msg(m) => {
                        if self.on_msg(m).is_err() {
                            return;
                        }
                    }
                    Recv::Lagged(n) => {
                        // The hub's ring has what the bus dropped.
                        tracing::debug!(host = %self.feed.host(), lost = n, "live channel lagged");
                        if self.catch_up().is_err() {
                            return;
                        }
                    }
                    Recv::Closed => return,
                },
                changed = visible.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    if !*visible.borrow_and_update() {
                        tracing::debug!(host = %self.feed.host(), "live channel stopped");
                        sub = None;
                    }
                }
                () = tokio::time::sleep_until(earlier_at), if pending && !go => {
                    self.earlier_go = true;
                }
                () = std::future::ready(()), if pending && go => {
                    if self.send_earlier().is_err() {
                        return;
                    }
                }
            }
        }
    }
}

struct StreamSlot {
    id: u32,
    task: JoinHandle<()>,
    procs: Arc<Mutex<Option<EngineView>>>,
}

struct ProcInterest {
    view: EngineView,
    /// The stream (page load) it belongs to; `None` from a caller that does not say,
    /// which then counts whatever stream the window has.
    token: Option<u32>,
}

/// One window's state for one host.
struct HostSlot {
    feed: Arc<dyn LiveFeed>,
    stream: Option<StreamSlot>,
    /// The sequence number of the newest `subscribe` for this slot. Only that one
    /// installs its stream; an older one still setting up when a newer one started
    /// aborts its own.
    latest: u64,
    procs: Option<ProcInterest>,
    /// Counted in the engine's detail interest.
    detail_applied: bool,
}

impl HostSlot {
    fn new(feed: Arc<dyn LiveFeed>) -> Self {
        Self {
            feed,
            stream: None,
            latest: 0,
            procs: None,
            detail_applied: false,
        }
    }

    /// The process view in effect: interest whose token matches the current stream.
    fn active_procs(&self) -> Option<&EngineView> {
        let p = self.procs.as_ref()?;
        match (p.token, &self.stream) {
            (None, _) => Some(&p.view),
            (Some(t), Some(s)) if s.id == t => Some(&p.view),
            _ => None,
        }
    }

    /// Hands the stream its process view and applies detail interest for `visible`.
    fn sync(&mut self, visible: bool) {
        if let Some(s) = &self.stream {
            *s.procs.lock().unwrap_or_else(|e| e.into_inner()) = self.active_procs().cloned();
        }
        let detail = visible && self.stream.is_some();
        if detail != self.detail_applied {
            self.feed.set_detail_interest(detail);
            self.detail_applied = detail;
        }
    }

    fn release(self) {
        if let Some(s) = self.stream {
            s.task.abort();
        }
        if self.detail_applied {
            self.feed.set_detail_interest(false);
        }
    }
}

struct WindowEntry {
    visible: watch::Sender<bool>,
    hosts: HashMap<HostId, HostSlot>,
}

impl WindowEntry {
    fn new() -> Self {
        // Hidden until the window code says otherwise (D-066): a window it forgot to
        // report must not stream unseen.
        Self {
            visible: watch::Sender::new(false),
            hosts: HashMap::new(),
        }
    }

    fn is_visible(&self) -> bool {
        *self.visible.borrow()
    }
}

#[derive(Default)]
struct Inner {
    windows: HashMap<String, WindowEntry>,
    /// The process period and network, GPU and ports flags last given to each host.
    applied: HashMap<HostId, (Option<u32>, bool, bool, bool)>,
    /// Numbers `subscribe` calls, registry-wide, so a slot recreated after its window
    /// closed never matches a call that started before.
    next_seq: u64,
}

impl Inner {
    fn window(&mut self, label: &str) -> &mut WindowEntry {
        self.windows
            .entry(label.to_owned())
            .or_insert_with(WindowEntry::new)
    }

    /// Tells `feed`'s host the shortest process period any visible window wants, and
    /// whether any of those windows shows network rates or GPU time.
    fn apply_process_interest(&mut self, feed: &Arc<dyn LiveFeed>) {
        let host = feed.host();
        let mut period = None;
        let mut network = false;
        let mut gpu = false;
        let mut ports = false;
        for v in self
            .windows
            .values()
            .filter(|w| w.is_visible())
            .filter_map(|w| w.hosts.get(&host)?.active_procs())
        {
            let p = v.period_ms.unwrap_or(0);
            period = Some(period.map_or(p, |q: u32| q.min(p)));
            network |= v.network;
            gpu |= v.gpu;
            ports |= v.ports;
        }
        let applied = self
            .applied
            .entry(host)
            .or_insert((None, false, false, false));
        if applied.0 != period {
            applied.0 = period;
            feed.set_process_interest(period);
        }
        if applied.1 != network {
            applied.1 = network;
            feed.set_network_process_interest(network);
        }
        if applied.2 != gpu {
            applied.2 = gpu;
            feed.set_gpu_process_interest(gpu);
        }
        if applied.3 != ports {
            applied.3 = ports;
            feed.set_port_process_interest(ports);
        }
    }
}

/// Every live channel and process interest, by window label.
#[derive(Default)]
pub struct LiveRegistry {
    inner: Mutex<Inner>,
}

impl LiveRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Starts (or restarts, after a page reload) the stream for `label` and the feed's
    /// host. If the window is visible, `Caps`, `Status`, the layouts and the recent
    /// backfill are sent before this returns; frames and older history follow from a
    /// task. Process interest tagged with another stream (the page before a reload) ends
    /// here. Must run inside the tokio runtime.
    pub fn subscribe(
        &self,
        label: &str,
        feed: Arc<dyn LiveFeed>,
        sink: Arc<dyn LiveSink>,
        req: LiveRequest,
    ) -> SubscriptionInfo {
        let host = feed.host();
        let id = sink.id();
        let procs = Arc::new(Mutex::new(None));
        let (seq, mut visible) = {
            let mut inner = self.lock();
            inner.next_seq += 1;
            let seq = inner.next_seq;
            let w = inner.window(label);
            let vis = w.is_visible();
            let slot = w
                .hosts
                .entry(host)
                .or_insert_with(|| HostSlot::new(Arc::clone(&feed)));
            slot.latest = seq;
            // A visible window's first status must carry the visible tick, not the
            // background's (D-094): its detail interest goes in before the catch-up.
            if vis && !slot.detail_applied {
                slot.feed.set_detail_interest(true);
                slot.detail_applied = true;
            }
            if let Some(old) = slot.stream.take() {
                old.task.abort();
            }
            if slot
                .procs
                .as_ref()
                .is_some_and(|p| p.token.is_some_and(|t| t != id))
            {
                slot.procs = None;
            }
            (seq, w.visible.subscribe())
        };
        let mut stream = Stream::new(Arc::clone(&feed), sink, req, Arc::clone(&procs));
        let mut info = SubscriptionInfo {
            host,
            stream: id,
            backfill_start_ms: None,
            backfill_rows: 0,
            earlier_start_ms: None,
            earlier_rows: 0,
        };
        let mut sub = None;
        if *visible.borrow_and_update() {
            // Subscribe before reading the ring, so nothing published in between is lost;
            // frames the backfill already covered are skipped by timestamp.
            let s = feed.hub().subscribe();
            match stream.catch_up() {
                Ok(c) => {
                    info.backfill_start_ms = c.first;
                    info.backfill_rows = c.rows;
                    info.earlier_start_ms = c.earlier_start;
                    info.earlier_rows = c.earlier_rows;
                }
                Err(Closed) => return info,
            }
            sub = Some(s);
        }
        let task = tokio::spawn(stream.run(visible, sub));
        let mut inner = self.lock();
        let current = inner.windows.get_mut(label).and_then(|w| {
            let vis = w.is_visible();
            w.hosts
                .get_mut(&host)
                .filter(|slot| slot.latest == seq)
                .map(|slot| (slot, vis))
        });
        match current {
            Some((slot, vis)) => {
                if let Some(old) = slot.stream.replace(StreamSlot { id, task, procs }) {
                    old.task.abort();
                }
                slot.sync(vis);
            }
            // A newer subscribe for this window and host started while this one was
            // setting up (React StrictMode subscribes twice), or the window closed.
            None => {
                task.abort();
                tracing::debug!(%host, label, stream = id, "live channel superseded");
                return info;
            }
        }
        inner.apply_process_interest(&feed);
        tracing::debug!(%host, label, stream = id, "live channel subscribed");
        info
    }

    /// Sets (`Some`) or withdraws (`None`) the process rows `label` wants for the feed's
    /// host. `token` is the stream id from `subscribe_live`: the interest counts only
    /// while that stream is the window's current one, so a reloaded page's interest ends
    /// with it. Without a token it counts for whatever stream the window has.
    pub fn set_process_interest(
        &self,
        label: &str,
        feed: Arc<dyn LiveFeed>,
        view: Option<ProcessView>,
        token: Option<u32>,
    ) {
        let host = feed.host();
        let mut inner = self.lock();
        let w = inner.window(label);
        let vis = w.is_visible();
        let slot = w
            .hosts
            .entry(host)
            .or_insert_with(|| HostSlot::new(Arc::clone(&feed)));
        slot.procs = view.map(|view| ProcInterest {
            view: view.into(),
            token,
        });
        slot.sync(vis);
        inner.apply_process_interest(&feed);
    }

    /// The window was shown (`true`) or hidden, minimized or occluded (`false`). Streams
    /// stop receiving while hidden and resume with a backfill of the missed span; process
    /// and detail interest leave the hosts' counts while hidden and return on show.
    ///
    /// On show, detail interest goes in before the streams resume, and the engine has
    /// published the visible tick by then (D-094), so a resumed stream's status never
    /// carries the background's 2 s. On hide the streams stop first.
    pub fn window_visible(&self, label: &str, visible: bool) {
        let mut inner = self.lock();
        let w = inner.window(label);
        let send = |w: &WindowEntry| {
            w.visible.send_if_modified(|v| {
                let changed = *v != visible;
                *v = visible;
                changed
            });
        };
        if !visible {
            send(w);
        }
        let mut feeds = Vec::with_capacity(w.hosts.len());
        for slot in w.hosts.values_mut() {
            slot.sync(visible);
            feeds.push(Arc::clone(&slot.feed));
        }
        if visible {
            send(w);
        }
        for f in &feeds {
            inner.apply_process_interest(f);
        }
        tracing::debug!(label, visible, "window visibility");
    }

    /// The window is gone: drop its streams and its interest.
    pub fn window_closed(&self, label: &str) {
        let mut inner = self.lock();
        let Some(w) = inner.windows.remove(label) else {
            return;
        };
        let mut feeds = Vec::with_capacity(w.hosts.len());
        for (_, slot) in w.hosts {
            feeds.push(Arc::clone(&slot.feed));
            slot.release();
        }
        for f in &feeds {
            inner.apply_process_interest(f);
        }
        tracing::debug!(label, "window closed; live channels dropped");
    }

    /// Whether `label` currently counts as visible. Unknown labels do not.
    pub fn is_visible(&self, label: &str) -> bool {
        self.lock()
            .windows
            .get(label)
            .is_some_and(WindowEntry::is_visible)
    }
}

#[cfg(test)]
mod tests;

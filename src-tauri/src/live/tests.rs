//! Live registry tests. They run on a current-thread runtime with paused time: tokio only
//! advances a paused clock when every task is blocked, so `settle()` (a 1 ms sleep)
//! returns once each stream has handled everything it can. No wall-clock waits.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use kelvo_engine::{Bus, FrameLayout, LiveFrame, ProcessBatch};
use kelvo_schema::{Labels, MetricId, SeriesKey};
use uuid::Uuid;

use super::*;

/// A host whose hub, status and capabilities the test drives, the way a source would.
struct FakeFeed {
    host: HostId,
    hub: LiveHub,
    caps: Mutex<Capabilities>,
    /// Every process period the registry set, in order.
    periods: Mutex<Vec<Option<u32>>>,
    /// Every network flag the registry set, in order.
    network: Mutex<Vec<bool>>,
    gpu: Mutex<Vec<bool>>,
    detail: AtomicI64,
    /// Behave like the engine (D-094): detail interest crossing between zero and one
    /// publishes a status with the visible 1 s or background 2 s tick before returning.
    background_tick: AtomicBool,
    /// The clock timeline the next frames carry; `step` bumps it.
    timeline: AtomicU32,
}

impl FakeFeed {
    fn new() -> Arc<Self> {
        Self::with_bus(Bus::default())
    }

    fn with_bus(bus: Bus) -> Arc<Self> {
        let feed = Arc::new(Self {
            host: HostId(Uuid::new_v4()),
            hub: LiveHub::new(bus),
            caps: Mutex::new(Capabilities {
                revision: 1,
                ..Capabilities::default()
            }),
            periods: Mutex::new(Vec::new()),
            network: Mutex::new(Vec::new()),
            gpu: Mutex::new(Vec::new()),
            detail: AtomicI64::new(0),
            background_tick: AtomicBool::new(false),
            timeline: AtomicU32::new(0),
        });
        feed.set_status(|s| s.interval_ms = 1000);
        feed
    }

    /// One tick at `ts_ms` with value `v` in every series.
    fn tick(&self, layout: &Arc<FrameLayout>, ts_ms: i64, v: f32) {
        let vals: Arc<[f32]> = vec![v; layout.series.len()].into();
        self.frame(layout, ts_ms, vals.clone(), vals);
    }

    fn frame(&self, layout: &Arc<FrameLayout>, ts_ms: i64, values: Arc<[f32]>, held: Arc<[f32]>) {
        let holds: Arc<[u32]> = vec![2_500; layout.series.len()].into();
        self.frame_with_holds(layout, ts_ms, values, held, holds);
    }

    fn frame_with_holds(
        &self,
        layout: &Arc<FrameLayout>,
        ts_ms: i64,
        values: Arc<[f32]>,
        held: Arc<[f32]>,
        holds: Arc<[u32]>,
    ) {
        self.hub.publish(BusMsg::Frame(Arc::new(LiveFrame {
            ts_ms,
            interval_ms: 1000,
            layout: Arc::clone(layout),
            timeline: self.timeline.load(Ordering::SeqCst),
            values,
            held,
            holds,
        })));
    }

    /// The source's wall clock was stepped: later frames are on a new timeline.
    fn step(&self) {
        self.timeline.fetch_add(1, Ordering::SeqCst);
    }

    fn set_status(&self, f: impl FnOnce(&mut EngineStatus)) {
        let mut s = self.hub.status();
        f(&mut s);
        self.hub.publish(BusMsg::Status(s));
    }

    fn period(&self) -> Option<u32> {
        self.periods.lock().unwrap().last().copied().flatten()
    }

    fn processes(&self, ts_ms: i64, rows: Vec<ProcessSample>) {
        self.hub
            .publish(BusMsg::Processes(Arc::new(ProcessBatch { ts_ms, rows })));
    }
}

impl LiveFeed for FakeFeed {
    fn host(&self) -> HostId {
        self.host
    }
    fn hub(&self) -> &LiveHub {
        &self.hub
    }
    fn capabilities(&self) -> Capabilities {
        self.caps.lock().unwrap().clone()
    }
    fn set_process_interest(&self, period_ms: Option<u32>) {
        self.periods.lock().unwrap().push(period_ms);
    }
    fn set_network_process_interest(&self, interested: bool) {
        self.network.lock().unwrap().push(interested);
    }
    fn set_gpu_process_interest(&self, interested: bool) {
        self.gpu.lock().unwrap().push(interested);
    }
    fn set_detail_interest(&self, interested: bool) {
        let before = self
            .detail
            .fetch_add(if interested { 1 } else { -1 }, Ordering::SeqCst);
        if self.background_tick.load(Ordering::SeqCst) && (before == 0) == interested {
            self.set_status(|s| s.interval_ms = if interested { 1000 } else { 2000 });
        }
    }
}

static NEXT_SINK: AtomicU32 = AtomicU32::new(1);

struct TestSink {
    tx: Mutex<mpsc::Sender<LiveMsg>>,
    id: u32,
}

impl LiveSink for TestSink {
    fn send(&self, msg: LiveMsg) -> bool {
        self.tx.lock().unwrap().send(msg).is_ok()
    }
    fn id(&self) -> u32 {
        self.id
    }
}

fn sink() -> (Arc<dyn LiveSink>, mpsc::Receiver<LiveMsg>, u32) {
    let (tx, rx) = mpsc::channel();
    let id = NEXT_SINK.fetch_add(1, Ordering::SeqCst);
    (
        Arc::new(TestSink {
            tx: Mutex::new(tx),
            id,
        }),
        rx,
        id,
    )
}

fn layout(no: u32, n: usize) -> Arc<FrameLayout> {
    let ids = ["cpu.total", "gpu.util", "mem.used"];
    Arc::new(FrameLayout {
        layout_no: no,
        series: ids[..n]
            .iter()
            .map(|id| SeriesKey::bare(MetricId::from_static(id)))
            .collect(),
    })
}

fn req(backfill_ms: i64) -> LiveRequest {
    LiveRequest {
        backfill_ms,
        ..LiveRequest::default()
    }
}

/// A registry with these windows reported visible.
fn registry(visible: &[&str]) -> LiveRegistry {
    let reg = LiveRegistry::new();
    for l in visible {
        reg.window_visible(l, true);
    }
    reg
}

/// A compact, comparable description of each message.
fn tag(m: &LiveMsg) -> String {
    // Timeline 0 is left out, so only tests that step the clock show it.
    let tl = |t: u32| {
        if t == 0 {
            String::new()
        } else {
            format!(" t{t}")
        }
    };
    match m {
        LiveMsg::Layout {
            layout_no, series, ..
        } => format!("layout {layout_no}/{}", series.len()),
        LiveMsg::Holds {
            layout_no,
            holds_ms,
        } => format!("holds {layout_no} {holds_ms:?}"),
        LiveMsg::Backfill {
            layout_no,
            start_ms,
            timeline,
            rows,
            ..
        } => format!(
            "backfill {layout_no} @{start_ms} x{}{}",
            rows.len(),
            tl(*timeline)
        ),
        LiveMsg::BackfillEarlier {
            layout_no,
            start_ms,
            rows,
            ..
        } => format!("earlier {layout_no} @{start_ms} x{}", rows.len()),
        LiveMsg::Frame {
            ts_ms,
            layout_no,
            timeline,
            ..
        } => format!("frame {layout_no} @{ts_ms}{}", tl(*timeline)),
        LiveMsg::Processes { rows, .. } => format!("processes x{}", rows.len()),
        LiveMsg::Caps { capabilities } => format!("caps r{}", capabilities.revision),
        LiveMsg::Status(s) => format!(
            "status {} paused={}{}",
            s.interval_ms,
            s.paused,
            if s.display_idle { " idle" } else { "" }
        ),
    }
}

/// Lets every task run until it blocks.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(1)).await;
}

/// Everything sent so far, once the streams are idle, without `Holds` (the tests below
/// that are about holds read them with [`drain_all`]).
async fn drain(rx: &mpsc::Receiver<LiveMsg>) -> Vec<String> {
    settle().await;
    rx.try_iter()
        .filter(|m| !matches!(m, LiveMsg::Holds { .. }))
        .map(|m| tag(&m))
        .collect()
}

/// Everything sent so far, as messages.
async fn drain_raw(rx: &mpsc::Receiver<LiveMsg>) -> Vec<LiveMsg> {
    settle().await;
    rx.try_iter().collect()
}

/// Everything sent so far, `Holds` included.
async fn drain_all(rx: &mpsc::Receiver<LiveMsg>) -> Vec<String> {
    settle().await;
    rx.try_iter().map(|m| tag(&m)).collect()
}

const HEAD: [&str; 2] = ["caps r1", "status 1000 paused=false"];

fn with_head(rest: &[&str]) -> Vec<String> {
    HEAD.iter().chain(rest).map(|s| s.to_string()).collect()
}

#[tokio::test(start_paused = true)]
async fn subscribe_sends_layout_then_backfill_then_frames() {
    let feed = FakeFeed::new();
    let l1 = layout(1, 2);
    let l2 = layout(2, 3);
    for i in 0..5 {
        feed.tick(&l1, 10_000 + i * 1000 + 40, i as f32);
    }
    let (s, rx, _) = sink();
    let reg = registry(&["popover"]);
    let info = reg.subscribe("popover", feed.clone(), s, req(3_000));
    // The ring holds 10,040..14,040; 3 s back from the latest frame gives four rows.
    assert_eq!(info.backfill_start_ms, Some(11_040));
    assert_eq!(info.backfill_rows, 4);
    assert_eq!(info.earlier_rows, 0);

    // A frame the backfill already covered is skipped; later frames flow, with a Layout
    // before the first frame of a new layout.
    feed.frame(&l1, 14_040, vec![0.0, 0.0].into(), vec![0.0, 0.0].into());
    feed.tick(&l1, 15_010, 5.0);
    feed.tick(&l2, 16_030, 6.0);
    feed.tick(&l2, 17_000, 7.0);
    assert_eq!(
        drain(&rx).await,
        with_head(&[
            "layout 1/2",
            "backfill 1 @11040 x4",
            "frame 1 @15010",
            "layout 2/3",
            "frame 2 @16030",
            "frame 2 @17000",
        ])
    );
}

#[tokio::test(start_paused = true)]
async fn frames_carry_raw_values_and_held_values_with_nan_as_null() {
    let feed = FakeFeed::new();
    let l = layout(1, 2);
    let (s, rx, _) = sink();
    let reg = registry(&["dashboard"]);
    reg.subscribe("dashboard", feed.clone(), s, req(60_000));
    feed.frame(&l, 1_000, vec![f32::NAN, 2.0].into(), vec![1.0, 2.0].into());
    settle().await;
    let frame = rx
        .try_iter()
        .find(|m| matches!(m, LiveMsg::Frame { .. }))
        .unwrap();
    assert_eq!(
        frame,
        LiveMsg::Frame {
            ts_ms: 1_000,
            layout_no: 1,
            timeline: 0,
            values: vec![None, Some(2.0)],
            held: vec![Some(1.0), Some(2.0)],
        }
    );
}

#[tokio::test(start_paused = true)]
async fn hidden_window_stops_and_resumes_with_only_the_missed_span() {
    let feed = FakeFeed::new();
    let l = layout(1, 1);
    feed.tick(&l, 1_000, 0.0);
    let reg = registry(&["dashboard"]);
    let (s, rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s, req(60_000));
    feed.tick(&l, 2_000, 1.0);
    assert_eq!(
        drain(&rx).await,
        with_head(&["layout 1/1", "backfill 1 @1000 x1", "frame 1 @2000"])
    );

    reg.window_visible("dashboard", false);
    settle().await;
    assert_eq!(
        feed.hub.bus().subscriber_count(),
        0,
        "a hidden stream holds no subscriber"
    );
    for t in 3..=6 {
        feed.tick(&l, t * 1000, t as f32);
    }
    assert!(drain(&rx).await.is_empty(), "nothing is sent while hidden");

    reg.window_visible("dashboard", true);
    settle().await;
    feed.tick(&l, 7_000, 7.0);
    // Caps, status and layout are unchanged, so only the missed rows and new frames.
    assert_eq!(
        drain(&rx).await,
        vec!["backfill 1 @3000 x4", "frame 1 @7000"]
    );
}

#[tokio::test(start_paused = true)]
async fn subscribing_while_hidden_sends_nothing_until_shown() {
    let feed = FakeFeed::new();
    let l = layout(1, 1);
    feed.tick(&l, 1_000, 0.0);
    let reg = LiveRegistry::new();
    reg.window_visible("popover", false);
    let (s, rx, _) = sink();
    let info = reg.subscribe("popover", feed.clone(), s, req(60_000));
    assert_eq!(info.backfill_start_ms, None);
    feed.tick(&l, 2_000, 1.0);
    assert!(drain(&rx).await.is_empty());

    reg.window_visible("popover", true);
    assert_eq!(
        drain(&rx).await,
        with_head(&["layout 1/1", "backfill 1 @1000 x2"])
    );
}

/// #26: a label the window code never reported is hidden, not streaming unseen.
#[tokio::test(start_paused = true)]
async fn an_unreported_window_counts_as_hidden() {
    let feed = FakeFeed::new();
    let l = layout(1, 1);
    feed.tick(&l, 1_000, 0.0);
    let reg = LiveRegistry::new();
    assert!(!reg.is_visible("board-1"));
    let (s, rx, _) = sink();
    let info = reg.subscribe("board-1", feed.clone(), s, req(60_000));
    feed.tick(&l, 2_000, 1.0);
    assert_eq!(info.backfill_rows, 0);
    assert!(drain(&rx).await.is_empty());
    assert_eq!(feed.detail.load(Ordering::SeqCst), 0, "no detail interest");
    reg.window_visible("board-1", true);
    assert!(reg.is_visible("board-1"));
    assert_eq!(
        drain(&rx).await,
        with_head(&["layout 1/1", "backfill 1 @1000 x2"])
    );
}

#[tokio::test(start_paused = true)]
async fn display_sleep_pauses_frames_and_wake_backfills_the_gap() {
    let feed = FakeFeed::new();
    let l = layout(1, 1);
    let reg = registry(&["dashboard"]);
    let (s, rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s, req(60_000));
    feed.tick(&l, 1_000, 1.0);
    feed.set_status(|s| s.display_idle = true);
    feed.tick(&l, 2_000, 2.0);
    feed.tick(&l, 3_000, 3.0);
    feed.set_status(|s| s.display_idle = false);
    // The stream handles the wake before the next tick lands in the ring.
    settle().await;
    feed.tick(&l, 4_000, 4.0);
    assert_eq!(
        drain(&rx).await,
        with_head(&[
            "layout 1/1",
            "frame 1 @1000",
            "status 1000 paused=false idle",
            "status 1000 paused=false",
            "backfill 1 @2000 x2",
            "frame 1 @4000",
        ])
    );
}

#[tokio::test(start_paused = true)]
async fn status_and_caps_are_sent_on_change_only() {
    let feed = FakeFeed::new();
    let reg = registry(&["dashboard"]);
    let (s, rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s, req(60_000));
    feed.set_status(|_| {});
    feed.set_status(|s| s.interval_ms = 2000);
    for revision in [1, 2] {
        feed.hub.publish(BusMsg::Caps(Arc::new(Capabilities {
            revision,
            ..Capabilities::default()
        })));
    }
    assert_eq!(
        drain(&rx).await,
        with_head(&["status 2000 paused=false", "caps r2"])
    );
}

fn view(limit: Option<u16>, sort: &[ProcessSort], period_ms: Option<u32>) -> ProcessView {
    ProcessView {
        limit,
        sort: sort.to_vec(),
        period_ms,
        network: false,
        gpu: false,
    }
}

#[tokio::test(start_paused = true)]
async fn process_interest_is_the_shortest_period_of_the_visible_windows() {
    let feed = FakeFeed::new();
    let reg = registry(&["dashboard", "popover"]);
    let f = || -> Arc<dyn LiveFeed> { feed.clone() };

    reg.set_process_interest(
        "dashboard",
        f(),
        Some(view(Some(5), &[], Some(5_000))),
        None,
    );
    assert_eq!(feed.period(), Some(5_000));
    reg.set_process_interest("popover", f(), Some(view(None, &[], None)), None);
    assert_eq!(feed.period(), Some(0), "the full table wants every sample");

    reg.window_visible("popover", false);
    assert_eq!(feed.period(), Some(5_000), "a hidden window does not count");
    reg.window_visible("popover", true);
    assert_eq!(feed.period(), Some(0), "and counts again when shown");

    reg.set_process_interest("popover", f(), None, None);
    assert_eq!(feed.period(), Some(5_000));
    reg.window_visible("dashboard", false);
    assert_eq!(feed.period(), None);
    reg.window_visible("dashboard", true);
    reg.window_closed("dashboard");
    assert_eq!(feed.period(), None, "closing drops it");
    assert_eq!(
        *feed.periods.lock().unwrap(),
        vec![
            Some(5_000),
            Some(0),
            Some(5_000),
            Some(0),
            Some(5_000),
            None,
            Some(5_000),
            None
        ],
        "the host hears only changes"
    );
}

/// Network rates are wanted while any visible window's view asks for them, and only
/// changes reach the host (D-081).
#[tokio::test(start_paused = true)]
async fn network_interest_follows_the_visible_views_that_ask_for_it() {
    let feed = FakeFeed::new();
    let reg = registry(&["dashboard", "popover"]);
    let f = || -> Arc<dyn LiveFeed> { feed.clone() };
    let net = |limit, period| ProcessView {
        network: true,
        ..view(limit, &[ProcessSort::NetTotal], period)
    };

    reg.set_process_interest("popover", f(), Some(view(Some(5), &[], None)), None);
    reg.set_process_interest("dashboard", f(), Some(net(Some(5), Some(5_000))), None);
    reg.window_visible("dashboard", false);
    reg.window_visible("dashboard", true);
    reg.set_process_interest("dashboard", f(), Some(view(None, &[], None)), None);
    reg.set_process_interest("popover", f(), Some(net(None, None)), None);
    reg.window_closed("popover");
    assert_eq!(
        *feed.network.lock().unwrap(),
        vec![true, false, true, false, true, false]
    );
    assert_eq!(feed.period(), Some(0), "the dashboard still wants rows");
}

#[tokio::test(start_paused = true)]
async fn gpu_interest_follows_the_visible_views_that_ask_for_it() {
    let feed = FakeFeed::new();
    let reg = registry(&["dashboard", "popover"]);
    let f = || -> Arc<dyn LiveFeed> { feed.clone() };
    let gpu = |limit, period| ProcessView {
        gpu: true,
        ..view(limit, &[ProcessSort::Gpu], period)
    };

    reg.set_process_interest("popover", f(), Some(view(Some(5), &[], None)), None);
    reg.set_process_interest("dashboard", f(), Some(gpu(Some(5), Some(5_000))), None);
    reg.window_visible("dashboard", false);
    reg.window_visible("dashboard", true);
    reg.set_process_interest("dashboard", f(), Some(view(None, &[], None)), None);
    reg.set_process_interest("popover", f(), Some(gpu(None, None)), None);
    reg.window_closed("popover");
    assert_eq!(
        *feed.gpu.lock().unwrap(),
        vec![true, false, true, false, true, false]
    );
    assert!(
        feed.network.lock().unwrap().is_empty(),
        "GPU interest is not network interest"
    );
}

/// #15: interest belongs to the page load that asked for it. A reload's new stream ends
/// the old page's interest; the new page's interest, sent before its subscribe, starts
/// with it.
#[tokio::test(start_paused = true)]
async fn a_reloaded_page_does_not_keep_the_old_pages_interest() {
    let feed = FakeFeed::new();
    let reg = registry(&["dashboard"]);
    let f = || -> Arc<dyn LiveFeed> { feed.clone() };

    let (s1, _rx1, id1) = sink();
    reg.subscribe("dashboard", f(), s1, req(0));
    reg.set_process_interest("dashboard", f(), Some(view(None, &[], None)), Some(id1));
    assert_eq!(feed.period(), Some(0));

    // Reload: the new page subscribes without asking for processes.
    let (s2, _rx2, id2) = sink();
    reg.subscribe("dashboard", f(), s2, req(0));
    assert_eq!(feed.period(), None, "the old page's interest ended with it");

    // Next reload: this page asks before its subscribe arrives. It waits for its stream.
    let (s3, rx3, id3) = sink();
    reg.set_process_interest("dashboard", f(), Some(view(None, &[], None)), Some(id3));
    assert_eq!(
        feed.period(),
        None,
        "the current stream ({id2}) is not this page's"
    );
    reg.subscribe("dashboard", f(), s3, req(0));
    assert_eq!(feed.period(), Some(0));
    feed.processes(1, Vec::new());
    assert_eq!(drain(&rx3).await, with_head(&["processes x0"]));
}

#[tokio::test(start_paused = true)]
async fn a_visible_window_with_a_stream_counts_as_detail_interest() {
    let feed = FakeFeed::new();
    let reg = registry(&["dashboard"]);
    let count = || feed.detail.load(Ordering::SeqCst);
    assert_eq!(count(), 0, "tray-only: nothing streams");

    let (s1, _rx1, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s1, req(0));
    assert_eq!(count(), 1);
    let (s1, _rx1b, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s1, req(0));
    assert_eq!(count(), 1, "a page reload re-subscribes but counts once");

    reg.window_visible("popover", false);
    let (s2, _rx2, _) = sink();
    reg.subscribe("popover", feed.clone(), s2, req(0));
    assert_eq!(count(), 1, "the hidden warm popover does not count");
    reg.window_visible("popover", true);
    assert_eq!(count(), 2);
    reg.window_visible("popover", false);
    assert_eq!(count(), 1);

    reg.window_visible("dashboard", false);
    assert_eq!(count(), 0, "every window hidden: back to tray-only");
    reg.window_visible("dashboard", true);
    assert_eq!(count(), 1);
    reg.window_closed("dashboard");
    reg.window_closed("popover");
    assert_eq!(count(), 0, "closing drops it, with no double decrement");
}

/// Every status a window received, as tick intervals.
fn status_intervals(msgs: &[LiveMsg]) -> Vec<u32> {
    msgs.iter()
        .filter_map(|m| match m {
            LiveMsg::Status(s) => Some(s.interval_ms),
            _ => None,
        })
        .collect()
}

/// A window never sees the background's 2 s tick (D-094): subscribing while visible
/// puts its detail interest in before the catch-up sends the first status, and hiding
/// stops the stream before the tick slows.
#[tokio::test(start_paused = true)]
async fn a_visible_window_never_sees_the_background_tick() {
    let feed = FakeFeed::new();
    feed.background_tick.store(true, Ordering::SeqCst);
    feed.set_status(|s| s.interval_ms = 2000);
    let l = layout(1, 1);
    feed.tick(&l, 2_000, 0.0);
    let reg = registry(&["popover"]);
    let (s, rx, _) = sink();
    reg.subscribe("popover", feed.clone(), s, req(60_000));
    assert_eq!(status_intervals(&drain_raw(&rx).await), [1000]);

    reg.window_visible("popover", false);
    settle().await;
    assert_eq!(
        feed.hub.status().interval_ms,
        2000,
        "back in the background"
    );
    reg.window_visible("popover", true);
    settle().await;
    feed.tick(&l, 4_000, 1.0);
    let msgs = drain_raw(&rx).await;
    assert!(
        !status_intervals(&msgs).contains(&2000),
        "{:?}",
        status_intervals(&msgs)
    );

    // Hidden and shown again before the stream runs (occlusion flapping): the 2 s
    // status hiding put on the bus must not reach the window it is now visible again.
    reg.window_visible("popover", false);
    reg.window_visible("popover", true);
    settle().await;
    feed.tick(&l, 6_000, 1.0);
    let msgs = drain_raw(&rx).await;
    assert!(
        !status_intervals(&msgs).contains(&2000),
        "{:?}",
        status_intervals(&msgs)
    );
}

fn proc(pid: i32, cpu: f32, mem: u64) -> ProcessSample {
    ProcessSample {
        pid,
        start_time_us: i64::from(pid) * 10,
        name: format!("p{pid}").into(),
        cpu_pct: cpu,
        mem_bytes: mem,
        compressed_bytes: None,
        threads: 1,
        idle_wakeups_per_s: 0.0,
        energy: 0.0,
        energy_j: 0.0,
        app: None,
        app_main: false,
        disk_read_bps: 0.0,
        disk_write_bps: 0.0,
        net_rx_bps: None,
        net_tx_bps: None,
        gpu_pct: None,
        user: "me".into(),
    }
}

#[tokio::test(start_paused = true)]
async fn processes_go_only_to_windows_with_interest_at_their_period() {
    let feed = FakeFeed::new();
    let reg = registry(&["dashboard", "popover"]);
    let (s1, rx1, _) = sink();
    let (s2, rx2, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s1, req(0));
    reg.subscribe("popover", feed.clone(), s2, req(0));
    reg.set_process_interest(
        "dashboard",
        feed.clone(),
        Some(view(Some(1), &[ProcessSort::Memory], Some(3_000))),
        None,
    );
    for t in 0..7 {
        feed.processes(10_000 + t * 1000, vec![proc(1, 50.0, 1), proc(2, 1.0, 9)]);
    }
    settle().await;
    let batches: Vec<Vec<i32>> = rx1
        .try_iter()
        .filter_map(|m| match m {
            LiveMsg::Processes { rows, .. } => Some(rows.iter().map(|r| r.pid).collect()),
            _ => None,
        })
        .collect();
    assert_eq!(batches, [[2], [2], [2]], "top 1 by memory every 3 s");
    assert!(
        drain(&rx2)
            .await
            .iter()
            .all(|t| !t.starts_with("processes"))
    );
}

#[tokio::test(start_paused = true)]
async fn resubscribe_replaces_and_close_ends_the_stream() {
    let feed = FakeFeed::new();
    let l = layout(1, 1);
    let reg = registry(&["dashboard"]);
    let (old, old_rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), old, req(0));
    let (new, new_rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), new, req(0));
    settle().await;
    assert_eq!(
        feed.hub.bus().subscriber_count(),
        1,
        "the old stream is gone"
    );
    feed.tick(&l, 1_000, 1.0);
    assert!(drain(&old_rx).await.iter().all(|t| !t.starts_with("frame")));
    assert!(drain(&new_rx).await.contains(&"frame 1 @1000".to_string()));

    reg.window_closed("dashboard");
    settle().await;
    assert_eq!(feed.hub.bus().subscriber_count(), 0);
    feed.tick(&l, 2_000, 2.0);
    assert!(drain(&new_rx).await.is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_dropped_receiver_ends_the_stream() {
    let feed = FakeFeed::new();
    let l = layout(1, 1);
    let reg = registry(&["dashboard"]);
    let (s, rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s, req(0));
    drop(rx);
    feed.tick(&l, 1_000, 1.0);
    settle().await;
    assert_eq!(feed.hub.bus().subscriber_count(), 0);
}

/// #4, #12: after a wall-clock step back the source's frames carry a new timeline, with
/// older times. The stream starts over instead of skipping every frame until the clock
/// catches up with what it sent.
#[tokio::test(start_paused = true)]
async fn a_clock_step_back_restarts_the_stream_instead_of_freezing_it() {
    let feed = FakeFeed::new();
    let l1 = layout(1, 1);
    let reg = registry(&["dashboard"]);
    let (s, rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s, req(60_000));
    reg.set_process_interest(
        "dashboard",
        feed.clone(),
        Some(view(Some(5), &[], Some(2_000))),
        None,
    );
    feed.tick(&l1, 600_000, 1.0);
    feed.processes(600_000, Vec::new());
    feed.tick(&l1, 601_000, 1.0);
    // NTP steps the clock back ten minutes. The engine also bumps the layout number,
    // but the timeline is the signal: the same layout here.
    feed.step();
    feed.tick(&l1, 2_000, 2.0);
    feed.processes(2_000, Vec::new());
    feed.tick(&l1, 3_000, 3.0);
    assert_eq!(
        drain(&rx).await,
        with_head(&[
            "layout 1/1",
            "frame 1 @600000",
            "processes x0",
            "frame 1 @601000",
            "frame 1 @2000 t1",
            "processes x0",
            "frame 1 @3000 t1",
        ]),
        "frames and the paced process table both continue on the new timeline"
    );

    // A resume after the step backfills from the new timeline, not from 601,000.
    reg.window_visible("dashboard", false);
    settle().await;
    feed.tick(&l1, 4_000, 4.0);
    reg.window_visible("dashboard", true);
    assert_eq!(drain(&rx).await, vec!["backfill 1 @4000 x1 t1"]);
}

/// #12: a layout change is not a clock step. A frame under a new layout number at a
/// time the channel already sent (a module toggled between the ring read and the bus)
/// is a duplicate, not a reason to start over.
#[tokio::test(start_paused = true)]
async fn a_new_layout_with_an_old_time_is_not_a_reset() {
    let feed = FakeFeed::new();
    let l1 = layout(1, 1);
    let l2 = layout(2, 2);
    for t in 1..=3 {
        feed.tick(&l1, t * 1000, 0.0);
    }
    let reg = registry(&["dashboard"]);
    let (s, rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s, req(60_000));
    feed.tick(&l2, 3_000, 1.0);
    feed.tick(&l2, 4_000, 1.0);
    assert_eq!(
        drain(&rx).await,
        with_head(&[
            "layout 1/1",
            "backfill 1 @1000 x3",
            "layout 2/2",
            "frame 2 @4000",
        ])
    );
}

/// #11: a window hidden across a clock step back. "Now" is before the last thing sent,
/// so a resume measured from that would send nothing and the stream would sit until the
/// clock caught up. It starts over and sends the new timeline from its first row.
#[tokio::test(start_paused = true)]
async fn a_window_hidden_across_a_step_back_resumes_on_the_new_timeline() {
    let feed = FakeFeed::new();
    let l = layout(1, 1);
    for t in 595..=600 {
        feed.tick(&l, t * 1000, 0.0);
    }
    let reg = registry(&["dashboard"]);
    let (s, rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s, req(60_000));
    assert_eq!(
        drain(&rx).await,
        with_head(&["layout 1/1", "backfill 1 @595000 x6"])
    );
    reg.window_visible("dashboard", false);
    settle().await;
    // Hidden: the clock steps back two seconds. The ring keeps 595..597 s.
    feed.step();
    for t in 598..=599 {
        feed.tick(&l, t * 1000, 1.0);
    }
    reg.window_visible("dashboard", true);
    settle().await;
    feed.tick(&l, 600_000, 1.0);
    assert_eq!(
        drain(&rx).await,
        vec!["backfill 1 @598000 x2 t1", "frame 1 @600000 t1"],
        "from the step on, then frames"
    );
}

/// #11, without a timeline: a source whose "now" went back past what was sent still
/// gets a backfill and live frames, not a stream frozen until its clock catches up.
#[tokio::test(start_paused = true)]
async fn a_resume_before_the_last_thing_sent_starts_over() {
    let feed = FakeFeed::new();
    let l = layout(1, 1);
    feed.tick(&l, 600_000, 0.0);
    let reg = registry(&["dashboard"]);
    let (s, rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s, req(60_000));
    drain(&rx).await;
    reg.window_visible("dashboard", false);
    settle().await;
    feed.tick(&l, 2_000, 1.0);
    reg.window_visible("dashboard", true);
    settle().await;
    feed.tick(&l, 3_000, 1.0);
    assert_eq!(
        drain(&rx).await,
        vec!["backfill 1 @2000 x1", "frame 1 @3000"]
    );
}

/// #5: a window hidden for half an hour gets the span it missed in time order, in
/// messages of at most 600 rows, not one message of 1,800.
#[tokio::test(start_paused = true)]
async fn a_long_resume_arrives_in_chunks_in_time_order() {
    let feed = FakeFeed::new();
    let l = layout(1, 1);
    feed.tick(&l, 1_000, 0.0);
    let reg = registry(&["dashboard"]);
    let (s, rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s, req(3_600_000));
    drain(&rx).await;
    reg.window_visible("dashboard", false);
    settle().await;
    for t in 2..=1_801 {
        feed.tick(&l, t * 1000, 0.0);
    }
    reg.window_visible("dashboard", true);
    settle().await;
    feed.tick(&l, 1_802_000, 0.0);
    assert_eq!(
        drain(&rx).await,
        vec![
            "backfill 1 @2000 x600",
            "backfill 1 @602000 x600",
            "backfill 1 @1202000 x600",
            "frame 1 @1802000",
        ]
    );
}

/// #25: a stream that falls more than the bus capacity behind catches up from the ring
/// instead of silently missing frames.
#[tokio::test(start_paused = true)]
async fn a_lagging_stream_catches_up_from_the_ring() {
    let feed = FakeFeed::with_bus(Bus::new(4));
    let l = layout(1, 1);
    let reg = registry(&["dashboard"]);
    let (s, rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s, req(60_000));
    feed.tick(&l, 1_000, 1.0);
    assert_eq!(
        drain(&rx).await,
        with_head(&["layout 1/1", "frame 1 @1000"])
    );
    // Ten frames while the stream task cannot run: the bus keeps the newest four.
    for t in 2..=11 {
        feed.tick(&l, t * 1000, t as f32);
    }
    let got = drain(&rx).await;
    assert_eq!(
        got.first().map(String::as_str),
        Some("backfill 1 @2000 x10"),
        "the missed span from the ring: {got:?}"
    );
    assert!(
        got.iter().skip(1).all(|t| !t.starts_with("frame")),
        "the four frames the bus kept were in that backfill: {got:?}"
    );
}

fn labelled(no: u32) -> Arc<FrameLayout> {
    let key = |s: &str| SeriesKey::parse(s).unwrap();
    Arc::new(FrameLayout {
        layout_no: no,
        series: vec![
            key("cpu.load{core=E0}"),
            key("cpu.load{core=P0}"),
            key("cpu.total"),
            key("gpu.util"),
        ]
        .into(),
    })
}

/// #7: a window that names its series gets only those, in layout order, in every layout,
/// backfill row and frame.
#[tokio::test(start_paused = true)]
async fn a_series_selection_projects_layouts_rows_and_frames() {
    let feed = FakeFeed::new();
    let l = labelled(1);
    let values =
        |base: f32| -> Arc<[f32]> { vec![base, base + 1.0, base + 2.0, base + 3.0].into() };
    feed.frame(&l, 1_000, values(10.0), values(10.0));
    let reg = registry(&["popover"]);
    let (s, rx, _) = sink();
    let series = vec![
        SeriesSelector {
            metric: MetricId::from_static("gpu.util"),
            labels: Labels::default(),
        },
        SeriesSelector {
            metric: MetricId::from_static("cpu.load"),
            labels: Labels::single("core", "P0"),
        },
    ];
    reg.subscribe(
        "popover",
        feed.clone(),
        s,
        LiveRequest {
            series: Some(series),
            ..req(60_000)
        },
    );
    feed.frame(&l, 2_000, values(20.0), values(20.0));
    settle().await;
    let msgs: Vec<LiveMsg> = rx.try_iter().collect();
    let layout = msgs.iter().find_map(|m| match m {
        LiveMsg::Layout { series, .. } => {
            Some(series.iter().map(ToString::to_string).collect::<Vec<_>>())
        }
        _ => None,
    });
    assert_eq!(layout.unwrap(), ["cpu.load{core=P0}", "gpu.util"]);
    let rows = msgs.iter().find_map(|m| match m {
        LiveMsg::Backfill { rows, .. } => Some(rows.clone()),
        _ => None,
    });
    assert_eq!(rows.unwrap(), [[Some(11.0), Some(13.0)]]);
    let frame = msgs.iter().find_map(|m| match m {
        LiveMsg::Frame { values, held, .. } => Some((values.clone(), held.clone())),
        _ => None,
    });
    assert_eq!(
        frame.unwrap(),
        (vec![Some(21.0), Some(23.0)], vec![Some(21.0), Some(23.0)])
    );
}

/// #6: an hour of history no longer goes out in one piece. The last two minutes are sent
/// before `subscribe` returns; older rows follow in chunks, newest first, once the first
/// frame is out, with every layout they use announced up front.
#[tokio::test(start_paused = true)]
async fn the_recent_window_goes_first_and_older_history_follows_in_chunks() {
    let feed = FakeFeed::new();
    let old = layout(1, 1);
    let cur = layout(2, 2);
    // 1,500 rows: 300 in layout 1, then 1,200 in layout 2, ending at 1,500,000.
    for i in 1..=1_500_i64 {
        let l = if i <= 300 { &old } else { &cur };
        feed.tick(l, i * 1000, 0.0);
    }
    let reg = registry(&["dashboard"]);
    let (s, rx, _) = sink();
    let info = reg.subscribe("dashboard", feed.clone(), s, req(3_600_000));
    assert_eq!(
        info.backfill_start_ms,
        Some(1_380_000),
        "two minutes back from 1,500,000"
    );
    assert_eq!(info.backfill_rows, 121);
    assert_eq!(info.earlier_start_ms, Some(1_000));
    assert_eq!(info.earlier_rows, 1_379);
    let before_frame: Vec<String> = rx.try_iter().map(|m| tag(&m)).collect();
    assert_eq!(
        before_frame,
        with_head(&["layout 1/1", "layout 2/2", "backfill 2 @1380000 x121"]),
        "only the recent window before the call returns"
    );

    feed.tick(&cur, 1_501_000, 1.0);
    assert_eq!(
        drain(&rx).await,
        vec![
            "frame 2 @1501000",
            "earlier 2 @901000 x479",
            "earlier 2 @301000 x600",
            "earlier 1 @1000 x300",
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn older_history_still_arrives_when_no_frame_comes() {
    let feed = FakeFeed::new();
    let l = layout(1, 1);
    for i in 1..=200_i64 {
        feed.tick(&l, i * 1000, 0.0);
    }
    feed.set_status(|s| s.paused = true);
    let reg = registry(&["dashboard"]);
    let (s, rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s, req(3_600_000));
    let first = drain(&rx).await;
    assert!(first.iter().all(|t| !t.starts_with("earlier")), "{first:?}");
    tokio::time::sleep(EARLIER_AFTER).await;
    assert_eq!(drain(&rx).await, vec!["earlier 1 @1000 x79"]);
}

#[tokio::test(start_paused = true)]
async fn a_minimum_period_thins_the_frames() {
    let feed = FakeFeed::new();
    let l = layout(1, 1);
    let reg = registry(&["board-1"]);
    let (s, rx, _) = sink();
    reg.subscribe(
        "board-1",
        feed.clone(),
        s,
        LiveRequest {
            min_period_ms: 5_000,
            ..req(0)
        },
    );
    for t in 1..=11 {
        // Real ticks jitter a little either way.
        feed.tick(&l, t * 1000 + if t % 2 == 0 { 30 } else { -30 }, 0.0);
    }
    let frames: Vec<String> = drain(&rx)
        .await
        .into_iter()
        .filter(|t| t.starts_with("frame"))
        .collect();
    assert_eq!(frames, ["frame 1 @970", "frame 1 @6030", "frame 1 @10970"]);
}

/// A sink that runs `hook` before its first message: here, a second `subscribe` for the
/// same window, landing while the first is between its two registry locks.
struct ReentrantSink {
    inner: Arc<dyn LiveSink>,
    hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl LiveSink for ReentrantSink {
    fn send(&self, msg: LiveMsg) -> bool {
        let hook = self.hook.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        self.inner.send(msg)
    }
    fn id(&self) -> u32 {
        self.inner.id()
    }
}

/// #6: StrictMode subscribes twice. With A then B interleaved (B starts and finishes
/// while A is sending its backfill), the newer one, B, must be the stream that stays;
/// A aborts itself instead of replacing it.
#[tokio::test(start_paused = true)]
async fn of_two_interleaved_subscribes_the_newer_one_stays() {
    let feed = FakeFeed::new();
    let l = layout(1, 1);
    feed.tick(&l, 1_000, 0.0);
    let reg = Arc::new(registry(&["dashboard"]));
    let (sa, rx_a, _) = sink();
    let (sb, rx_b, _) = sink();
    let hook = {
        let reg = Arc::clone(&reg);
        let feed: Arc<dyn LiveFeed> = feed.clone();
        Box::new(move || {
            reg.subscribe("dashboard", feed, sb, req(60_000));
        }) as Box<dyn FnOnce() + Send>
    };
    let a: Arc<dyn LiveSink> = Arc::new(ReentrantSink {
        inner: sa,
        hook: Mutex::new(Some(hook)),
    });
    reg.subscribe("dashboard", feed.clone(), a, req(60_000));
    settle().await;
    assert_eq!(feed.hub.bus().subscriber_count(), 1, "one stream");
    assert_eq!(feed.detail.load(Ordering::SeqCst), 1);
    rx_a.try_iter().count();
    rx_b.try_iter().count();
    feed.tick(&l, 2_000, 1.0);
    assert_eq!(drain(&rx_b).await, vec!["frame 1 @2000"], "B streams");
    assert!(drain(&rx_a).await.is_empty(), "A ended");
}

/// #13: the status says when the display is idle (no frames, not stale) and how often
/// this channel sends frames, so the window measures stale in the right unit.
#[tokio::test(start_paused = true)]
async fn status_carries_display_idle_and_the_frame_period() {
    let feed = FakeFeed::new();
    let reg = registry(&["board-1"]);
    let (s, rx, _) = sink();
    reg.subscribe(
        "board-1",
        feed.clone(),
        s,
        LiveRequest {
            min_period_ms: 5_000,
            ..req(0)
        },
    );
    feed.set_status(|s| s.display_idle = true);
    feed.set_status(|s| {
        s.display_idle = false;
        s.interval_ms = 2_000;
    });
    settle().await;
    let statuses: Vec<(u32, u32, bool)> = rx
        .try_iter()
        .filter_map(|m| match m {
            LiveMsg::Status(s) => Some((s.interval_ms, s.frame_period_ms, s.display_idle)),
            _ => None,
        })
        .collect();
    assert_eq!(
        statuses,
        [
            (1_000, 5_000, false),
            (1_000, 5_000, true),
            (2_000, 4_000, false)
        ],
        "at 2 s a 5 s minimum period sends every second tick"
    );
    assert_eq!(frame_period_ms(1_000, 0), 1_000);
    assert_eq!(frame_period_ms(3_000, 5_000), 6_000);
}

/// Performance mode (D-088) paces every visible stream to at least 2 s, whatever it
/// asked for, and says so in the status; it lets a longer requested period stand.
#[tokio::test(start_paused = true)]
async fn performance_mode_paces_frames_to_two_seconds() {
    let feed = FakeFeed::new();
    let l = layout(1, 1);
    let reg = registry(&["dashboard"]);
    let (s, rx, _) = sink();
    reg.subscribe("dashboard", feed.clone(), s, req(0));
    feed.set_status(|s| s.performance = PerformanceReason::LowPowerMode);
    settle().await;
    for t in 1..=6 {
        feed.tick(&l, t * 1000, 0.0);
    }
    settle().await;
    let msgs: Vec<LiveMsg> = rx.try_iter().collect();
    let status = msgs
        .iter()
        .filter_map(|m| match m {
            LiveMsg::Status(s) => Some((s.frame_period_ms, s.performance)),
            _ => None,
        })
        .next_back();
    assert_eq!(status, Some((2_000, PerformanceReason::LowPowerMode)));
    let frames: Vec<i64> = msgs
        .iter()
        .filter_map(|m| match m {
            LiveMsg::Frame { ts_ms, .. } => Some(*ts_ms),
            _ => None,
        })
        .collect();
    assert_eq!(frames, [1_000, 3_000, 5_000]);
    assert_eq!(
        effective_min_period_ms(5_000, PerformanceReason::Setting),
        5_000
    );
    assert_eq!(effective_min_period_ms(0, PerformanceReason::Off), 0);
}

#[tokio::test(start_paused = true)]
async fn holds_go_before_the_first_frame_and_again_only_when_they_change() {
    let feed = FakeFeed::new();
    let l1 = layout(1, 2);
    let reg = registry(&["main"]);
    let (s, rx, _) = sink();
    reg.subscribe("main", feed.clone(), s, req(0));
    let tray: Arc<[u32]> = vec![2_500, 25_000].into();
    let v: Arc<[f32]> = vec![1.0, 1.0].into();
    feed.frame_with_holds(&l1, 1_000, v.clone(), v.clone(), Arc::clone(&tray));
    feed.frame_with_holds(&l1, 2_000, v.clone(), v.clone(), Arc::clone(&tray));
    // A window opened: the second series is sampled every tick now.
    feed.frame_with_holds(&l1, 3_000, v.clone(), v.clone(), vec![2_500, 2_500].into());
    assert_eq!(
        drain_all(&rx).await,
        with_head(&[
            "layout 1/2",
            "holds 1 [2500, 25000]",
            "frame 1 @1000",
            "frame 1 @2000",
            "holds 1 [2500, 2500]",
            "frame 1 @3000",
        ])
    );
}

#[tokio::test(start_paused = true)]
async fn a_thinned_stream_holds_samples_for_at_least_its_frame_period() {
    let feed = FakeFeed::new();
    let l1 = layout(1, 2);
    let reg = registry(&["board-1"]);
    let (s, rx, _) = sink();
    reg.subscribe(
        "board-1",
        feed.clone(),
        s,
        LiveRequest {
            min_period_ms: 5_000,
            ..req(0)
        },
    );
    let v: Arc<[f32]> = vec![1.0, 1.0].into();
    feed.frame_with_holds(&l1, 1_000, v.clone(), v, vec![2_500, 25_000].into());
    let holds: Vec<Vec<u32>> = drain_raw(&rx)
        .await
        .into_iter()
        .filter_map(|m| match m {
            LiveMsg::Holds { holds_ms, .. } => Some(holds_ms),
            _ => None,
        })
        .collect();
    // Frames 5 s apart: one held for 2.5 frame periods, the slower series unchanged.
    assert_eq!(holds, [vec![12_500, 25_000]]);
}

#[tokio::test(start_paused = true)]
async fn layout_carries_catalog_kinds_and_backfill_carries_holds() {
    let feed = FakeFeed::new();
    let l = Arc::new(FrameLayout {
        layout_no: 1,
        series: ["power.gpu", "mem.used", "net.rx{iface=en0}", "made.up"]
            .iter()
            .map(|k| SeriesKey::parse(k).unwrap())
            .collect(),
    });
    let v: Arc<[f32]> = vec![1.0; 4].into();
    feed.frame_with_holds(
        &l,
        1_000,
        v.clone(),
        v,
        vec![25_000, 2_500, 25_000, 2_500].into(),
    );
    let reg = registry(&["main"]);
    let (s, rx, _) = sink();
    reg.subscribe("main", feed.clone(), s, req(60_000));
    let msgs = drain_raw(&rx).await;
    let kinds = msgs.iter().find_map(|m| match m {
        LiveMsg::Layout { kinds, .. } => Some(kinds.clone()),
        _ => None,
    });
    assert_eq!(
        kinds,
        Some(vec![
            MetricKind::Mean,
            MetricKind::Gauge,
            MetricKind::Rate,
            MetricKind::Gauge,
        ]),
        "a metric this build does not know is a gauge"
    );
    let holds = msgs.iter().find_map(|m| match m {
        LiveMsg::Backfill { holds_ms, .. } => Some(holds_ms.clone()),
        _ => None,
    });
    assert_eq!(holds, Some(vec![25_000, 2_500, 25_000, 2_500]));
}

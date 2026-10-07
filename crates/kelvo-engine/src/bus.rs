//! The live bus: one `tokio::sync::broadcast` channel per host (architecture.md, Engine).
//!
//! The engine publishes and never waits. A subscriber that falls more than the channel
//! capacity behind loses the oldest messages; [`Subscriber`] skips past the loss, counts
//! it and carries on. Frames carry their layout, so a subscriber that missed a
//! [`BusMsg::Layout`] can still read every later frame.

use std::sync::Arc;

use kelvo_collect::ProcessSample;
use kelvo_schema::{
    Capabilities, Catalog, Event, FrameLenMismatch, FrameView, HostId, SeriesKey, Snapshot,
};
use tokio::sync::broadcast::{self, error::RecvError, error::TryRecvError};

/// Messages kept for a slow subscriber before it starts dropping (about a minute of
/// frames at 1 Hz plus the status and process messages around them).
pub const BUS_CAPACITY: usize = 128;

/// The ordered series of a frame. `layout_no` is session-scoped: it numbers layouts
/// within one engine run and never goes to disk.
#[derive(Debug, PartialEq)]
pub struct FrameLayout {
    pub layout_no: u32,
    pub series: Arc<[SeriesKey]>,
}

/// One tick of values.
#[derive(Debug)]
pub struct LiveFrame {
    pub ts_ms: i64,
    /// The base tick this frame was taken at (after back-off): the spacing the ring
    /// buffer expects before the next one.
    pub interval_ms: u32,
    pub layout: Arc<FrameLayout>,
    /// Which run of the source's wall clock `ts_ms` is on. The source bumps it when its
    /// clock is stepped (D-064) and nowhere else: rows at or after the first frame of a
    /// new timeline replace whatever a consumer held from that time on.
    pub timeline: u32,
    /// What was measured this tick, in layout order. `NaN` for a series not sampled on
    /// this tick (a gauge sampled every 10 s is `NaN` on the other nine ticks at 1 s). This is
    /// what the ring buffer and the rollups see: a held value is never a measurement.
    pub values: Arc<[f32]>,
    /// The latest value of each series while it is still current, `NaN` once it is
    /// stale. A value stays current for 2.5 times its sampling period (the largest of the
    /// catalog period, its collector's current period and the base tick), so disk capacity
    /// sampled every 60 s shows on every frame, while a series that stopped arriving
    /// turns into a gap after two missed samples. Build displays (tray, Snapshot) from
    /// this, never history.
    pub held: Arc<[f32]>,
    /// How long each series' sample stays current, in layout order: the same 2.5 times its
    /// sampling period as on this tick that `held` uses. Two samples of a series further
    /// apart than the earlier one's hold have a gap between them (D-090). The engine
    /// shares one `Arc` across frames until a period changes.
    pub holds: Arc<[u32]>,
}

impl LiveFrame {
    /// The typed view over the held values.
    pub fn snapshot(&self, host: HostId, catalog: &Catalog) -> Result<Snapshot, FrameLenMismatch> {
        let view = FrameView::new(self.ts_ms, &self.layout.series, &self.held)?;
        Ok(Snapshot::from_frame(host, &view, catalog))
    }
}

/// Process rows from one sample of the processes collector.
#[derive(Debug)]
pub struct ProcessBatch {
    pub ts_ms: i64,
    pub rows: Vec<ProcessSample>,
    /// The wall time the rows' `gpu_pct` shares are of, ms, when GPU was measured: since
    /// the GPU collector's previous pass, which can span several process samples (D-099).
    pub gpu_span_ms: Option<i64>,
}

/// Engine state the UI shows, published on every change.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EngineStatus {
    /// The base tick in effect, after back-off.
    pub interval_ms: u32,
    pub paused: bool,
    pub asleep: bool,
    pub on_battery: bool,
    pub low_power_mode: bool,
    /// The base tick is slowed because of battery or Low Power Mode.
    pub backed_off: bool,
    /// No visible window shows detail the tray does not (D-061): the app is in the
    /// background, with the menu bar only. The base tick is then at least
    /// [`crate::BACKGROUND_TICK_MS`], temperatures and processes run at their slow
    /// periods, and the tray redraws less often (D-094). Not a back-off: `backed_off`
    /// stays false for it.
    pub backgrounded: bool,
    /// The display is asleep or the screen is locked. The engine keeps sampling and
    /// persisting; consumers that only draw (the tray) stop.
    pub display_idle: bool,
    /// After the wall clock stepped back: nothing is persisted for ticks before this
    /// wall-clock time (ms), at most an hour after the step (D-070). Live values keep
    /// flowing. `None` while history is written normally.
    pub history_held_until: Option<i64>,
    /// Whether Performance mode is in effect, and why (D-088).
    pub performance: kelvo_schema::PerformanceReason,
    /// Battery, adapter or charging, from the same power-source change as `on_battery`
    /// (D-092).
    pub power_source: kelvo_schema::PowerSource,
    /// The reported interface carrying the default route, as the network collector last
    /// read it (on probe and every 60 s); `None` on a VPN tunnel or with no route (D-092).
    pub primary_iface: kelvo_collect::PrimaryIface,
}

#[derive(Clone, Debug)]
pub enum BusMsg {
    /// Published before the first frame that uses it.
    Layout(Arc<FrameLayout>),
    Frame(Arc<LiveFrame>),
    Caps(Arc<Capabilities>),
    Processes(Arc<ProcessBatch>),
    Status(EngineStatus),
    /// A detector or alert rule fired (v1.2). Published as soon as it is queued to the
    /// store with a commit request (`Writer::commit_soon`), before that commit lands.
    Event(Arc<Event>),
}

/// The publishing side. Cloneable; publishing never blocks.
#[derive(Clone)]
pub struct Bus {
    tx: broadcast::Sender<BusMsg>,
}

impl Default for Bus {
    fn default() -> Self {
        Self::new(BUS_CAPACITY)
    }
}

impl Bus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity.max(1));
        Self { tx }
    }

    /// Sends to every current subscriber. With none, the message is dropped.
    pub fn publish(&self, msg: BusMsg) {
        let _ = self.tx.send(msg);
    }

    pub fn subscribe(&self) -> Subscriber {
        Subscriber {
            rx: self.tx.subscribe(),
            dropped: 0,
        }
    }

    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

/// What [`Subscriber::recv_event`] got.
#[derive(Debug)]
pub enum Recv {
    Msg(BusMsg),
    /// This many messages were lost to lag before the next one; a consumer that keeps
    /// state from the stream (a window's live channel) should resynchronize.
    Lagged(u64),
    /// Every [`Bus`] handle is gone.
    Closed,
}

/// One consumer of the bus.
pub struct Subscriber {
    rx: broadcast::Receiver<BusMsg>,
    dropped: u64,
}

impl Subscriber {
    /// The next message, skipping over anything lost to lag. `None` once every [`Bus`]
    /// handle is gone.
    pub async fn recv(&mut self) -> Option<BusMsg> {
        loop {
            match self.rx.recv().await {
                Ok(m) => return Some(m),
                Err(RecvError::Lagged(n)) => self.dropped += n,
                Err(RecvError::Closed) => return None,
            }
        }
    }

    /// Like [`Subscriber::recv`], but reports a lag instead of skipping past it.
    pub async fn recv_event(&mut self) -> Recv {
        match self.rx.recv().await {
            Ok(m) => Recv::Msg(m),
            Err(RecvError::Lagged(n)) => {
                self.dropped += n;
                Recv::Lagged(n)
            }
            Err(RecvError::Closed) => Recv::Closed,
        }
    }

    /// Like [`Subscriber::recv`], blocking the calling thread. Not for async contexts.
    pub fn blocking_recv(&mut self) -> Option<BusMsg> {
        loop {
            match self.rx.blocking_recv() {
                Ok(m) => return Some(m),
                Err(RecvError::Lagged(n)) => self.dropped += n,
                Err(RecvError::Closed) => return None,
            }
        }
    }

    /// The next message if one is ready.
    pub fn try_recv(&mut self) -> Option<BusMsg> {
        loop {
            match self.rx.try_recv() {
                Ok(m) => return Some(m),
                Err(TryRecvError::Lagged(n)) => self.dropped += n,
                Err(TryRecvError::Empty | TryRecvError::Closed) => return None,
            }
        }
    }

    /// Messages this subscriber lost by lagging.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(interval_ms: u32) -> BusMsg {
        BusMsg::Status(EngineStatus {
            interval_ms,
            ..EngineStatus::default()
        })
    }

    #[test]
    fn lagging_subscriber_drops_oldest_and_publisher_never_blocks() {
        let bus = Bus::new(4);
        let mut slow = bus.subscribe();
        let mut fast = bus.subscribe();
        for i in 0..10 {
            bus.publish(status(i));
            // The fast subscriber keeps up.
            assert!(matches!(fast.try_recv(), Some(BusMsg::Status(s)) if s.interval_ms == i));
        }
        let mut seen = Vec::new();
        while let Some(BusMsg::Status(s)) = slow.try_recv() {
            seen.push(s.interval_ms);
        }
        assert_eq!(
            seen,
            vec![6, 7, 8, 9],
            "keeps the newest `capacity` messages"
        );
        assert_eq!(slow.dropped(), 6);
        assert_eq!(fast.dropped(), 0);
    }

    #[test]
    fn publish_without_subscribers_is_fine() {
        let bus = Bus::new(2);
        for i in 0..10 {
            bus.publish(status(i));
        }
        let mut late = bus.subscribe();
        assert!(
            late.try_recv().is_none(),
            "a new subscriber sees only new messages"
        );
    }
}

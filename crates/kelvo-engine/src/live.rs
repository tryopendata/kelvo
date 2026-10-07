//! A host's live output as consumers see it: the [`Bus`] plus what a late joiner needs
//! (architecture.md, Data flow; D-066).
//!
//! A [`Source`](crate::Source) publishes every live message through its host's
//! [`LiveHub`]. The hub records what it retains (the one-hour ring of raw frames, the
//! latest frame and layout, the latest status) and then puts the message on the bus. Since
//! retention happens before the publish, a consumer that subscribes and then reads the
//! ring never misses a frame: anything published before the read is in the ring, and
//! anything after reaches its subscriber (frames already in the ring are recognized by
//! timestamp).
//!
//! The ring lives here, not in the engine, so a remote source (v4) backfills windows the
//! same way the local engine does.
//!
//! Before a source's first frame the shell can warm the ring from the store
//! ([`LiveHub::warm`]), so a window opened right after launch draws the hour before it
//! from the 10 s history instead of starting empty.
//!
//! The hub also keeps the last hour of per-app network buckets and the open ones
//! (D-089), so a range query newer than the store's last commit (up to 5 minutes old)
//! still has its rows: [`LiveHub::recent_net_buckets`]. In the same way it holds the
//! history rollups and their last 15 minutes of rows ([`crate::RECENT_ROWS_MS`]), so
//! `query_history` answers through now (D-092): [`LiveHub::recent_rows`]. And it adds
//! every process batch to the last hour of per-process usage (D-093, D-099):
//! [`LiveHub::usage_by_app`].

use std::sync::{Arc, Mutex, MutexGuard};

use std::collections::BTreeMap;

use kelvo_schema::lock::LockExt;
use kelvo_schema::{CATALOG, HostId, SeriesKey};
use kelvo_store::{BucketRow, HistoryResult, NetBucket};

use crate::accum::{Rollups, recent_rows};
use crate::bus::{Bus, BusMsg, EngineStatus, FrameLayout, LiveFrame, Subscriber};
use crate::engine::hold_ms;
use crate::netacc::{NetRing, NetSlot};
use crate::ring::{BackfillSegment, Ring};
use crate::usage::{UsageByApp, UsageKey, UsageRing};

/// `layout_no` of ring rows read back from the store by [`LiveHub::warm`]. A source
/// numbers its own layouts from 0 within a run, so this one never collides with them.
pub const WARM_LAYOUT_NO: u32 = u32::MAX;

/// The hub's per-app network buckets over a range, as of one instant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecentNet {
    /// Measured buckets `(bucket_ts, bucket)`, closed then open, by start.
    pub buckets: Vec<(i64, NetBucket)>,
    /// Where the engine's buckets stop being final: the start of the oldest one it
    /// still has open (one stream, apps or interface, has not reported past it, so it
    /// still grows and its remainder is not meaningful yet, D-089), or with none open,
    /// the end of the newest it closed or wrote, since the next sample lands after it,
    /// or with neither, the start of the bucket it last restarted in (it counts nothing
    /// before a restart). `None` only before the engine starts.
    pub complete_to_ms: Option<i64>,
}

#[derive(Default)]
struct Retained {
    ring: Ring,
    frame: Option<Arc<LiveFrame>>,
    layout: Option<Arc<FrameLayout>>,
    status: EngineStatus,
}

/// One host's bus with its ring buffer and latest values. Cloneable; clones share state.
#[derive(Clone, Default)]
pub struct LiveHub {
    bus: Bus,
    retained: Arc<Mutex<Retained>>,
    /// Its own lock: the engine updates it only on ticks that sampled network bytes, and
    /// a range query copying buckets out must not hold up frame publishing.
    net: Arc<Mutex<NetRing>>,
    /// Its own lock as well: the engine adds to it on every persisted tick, and a history
    /// query copies rows out of it.
    rollups: Arc<Mutex<Rollups>>,
    /// Its own lock: a process batch adds to it, and a range query sums it.
    usage: Arc<Mutex<UsageRing>>,
}

impl LiveHub {
    /// A hub publishing on `bus`.
    pub fn new(bus: Bus) -> Self {
        Self {
            bus,
            retained: Arc::default(),
            net: Arc::default(),
            rollups: Arc::default(),
            usage: Arc::default(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Retained> {
        // Nothing panics while holding this lock; a poisoned one still holds valid data.
        self.retained.lock_ok()
    }

    /// Records what the hub retains from `msg`, then publishes it. Never blocks on
    /// subscribers.
    pub fn publish(&self, msg: BusMsg) {
        {
            let mut r = self.lock();
            match &msg {
                BusMsg::Frame(f) => {
                    r.ring.push(
                        f.ts_ms,
                        f.interval_ms,
                        f.timeline,
                        Arc::clone(&f.layout),
                        Arc::clone(&f.values),
                        Arc::clone(&f.holds),
                    );
                    r.layout = Some(Arc::clone(&f.layout));
                    r.frame = Some(Arc::clone(f));
                }
                BusMsg::Layout(l) => r.layout = Some(Arc::clone(l)),
                BusMsg::Status(s) => r.status = s.clone(),
                BusMsg::Caps(_) | BusMsg::Processes(_) | BusMsg::Event(_) => {}
            }
        }
        if let BusMsg::Processes(batch) = &msg {
            self.usage()
                .push_with_gpu_span(batch.ts_ms, &batch.rows, batch.gpu_span_ms);
        }
        self.bus.publish(msg);
    }

    /// A new subscriber; it sees only messages published from now on.
    pub fn subscribe(&self) -> Subscriber {
        self.bus.subscribe()
    }

    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    /// Fills an empty ring from stored history, one row per bucket of `history` (the 10 s
    /// tier) with each series' bucket average, placed at the bucket's end: a mean or rate
    /// covers the span before its sample (D-090). Each series stays current for the hold of
    /// the slower of the bucket and its catalog period, so consecutive buckets join and a
    /// missing one is a hole. The rows carry their own layout ([`WARM_LAYOUT_NO`]) on
    /// timeline 0, the first timeline of a run, and age out as frames arrive.
    ///
    /// Does nothing once the ring has rows: a frame is always newer and better than a
    /// stored average. Returns the rows added.
    pub fn warm(&self, history: &HistoryResult) -> usize {
        let Ok(interval) = u32::try_from(history.bucket_ms) else {
            return 0;
        };
        if history.series.is_empty() || interval == 0 {
            return 0;
        }
        let n = history.series.len();
        let mut rows: BTreeMap<i64, Vec<f32>> = BTreeMap::new();
        for (i, s) in history.series.iter().enumerate() {
            for p in &s.points {
                if let Some(v) = rows
                    .entry(p.t + history.bucket_ms)
                    .or_insert_with(|| vec![f32::NAN; n])
                    .get_mut(i)
                {
                    *v = p.avg;
                }
            }
        }
        let layout = Arc::new(FrameLayout {
            layout_no: WARM_LAYOUT_NO,
            series: history.series.iter().map(|s| s.key.clone()).collect(),
        });
        let holds: Arc<[u32]> = history
            .series
            .iter()
            .map(|s| {
                let period = CATALOG
                    .iter()
                    .find(|d| d.id == s.key.metric)
                    .map_or(0, |d| u32::from(d.period_s) * 1_000);
                hold_ms(period.max(interval))
            })
            .collect();
        let mut r = self.lock();
        if !r.ring.is_empty() {
            return 0;
        }
        let added = rows.len();
        for (ts_ms, values) in rows {
            r.ring.push(
                ts_ms,
                interval,
                0,
                Arc::clone(&layout),
                values.into(),
                Arc::clone(&holds),
            );
        }
        added
    }

    /// Raw ring rows at or after `since_ms`, as evenly spaced segments in time order.
    pub fn backfill(&self, since_ms: i64) -> Vec<BackfillSegment> {
        self.lock().ring.backfill(since_ms)
    }

    /// The most recent frame. Its `ts_ms` is "now" on the source's clock, which is what a
    /// backfill window is measured from.
    pub fn latest_frame(&self) -> Option<Arc<LiveFrame>> {
        self.lock().frame.clone()
    }

    /// The current layout, published before any frame uses it.
    pub fn layout(&self) -> Option<Arc<FrameLayout>> {
        self.lock().layout.clone()
    }

    /// The latest status the source published (default before the first one).
    pub fn status(&self) -> EngineStatus {
        self.lock().status.clone()
    }

    fn net(&self) -> MutexGuard<'_, NetRing> {
        // As for `lock`: nothing panics while holding it.
        self.net.lock_ok()
    }

    /// Per-app network buckets overlapping `[from_ms, to_ms)` from the last hour, the
    /// open ones included, by bucket start (10 s aligned). Only buckets with a measured
    /// span are here. Pass them all to `Reader::net_by_app_with`: they replace the
    /// store's rows for the same buckets, which may be older or not committed yet.
    pub fn recent_net_buckets(&self, from_ms: i64, to_ms: i64) -> Vec<(i64, NetBucket)> {
        self.recent_net(from_ms, to_ms).buckets
    }

    /// [`LiveHub::recent_net_buckets`] and, from the same instant, where the engine's
    /// buckets stop being final. The lock is held only to copy pointers out.
    pub fn recent_net(&self, from_ms: i64, to_ms: i64) -> RecentNet {
        let snap = self.net().snapshot(from_ms, to_ms);
        let complete_to_ms = snap.complete_to_ms;
        RecentNet {
            buckets: snap.into_buckets(),
            complete_to_ms,
        }
    }

    /// Appends closed buckets, replaces the open ones and records where the engine's
    /// buckets stop being final with none open (engine). Called before the closed buckets are queued to the store, so a
    /// committed row never meets an older copy in the ring.
    pub(crate) fn net_update(
        &self,
        closed: &[NetSlot],
        open: &[NetSlot],
        final_to_ms: Option<i64>,
    ) {
        self.net().update(closed, open, final_to_ms);
    }

    /// Forgets buckets ending after `ts_ms`: the wall clock stepped back (engine).
    pub(crate) fn net_drop_after(&self, ts_ms: i64) {
        self.net().drop_after(ts_ms);
    }

    /// Forgets every bucket: another store, so history was reset (engine).
    pub(crate) fn net_clear(&self) {
        self.net().clear();
    }

    fn usage(&self) -> MutexGuard<'_, UsageRing> {
        // As for `lock`: nothing panics while holding it.
        self.usage.lock_ok()
    }

    /// Use per app over `[from_ms, to_ms)` from the last hour of process batches, the
    /// largest `limit` by `by` (D-093, D-099).
    pub fn usage_by_app(&self, from_ms: i64, to_ms: i64, by: UsageKey, limit: usize) -> UsageByApp {
        self.usage().by_app(from_ms, to_ms, by, limit)
    }

    /// Forgets usage buckets ending after `ts_ms`: the wall clock stepped back (engine).
    pub(crate) fn usage_drop_after(&self, ts_ms: i64) {
        self.usage().drop_after(ts_ms);
    }

    pub(crate) fn rollups(&self) -> MutexGuard<'_, Rollups> {
        // As for `lock`: nothing panics while holding it.
        self.rollups.lock_ok()
    }

    /// The history bucket rows of buckets starting in `[from_ms, to_ms)` the engine
    /// emitted in the last 15 minutes, then its open 10 s and 1 minute buckets as
    /// they are, all stamped `host`. Pass them to `Reader::history_after`: they replace
    /// the store's rows for the same bucket and layout, which may be older or not
    /// committed yet. The engine keeps a row here before it queues it to the writer. The
    /// lock is held only to take shared copies.
    pub fn recent_rows(&self, host: HostId, from_ms: i64, to_ms: i64) -> Vec<BucketRow> {
        let rows = self.rollups().recent(host, from_ms, to_ms);
        recent_rows(rows)
    }

    /// Forgets the recent rows and the open buckets: the host's history was cleared
    /// (`clear_history`), and none of it may come back through `query_history` or a
    /// later emitted row. Call it before the store clears the host, so a bucket closing
    /// meanwhile holds only what was measured after, and again once the store is done. The
    /// last hour of per-app usage goes with them.
    pub fn forget_recent_rows(&self) {
        self.rollups().forget();
        // Per-app usage is history too, even though it is never stored.
        *self.usage() = UsageRing::default();
    }

    /// How long a history point of `key` in buckets of `bucket_ms`, in a range starting
    /// at `from_ms`, stays current (`HistorySeries.hold_ms`, D-092): the hold of the
    /// slowest period the series is sampled at (its catalog period, its collector's idle
    /// cadence slowed by Performance mode, and the backed-off base tick) under the
    /// current settings, or under any settings this session when the range starts before
    /// a period last got faster; or the bucket width if that is larger. Two consecutive
    /// points further apart than this have a hole between them.
    pub fn history_hold_ms(&self, key: &SeriesKey, bucket_ms: i64, from_ms: i64) -> i64 {
        let catalog = CATALOG
            .iter()
            .find(|d| d.id == key.metric)
            .map_or(0, |d| u32::from(d.period_s) * 1_000);
        let period = self.rollups().history_period_ms(key, catalog, from_ms);
        i64::from(hold_ms(period)).max(bucket_ms)
    }
}

#[cfg(test)]
mod tests {
    use kelvo_schema::{MetricId, SeriesKey};

    use super::*;

    fn layout(no: u32) -> Arc<FrameLayout> {
        Arc::new(FrameLayout {
            layout_no: no,
            series: vec![SeriesKey::bare(MetricId::from_static("cpu.total"))].into(),
        })
    }

    fn frame(ts_ms: i64, layout: &Arc<FrameLayout>) -> BusMsg {
        BusMsg::Frame(Arc::new(LiveFrame {
            ts_ms,
            interval_ms: 1_000,
            layout: Arc::clone(layout),
            timeline: layout.layout_no,
            values: vec![1.0].into(),
            held: vec![1.0].into(),
            holds: vec![2_500].into(),
        }))
    }

    fn stored(points: &[(i64, f32)], metric: &'static str) -> kelvo_store::SeriesPoints {
        kelvo_store::SeriesPoints {
            key: SeriesKey::bare(MetricId::from_static(metric)),
            points: points
                .iter()
                .map(|&(t, avg)| kelvo_store::Point {
                    t,
                    min: avg,
                    max: avg,
                    avg,
                })
                .collect(),
        }
    }

    #[test]
    fn warm_fills_an_empty_ring_from_stored_buckets() {
        let hub = LiveHub::default();
        // Buckets at 0, 10 s and 30 s: the one at 20 s is missing (asleep).
        let history = HistoryResult {
            tier: kelvo_schema::Tier::S10,
            bucket_ms: 10_000,
            series: vec![
                stored(&[(0, 10.0), (10_000, 20.0), (30_000, 30.0)], "cpu.total"),
                stored(&[(0, 5.0e11)], "disk.used"),
            ],
            gaps: Vec::new(),
        };
        assert_eq!(hub.warm(&history), 3);

        let segs = hub.backfill(i64::MIN);
        assert_eq!(segs.len(), 2, "the missing bucket is a hole");
        let first = &segs[0];
        assert_eq!(first.layout.layout_no, WARM_LAYOUT_NO);
        assert_eq!(first.interval_ms, 10_000);
        assert_eq!(first.timeline, 0);
        assert_eq!(first.start_ms, 10_000, "a row sits at its bucket's end");
        assert_eq!(first.rows.len(), 2);
        assert_eq!(&first.rows[0][..], &[10.0, 5.0e11]);
        assert!(first.rows[1][1].is_nan(), "not sampled in that bucket");
        // cpu.total at 1 s: the bucket's hold; disk.used at 60 s: its own, longer one.
        assert_eq!(&first.holds[..], &[hold_ms(10_000), hold_ms(60_000)]);
        assert_eq!(segs[1].start_ms, 40_000);

        // Frames that follow keep the stored rows before them.
        let l = layout(0);
        hub.publish(frame(50_000, &l));
        assert_eq!(hub.backfill(i64::MIN).len(), 3);
        // A ring with rows is never warmed again.
        assert_eq!(hub.warm(&history), 0);
    }

    #[test]
    fn warm_does_nothing_after_the_first_frame() {
        let hub = LiveHub::default();
        hub.publish(frame(50_000, &layout(0)));
        let history = HistoryResult {
            tier: kelvo_schema::Tier::S10,
            bucket_ms: 10_000,
            series: vec![stored(&[(0, 10.0)], "cpu.total")],
            gaps: Vec::new(),
        };
        assert_eq!(hub.warm(&history), 0);
        assert_eq!(hub.backfill(i64::MIN).len(), 1);
    }

    fn s10(series: Vec<kelvo_store::SeriesPoints>) -> HistoryResult {
        HistoryResult {
            tier: kelvo_schema::Tier::S10,
            bucket_ms: 10_000,
            series,
            gaps: Vec::new(),
        }
    }

    #[test]
    fn warm_ignores_unusable_history_and_leaves_the_ring_open() {
        let hub = LiveHub::default();
        let cpu = || stored(&[(0, 10.0)], "cpu.total");
        for bucket_ms in [0, -10_000, i64::from(u32::MAX) + 1] {
            let h = HistoryResult {
                bucket_ms,
                ..s10(vec![cpu()])
            };
            assert_eq!(hub.warm(&h), 0, "bucket_ms {bucket_ms}");
        }
        assert_eq!(hub.warm(&s10(Vec::new())), 0, "no series");
        assert_eq!(
            hub.warm(&s10(vec![stored(&[], "cpu.total")])),
            0,
            "no points"
        );
        assert!(hub.backfill(i64::MIN).is_empty());
        // None of those took the ring's one warm.
        assert_eq!(hub.warm(&s10(vec![cpu()])), 1);
    }

    #[test]
    fn warm_holds_a_metric_the_catalog_does_not_know_for_one_bucket() {
        // Stored by a newer build, say, before a downgrade.
        let hub = LiveHub::default();
        hub.warm(&s10(vec![stored(&[(0, 1.0)], "test.unknown")]));
        let segs = hub.backfill(i64::MIN);
        assert_eq!(&segs[0].holds[..], &[hold_ms(10_000)]);
    }

    #[test]
    fn warm_keeps_only_the_ring_span() {
        // Two hours of buckets: the older hour ages out as the rows go in.
        let hub = LiveHub::default();
        let points: Vec<(i64, f32)> = (0..720).map(|i| (i * 10_000, i as f32)).collect();
        assert_eq!(hub.warm(&s10(vec![stored(&points, "cpu.total")])), 720);
        let segs = hub.backfill(i64::MIN);
        assert_eq!(segs.len(), 1);
        let newest = 720 * 10_000;
        assert_eq!(segs[0].start_ms, newest - crate::RING_SPAN_MS);
        assert_eq!(segs[0].rows.len(), 361);
    }

    #[test]
    fn a_first_frame_inside_the_last_stored_bucket_drops_only_that_row() {
        // Kelvo restarted within seconds: the last stored bucket (20-30 s) ends at 30 s,
        // after the first frame at 25 s. The ring keeps time order by dropping that row,
        // not the hour before it.
        let hub = LiveHub::default();
        hub.warm(&s10(vec![stored(
            &[(0, 1.0), (10_000, 2.0), (20_000, 3.0)],
            "cpu.total",
        )]));
        hub.publish(frame(25_000, &layout(0)));
        let segs = hub.backfill(i64::MIN);
        let shape: Vec<_> = segs
            .iter()
            .map(|s| (s.layout.layout_no, s.start_ms, s.rows.len()))
            .collect();
        assert_eq!(shape, vec![(WARM_LAYOUT_NO, 10_000, 2), (0, 25_000, 1)]);
    }

    #[test]
    fn retains_before_publishing() {
        let hub = LiveHub::default();
        let mut sub = hub.subscribe();
        let l = layout(1);
        hub.publish(BusMsg::Layout(Arc::clone(&l)));
        hub.publish(frame(1_000, &l));
        hub.publish(frame(2_000, &l));
        hub.publish(BusMsg::Status(EngineStatus {
            interval_ms: 2_000,
            ..EngineStatus::default()
        }));
        // Every frame a subscriber sees is already in the ring.
        let mut frames = 0;
        while let Some(m) = sub.try_recv() {
            if let BusMsg::Frame(f) = m {
                frames += 1;
                assert!(hub.backfill(f.ts_ms)[0].start_ms <= f.ts_ms);
            }
        }
        assert_eq!(frames, 2);
        assert_eq!(hub.latest_frame().map(|f| f.ts_ms), Some(2_000));
        assert_eq!(hub.layout().map(|l| l.layout_no), Some(1));
        assert_eq!(hub.status().interval_ms, 2_000);
        assert_eq!(hub.backfill(i64::MIN)[0].rows.len(), 2);
    }

    #[test]
    fn a_clock_step_back_drops_the_rows_it_overlaps() {
        let hub = LiveHub::default();
        let l1 = layout(1);
        let l2 = layout(2);
        hub.publish(frame(4_000, &l1));
        hub.publish(frame(10_000, &l1));
        hub.publish(frame(11_000, &l1));
        hub.publish(BusMsg::Layout(Arc::clone(&l2)));
        hub.publish(frame(5_000, &l2));
        let segs = hub.backfill(i64::MIN);
        let shape: Vec<_> = segs
            .iter()
            .map(|s| (s.start_ms, s.layout.layout_no, s.timeline))
            .collect();
        assert_eq!(shape, [(4_000, 1, 1), (5_000, 2, 2)]);
    }
}

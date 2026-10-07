//! Rollup accumulators for the persisted tiers (architecture.md, Engine).
//!
//! One [`TierAcc`] per tier tracks min, max, a weighted sum and count per series for the
//! bucket in progress, one set per layout seen in that bucket. Each value comes with its
//! weight: for a `Mean` or `Rate` series the span since its collector's previous read
//! (its current period when the read is a new baseline), so a bucket where a collector
//! moves between 10 s and 1 s is not tilted either way, and 1 for a gauge, whose average
//! stays the plain mean of its readings (D-092). Min and max ignore weights. Buckets are
//! wall-clock multiples of the tier width. When a tick lands in a later bucket, every
//! layout's row for the old bucket is emitted and the state resets. `NaN` (not sampled)
//! is skipped; a series with no sample in the bucket gets a `NaN` triple, never zeros.
//!
//! A layout change inside a bucket keeps the old layout's partial state, so the bucket
//! produces one row per layout (the store keys rows on `(host, bucket_ts, layout)`), and a
//! layout that comes back within the same bucket resumes its own row instead of
//! overwriting it with a shorter one.
//!
//! [`TierAcc::flush`] (sleep, pause, shutdown) emits the rows so far without resetting.
//! If ticks resume inside the same bucket, the next emit is a superset of the flushed row
//! and replaces it through the store's upsert.
//!
//! [`TierAcc::reset`] drops the open bucket without emitting it, for a wall-clock step
//! (the engine flushes first, then resets, D-064).
//!
//! [`Rollups`] holds both tiers and the rows they emitted in the last
//! [`RECENT_ROWS_MS`]. It lives in the host's [`crate::LiveHub`], so `query_history`
//! reads the rows the writer has not committed yet and the open buckets as they are
//! (D-092).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use kelvo_schema::{HostId, SeriesKey, Tier};
use kelvo_store::BucketRow;

/// How long [`Rollups`] keeps the rows it emitted: three of the writer's commit intervals
/// (`kelvo_store::DEFAULT_COMMIT_INTERVAL`, 5 minutes), so a row is still here well after
/// the commit that makes it readable from the store. With history unavailable this is
/// also all `query_history` can answer from.
pub const RECENT_ROWS_MS: i64 = 15 * 60_000;

struct LayoutAcc {
    series: Arc<[SeriesKey]>,
    min: Vec<f32>,
    max: Vec<f32>,
    /// Sum of value times weight.
    sum: Vec<f64>,
    /// Sum of weights.
    weight: Vec<f64>,
    count: Vec<u32>,
}

impl LayoutAcc {
    fn new(series: Arc<[SeriesKey]>) -> Self {
        let n = series.len();
        Self {
            series,
            min: vec![f32::INFINITY; n],
            max: vec![f32::NEG_INFINITY; n],
            sum: vec![0.0; n],
            weight: vec![0.0; n],
            count: vec![0; n],
        }
    }

    /// `values` are `(value, weight)`; a weight that is not positive counts as 1.
    fn add(&mut self, values: impl Iterator<Item = (f32, f64)>) {
        for (i, (v, w)) in values.enumerate() {
            if !v.is_finite() {
                continue;
            }
            let w = if w > 0.0 { w } else { 1.0 };
            if let (Some(mn), Some(mx), Some(s), Some(sw), Some(c)) = (
                self.min.get_mut(i),
                self.max.get_mut(i),
                self.sum.get_mut(i),
                self.weight.get_mut(i),
                self.count.get_mut(i),
            ) {
                *mn = mn.min(v);
                *mx = mx.max(v);
                *s += f64::from(v) * w;
                *sw += w;
                *c += 1;
            }
        }
    }

    fn row(&self, host: HostId, tier: Tier, bucket_ts: i64) -> Option<BucketRow> {
        if self.count.iter().all(|&c| c == 0) {
            return None;
        }
        let mut stats = Vec::with_capacity(self.series.len() * 3);
        for i in 0..self.series.len() {
            let c = self.count.get(i).copied().unwrap_or(0);
            if c == 0 {
                stats.extend([f32::NAN; 3]);
            } else {
                let mn = self.min.get(i).copied().unwrap_or(f32::NAN);
                let mx = self.max.get(i).copied().unwrap_or(f32::NAN);
                let s = self.sum.get(i).copied().unwrap_or(f64::NAN);
                let w = self.weight.get(i).copied().unwrap_or(f64::NAN);
                stats.extend([mn, mx, (s / w) as f32]);
            }
        }
        Some(BucketRow {
            host,
            tier,
            bucket_ts,
            series: Arc::clone(&self.series),
            stats,
        })
    }
}

pub(crate) struct TierAcc {
    tier: Tier,
    bucket: Option<i64>,
    layouts: Vec<LayoutAcc>,
    current: usize,
}

impl TierAcc {
    pub(crate) fn new(tier: Tier) -> Self {
        Self {
            tier,
            bucket: None,
            layouts: Vec::new(),
            current: 0,
        }
    }

    /// Adds one tick of persisted values with their weights (`series` order) at `ts_ms`.
    /// Rows for a bucket that just closed are appended to `out`. Returns whether a bucket
    /// closed.
    pub(crate) fn add(
        &mut self,
        host: HostId,
        ts_ms: i64,
        series: &Arc<[SeriesKey]>,
        values: impl Iterator<Item = (f32, f64)>,
        out: &mut Vec<BucketRow>,
    ) -> bool {
        // Tier::Unknown has no buckets and is never constructed here; skip it rather
        // than guess a width.
        let Some(bucket) = self.tier.bucket_start(ts_ms) else {
            return false;
        };
        let mut closed = false;
        if self.bucket != Some(bucket) {
            if let Some(old) = self.bucket {
                self.emit(host, old, out);
                closed = true;
            }
            self.layouts.clear();
            self.bucket = Some(bucket);
        }
        let idx = match self.layouts.get(self.current) {
            Some(acc) if Arc::ptr_eq(&acc.series, series) => self.current,
            _ => match self
                .layouts
                .iter()
                .position(|a| Arc::ptr_eq(&a.series, series) || a.series == *series)
            {
                Some(i) => i,
                None => {
                    self.layouts.push(LayoutAcc::new(Arc::clone(series)));
                    self.layouts.len() - 1
                }
            },
        };
        self.current = idx;
        if let Some(acc) = self.layouts.get_mut(idx) {
            acc.add(values);
        }
        closed
    }

    /// Forgets the open bucket and every layout's partial state. The engine flushes
    /// first; the next tick starts a new bucket wherever the wall clock now is.
    pub(crate) fn reset(&mut self) {
        self.bucket = None;
        self.layouts.clear();
        self.current = 0;
    }

    /// End of the open bucket: rows have been (or will be) written up to here.
    pub(crate) fn bucket_end(&self) -> Option<i64> {
        self.bucket.and_then(|b| self.tier.bucket_end(b))
    }

    /// Emits the rows of the open bucket without resetting it.
    pub(crate) fn flush(&self, host: HostId, out: &mut Vec<BucketRow>) {
        if let Some(b) = self.bucket {
            self.emit(host, b, out);
        }
    }

    fn emit(&self, host: HostId, bucket: i64, out: &mut Vec<BucketRow>) {
        out.extend(
            self.layouts
                .iter()
                .filter_map(|a| a.row(host, self.tier, bucket)),
        );
    }
}

/// Both persisted tiers' accumulators, the rows they emitted in the last
/// [`RECENT_ROWS_MS`], and each series' slowest sampling period: what `query_history`
/// needs to answer through now (D-092). The engine adds to it on every persisted tick;
/// readers take shared copies of the rows under the hub's lock and clone them after.
pub(crate) struct Rollups {
    s10: TierAcc,
    m1: TierAcc,
    /// Emitted rows, in the order they were emitted. A row emitted again (a flushed
    /// bucket that kept going) replaces its earlier copy.
    recent: VecDeque<Arc<BucketRow>>,
    /// Per persisted series, ms: the slowest period it is sampled at under the current
    /// settings, and the slowest of those this session; see
    /// [`Rollups::history_period_ms`].
    periods: HashMap<SeriesKey, Periods>,
    /// The slowest base tick under the current settings, for a series not in `periods`.
    floor_ms: u32,
    /// The slowest `floor_ms` this session.
    slowest_floor_ms: u32,
    /// When a period last got faster this session: samples before then may be further
    /// apart than the current periods say.
    faster_at: Option<i64>,
}

#[derive(Clone, Copy)]
struct Periods {
    current: u32,
    slowest: u32,
}

impl Default for Rollups {
    fn default() -> Self {
        Self {
            s10: TierAcc::new(Tier::S10),
            m1: TierAcc::new(Tier::M1),
            recent: VecDeque::new(),
            periods: HashMap::new(),
            floor_ms: 0,
            slowest_floor_ms: 0,
            faster_at: None,
        }
    }
}

impl Rollups {
    /// [`TierAcc::add`] on both tiers. Rows of closed buckets are appended to `out` and
    /// kept. Returns whether a minute closed.
    pub(crate) fn add(
        &mut self,
        host: HostId,
        ts_ms: i64,
        series: &Arc<[SeriesKey]>,
        values: impl Iterator<Item = (f32, f64)> + Clone,
        out: &mut Vec<BucketRow>,
    ) -> bool {
        let start = out.len();
        self.s10.add(host, ts_ms, series, values.clone(), out);
        let minute = self.m1.add(host, ts_ms, series, values, out);
        self.keep(out.get(start..).unwrap_or_default());
        minute
    }

    /// [`TierAcc::flush`] on both tiers; the rows are kept too.
    pub(crate) fn flush(&mut self, host: HostId, out: &mut Vec<BucketRow>) {
        let start = out.len();
        self.s10.flush(host, out);
        self.m1.flush(host, out);
        self.keep(out.get(start..).unwrap_or_default());
    }

    /// The end of the later open bucket.
    pub(crate) fn bucket_end(&self) -> Option<i64> {
        self.m1.bucket_end().max(self.s10.bucket_end())
    }

    /// [`TierAcc::reset`] on both tiers. Kept rows stay.
    pub(crate) fn reset(&mut self) {
        self.s10.reset();
        self.m1.reset();
    }

    /// Forgets kept rows of buckets ending after `ts_ms`: the wall clock stepped back.
    pub(crate) fn drop_after(&mut self, ts_ms: i64) {
        self.recent
            .retain(|r| r.bucket_ts + r.tier.bucket_ms().unwrap_or(0) <= ts_ms);
    }

    /// Forgets every kept row: another store, so history was reset.
    pub(crate) fn clear(&mut self) {
        self.recent.clear();
    }

    /// Forgets every kept row and the open buckets: the host's history was cleared, and
    /// what was measured before must not come back with the next emitted row.
    pub(crate) fn forget(&mut self) {
        self.reset();
        self.clear();
    }

    fn keep(&mut self, rows: &[BucketRow]) {
        for row in rows {
            let same = self.recent.iter().rposition(|r| {
                r.tier == row.tier
                    && r.bucket_ts == row.bucket_ts
                    && (Arc::ptr_eq(&r.series, &row.series) || r.series == row.series)
            });
            match same.and_then(|i| self.recent.get_mut(i)) {
                // In place unless a reader still holds the old copy.
                Some(r) => Arc::make_mut(r).clone_from(row),
                None => self.recent.push_back(Arc::new(row.clone())),
            }
        }
        if let Some(newest) = rows.iter().map(|r| r.bucket_ts).max() {
            // Not only from the front: a minute row is emitted after its last 10 s row.
            self.recent
                .retain(|r| r.bucket_ts >= newest - RECENT_ROWS_MS);
        }
    }

    /// Kept rows of buckets starting in `[from_ms, to_ms)` as shared copies, then the
    /// open buckets as they are, all stamped `host`. Cheap enough to call under the hub's
    /// lock; [`recent_rows`] turns them into rows after it is released.
    pub(crate) fn recent(&self, host: HostId, from_ms: i64, to_ms: i64) -> RecentRows {
        let in_range = |r: &BucketRow| r.bucket_ts >= from_ms && r.bucket_ts < to_ms;
        let mut open = Vec::new();
        self.s10.flush(host, &mut open);
        self.m1.flush(host, &mut open);
        open.retain(in_range);
        // An open bucket replaces its kept copy flushed earlier (a sleep, a pause).
        let kept = self
            .recent
            .iter()
            .filter(|r| in_range(r))
            .filter(|r| {
                !open
                    .iter()
                    .any(|o| o.tier == r.tier && o.bucket_ts == r.bucket_ts && o.series == r.series)
            })
            .cloned()
            .collect();
        RecentRows { kept, open }
    }

    /// Records each persisted series' slowest period under the current settings, and the
    /// slowest base tick for series the engine has not laid out this session, as of
    /// `now_ms`. A period that got faster marks `now_ms`: see
    /// [`Rollups::history_period_ms`].
    pub(crate) fn set_periods(
        &mut self,
        periods: impl Iterator<Item = (SeriesKey, u32)>,
        floor_ms: u32,
        now_ms: i64,
    ) {
        let mut faster = self.slowest_floor_ms > 0 && floor_ms < self.floor_ms;
        for (key, p) in periods {
            match self.periods.get_mut(&key) {
                Some(e) => {
                    faster |= p < e.current;
                    e.current = p;
                    e.slowest = e.slowest.max(p);
                }
                None => {
                    self.periods.insert(
                        key,
                        Periods {
                            current: p,
                            slowest: p,
                        },
                    );
                }
            }
        }
        self.floor_ms = floor_ms;
        self.slowest_floor_ms = self.slowest_floor_ms.max(floor_ms);
        if faster {
            self.faster_at = Some(now_ms);
        }
    }

    /// How far apart samples of `key` may be in history from `from_ms` on, at least
    /// `catalog_ms` (its catalog period). From the last time a period got faster this
    /// session, the slowest period under the current settings; for a range starting
    /// before it, the slowest period under any settings this session, since its earlier
    /// samples were taken under those. Settings persist across launches, so earlier
    /// sessions count as running under the settings this one started with.
    pub(crate) fn history_period_ms(&self, key: &SeriesKey, catalog_ms: u32, from_ms: i64) -> u32 {
        let p = self.periods.get(key).copied().unwrap_or(Periods {
            current: self.floor_ms,
            slowest: self.slowest_floor_ms,
        });
        let before_change = self.faster_at.is_some_and(|at| from_ms < at);
        if before_change { p.slowest } else { p.current }.max(catalog_ms)
    }
}

/// What [`Rollups::recent`] copies out under the lock.
pub(crate) struct RecentRows {
    kept: Vec<Arc<BucketRow>>,
    open: Vec<BucketRow>,
}

/// `rows` as owned rows, kept ones first, cloned outside the hub's lock.
pub(crate) fn recent_rows(rows: RecentRows) -> Vec<BucketRow> {
    let mut out: Vec<BucketRow> = Vec::with_capacity(rows.kept.len() + rows.open.len());
    out.extend(rows.kept.iter().map(|r| BucketRow::clone(r)));
    out.extend(rows.open);
    out
}

#[cfg(test)]
mod tests {
    use kelvo_schema::MetricId;
    use uuid::Uuid;

    use super::*;

    fn keys(ids: &[&'static str]) -> Arc<[SeriesKey]> {
        ids.iter()
            .map(|id| SeriesKey::bare(MetricId::from_static(id)))
            .collect()
    }

    const T0: i64 = 1_791_158_400_000; // a minute boundary

    /// Values of equal weight: a gauge's plain mean.
    fn w1<const N: usize>(v: [f32; N]) -> impl Iterator<Item = (f32, f64)> + Clone {
        v.into_iter().map(|v| (v, 1.0))
    }

    #[test]
    fn closes_on_wall_clock_boundary_and_skips_nan() {
        let host = HostId(Uuid::nil());
        let s = keys(&["a", "b"]);
        let mut acc = TierAcc::new(Tier::S10);
        let mut out = Vec::new();
        // 10 ticks starting 500 ms into the bucket; `b` only on even ticks, one NaN `a`.
        for i in 0..10 {
            let ts = T0 + 500 + i * 1_000;
            let a = if i == 3 { f32::NAN } else { i as f32 };
            let b = if i % 2 == 0 { 10.0 } else { f32::NAN };
            acc.add(host, ts, &s, w1([a, b]), &mut out);
        }
        assert!(out.is_empty(), "bucket [T0, T0+10s) still open at T0+9.5s");
        acc.add(host, T0 + 10_000, &s, w1([1.0, 1.0]), &mut out);
        assert_eq!(out.len(), 1);
        let row = &out[0];
        assert_eq!(row.bucket_ts, T0);
        // a: 0..=9 without 3 -> min 0, max 9, avg 42/9
        assert_eq!(row.stats[0], 0.0);
        assert_eq!(row.stats[1], 9.0);
        assert!((row.stats[2] - 42.0 / 9.0).abs() < 1e-5);
        assert_eq!(&row.stats[3..6], &[10.0, 10.0, 10.0]);
    }

    #[test]
    fn unsampled_series_is_nan_triple() {
        let host = HostId(Uuid::nil());
        let s = keys(&["a", "b"]);
        let mut acc = TierAcc::new(Tier::M1);
        let mut out = Vec::new();
        acc.add(host, T0, &s, w1([1.0, f32::NAN]), &mut out);
        acc.flush(host, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].stats[3..6].iter().all(|v| v.is_nan()));
    }

    #[test]
    fn layout_change_and_return_within_a_bucket() {
        let host = HostId(Uuid::nil());
        let a = keys(&["x"]);
        let b = keys(&["x", "y"]);
        let a_again = keys(&["x"]); // same content, new Arc
        let mut acc = TierAcc::new(Tier::S10);
        let mut out = Vec::new();
        acc.add(host, T0, &a, w1([1.0]), &mut out);
        acc.add(host, T0 + 1_000, &b, w1([2.0, 5.0]), &mut out);
        acc.add(host, T0 + 2_000, &a_again, w1([3.0]), &mut out);
        acc.add(host, T0 + 10_000, &a_again, w1([9.0]), &mut out);
        assert_eq!(out.len(), 2, "one row per layout in the closed bucket");
        assert_eq!(out[0].series.len(), 1);
        assert_eq!(&out[0].stats, &[1.0, 3.0, 2.0], "resumed, not overwritten");
        assert_eq!(&out[1].stats, &[2.0, 2.0, 2.0, 5.0, 5.0, 5.0]);
    }

    #[test]
    fn reset_forgets_the_open_bucket() {
        let host = HostId(Uuid::nil());
        let s = keys(&["a"]);
        let mut acc = TierAcc::new(Tier::M1);
        let mut out = Vec::new();
        acc.add(host, T0 + 5_000, &s, w1([1.0]), &mut out);
        assert_eq!(acc.bucket_end(), Some(T0 + 60_000));
        acc.reset();
        assert_eq!(acc.bucket_end(), None);
        // A tick in an earlier minute after the reset closes nothing.
        assert!(!acc.add(host, T0 - 30_000, &s, w1([2.0]), &mut out));
        acc.flush(host, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bucket_ts, T0 - 60_000);
        assert_eq!(&out[0].stats, &[2.0, 2.0, 2.0]);
    }

    #[test]
    fn the_average_weighs_each_value_by_its_span() {
        let host = HostId(Uuid::nil());
        let s = keys(&["a"]);
        let mut acc = TierAcc::new(Tier::M1);
        let mut out = Vec::new();
        // 10 over 9 s, then 0 over 1 s: 9, where a plain mean says 5.
        acc.add(host, T0, &s, [(10.0, 9_000.0)].into_iter(), &mut out);
        acc.add(host, T0 + 1_000, &s, [(0.0, 1_000.0)].into_iter(), &mut out);
        acc.flush(host, &mut out);
        assert_eq!(&out[0].stats, &[0.0, 10.0, 9.0]);
    }

    #[test]
    fn rollups_keep_emitted_rows_and_serve_the_open_bucket() {
        let host = HostId(Uuid::nil());
        let s = keys(&["a"]);
        let mut r = Rollups::default();
        let mut out = Vec::new();
        for i in 0..15 {
            r.add(host, T0 + i * 1_000, &s, w1([i as f32]), &mut out);
        }
        assert_eq!(out.len(), 1, "the first 10 s bucket closed");
        // A sleep flushes the open buckets; they keep going after the wake.
        r.flush(host, &mut out);
        r.add(host, T0 + 15_000, &s, w1([15.0]), &mut out);
        let rows = recent_rows(r.recent(host, T0, T0 + 60_000));
        let shape: Vec<_> = rows.iter().map(|r| (r.tier, r.bucket_ts)).collect();
        assert_eq!(
            shape,
            [(Tier::S10, T0), (Tier::S10, T0 + 10_000), (Tier::M1, T0)],
            "each bucket once: the open ones replace their flushed copies"
        );
        assert_eq!(&rows[1].stats, &[10.0, 15.0, 12.5]);
        r.drop_after(T0 + 10_000);
        assert_eq!(r.recent.len(), 1, "only the closed bucket ended by then");
        r.clear();
        assert!(r.recent.is_empty());
    }

    #[test]
    fn rows_are_kept_for_three_commits() {
        let commit = i64::try_from(kelvo_store::DEFAULT_COMMIT_INTERVAL.as_millis()).unwrap();
        assert!(RECENT_ROWS_MS >= 3 * commit);
        let host = HostId(Uuid::nil());
        let s = keys(&["a"]);
        let mut r = Rollups::default();
        let mut out = Vec::new();
        for i in 0..=(RECENT_ROWS_MS + 60_000) / 10_000 {
            r.add(host, T0 + i * 10_000, &s, w1([1.0]), &mut out);
        }
        let oldest = r.recent.iter().map(|r| r.bucket_ts).min().unwrap();
        let newest = r.recent.iter().map(|r| r.bucket_ts).max().unwrap();
        assert_eq!(newest - oldest, RECENT_ROWS_MS, "trimmed to the window");
    }

    #[test]
    fn forget_drops_the_open_buckets_too() {
        let host = HostId(Uuid::nil());
        let s = keys(&["a"]);
        let mut r = Rollups::default();
        let mut out = Vec::new();
        for i in 0..15 {
            r.add(host, T0 + i * 1_000, &s, w1([5.0]), &mut out);
        }
        r.forget();
        assert!(recent_rows(r.recent(host, T0, T0 + 60_000)).is_empty());
        // The next value starts the bucket over: nothing from before comes back.
        r.add(host, T0 + 15_000, &s, w1([1.0]), &mut out);
        let rows = recent_rows(r.recent(host, T0, T0 + 60_000));
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|r| r.stats == [1.0, 1.0, 1.0]), "{rows:?}");
    }

    #[test]
    fn a_range_before_a_period_got_faster_takes_the_slowest() {
        let a = SeriesKey::bare(MetricId::from_static("a"));
        let unseen = SeriesKey::bare(MetricId::from_static("b"));
        let mut r = Rollups::default();
        r.set_periods([(a.clone(), 20_000)].into_iter(), 20_000, T0);
        assert_eq!(r.history_period_ms(&a, 0, T0 - 1), 20_000);
        // Slower: earlier samples were closer together, so the current period covers them.
        r.set_periods([(a.clone(), 40_000)].into_iter(), 40_000, T0 + 1_000);
        assert_eq!(r.history_period_ms(&a, 0, T0 - 1), 40_000);
        // Faster: a range reaching before the change keeps the slowest.
        r.set_periods([(a.clone(), 2_000)].into_iter(), 2_000, T0 + 2_000);
        assert_eq!(r.history_period_ms(&a, 0, T0 + 2_000), 2_000);
        assert_eq!(r.history_period_ms(&a, 0, T0 + 1_999), 40_000);
        assert_eq!(r.history_period_ms(&unseen, 0, T0 + 1_999), 40_000);
        assert_eq!(r.history_period_ms(&unseen, 0, T0 + 2_000), 2_000);
        assert_eq!(
            r.history_period_ms(&a, 60_000, T0 + 2_000),
            60_000,
            "catalog"
        );
    }
}

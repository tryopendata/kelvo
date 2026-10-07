//! The one-hour ring buffer of raw frames, used to backfill a window's charts when it
//! opens (architecture.md, Engine and Data flow). [`crate::LiveHub`] owns one per host
//! and fills it from whatever source publishes for that host.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::bus::FrameLayout;

/// How much the ring keeps.
pub const RING_SPAN_MS: i64 = 3_600_000;

/// Upper bound on rows whatever the timestamps say (an hour at the fastest 0.5 s tick,
/// plus slack), so a wall clock stepping backwards cannot grow the ring without bound.
pub const RING_MAX_ROWS: usize = 7_300;

struct Row {
    ts_ms: i64,
    interval_ms: u32,
    timeline: u32,
    layout: Arc<FrameLayout>,
    values: Arc<[f32]>,
    holds: Arc<[u32]>,
}

/// A run of evenly spaced rows that share a layout and a timeline: row `i` was taken at
/// about `start_ms + i * interval_ms`. A layout change, an interval change, a new
/// timeline, a change in how long samples stay current, or a hole (sleep, pause, skipped
/// ticks) starts a new segment, so a reader never has to guess spacing and never draws
/// across a hole.
#[derive(Clone, Debug)]
pub struct BackfillSegment {
    pub layout: Arc<FrameLayout>,
    pub start_ms: i64,
    pub interval_ms: u32,
    /// The source's clock timeline the rows are on (`LiveFrame::timeline`).
    pub timeline: u32,
    /// Raw values per row, `NaN` where a series was not sampled.
    pub rows: Vec<Arc<[f32]>>,
    /// `LiveFrame::holds` of every row in the segment.
    pub holds: Arc<[u32]>,
}

#[derive(Default)]
pub struct Ring {
    rows: VecDeque<Row>,
}

impl Ring {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Drops every row.
    pub fn clear(&mut self) {
        self.rows.clear();
    }

    /// Timestamp of the newest row.
    pub fn last_ts(&self) -> Option<i64> {
        self.rows.back().map(|r| r.ts_ms)
    }

    /// Appends a row. A row not after the newest one means the source's wall clock
    /// stepped back (D-064): the ring must stay in time order, so the rows at or after
    /// its time go. The older ones stay: a one-second step costs a second of history,
    /// not the hour.
    pub fn push(
        &mut self,
        ts_ms: i64,
        interval_ms: u32,
        timeline: u32,
        layout: Arc<FrameLayout>,
        values: Arc<[f32]>,
        holds: Arc<[u32]>,
    ) {
        while self.rows.back().is_some_and(|r| r.ts_ms >= ts_ms) {
            self.rows.pop_back();
        }
        self.rows.push_back(Row {
            ts_ms,
            interval_ms,
            timeline,
            layout,
            values,
            holds,
        });
        let cutoff = ts_ms - RING_SPAN_MS;
        while self
            .rows
            .front()
            .is_some_and(|r| r.ts_ms < cutoff || self.rows.len() > RING_MAX_ROWS)
        {
            self.rows.pop_front();
        }
    }

    /// Rows with `ts_ms >= since_ms`, as evenly spaced segments in time order. Each
    /// segment is placed from its newest row, so that row lands exactly where the next
    /// frame follows it, and a first row off the ticker's grid (the sample a window's
    /// opening takes between ticks, D-094) does not shift every row after it.
    pub fn backfill(&self, since_ms: i64) -> Vec<BackfillSegment> {
        let mut out: Vec<BackfillSegment> = Vec::new();
        let mut newest: Vec<i64> = Vec::new();
        for row in self.rows.iter().filter(|r| r.ts_ms >= since_ms) {
            let continues = out.last().is_some_and(|seg| {
                let expected = seg.start_ms
                    + i64::try_from(seg.rows.len()).unwrap_or(i64::MAX)
                        * i64::from(seg.interval_ms);
                Arc::ptr_eq(&seg.layout, &row.layout)
                    && seg.interval_ms == row.interval_ms
                    && seg.timeline == row.timeline
                    && (Arc::ptr_eq(&seg.holds, &row.holds) || seg.holds == row.holds)
                    && (row.ts_ms - expected).abs() <= i64::from(row.interval_ms) / 2
            });
            match (out.last_mut(), newest.last_mut()) {
                (Some(seg), Some(last)) if continues => {
                    seg.rows.push(Arc::clone(&row.values));
                    *last = row.ts_ms;
                }
                _ => {
                    out.push(BackfillSegment {
                        layout: Arc::clone(&row.layout),
                        start_ms: row.ts_ms,
                        interval_ms: row.interval_ms,
                        timeline: row.timeline,
                        rows: vec![Arc::clone(&row.values)],
                        holds: Arc::clone(&row.holds),
                    });
                    newest.push(row.ts_ms);
                }
            }
        }
        for (seg, last) in out.iter_mut().zip(newest) {
            let n = i64::try_from(seg.rows.len()).unwrap_or(i64::MAX) - 1;
            seg.start_ms = last - n * i64::from(seg.interval_ms);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use kelvo_schema::{MetricId, SeriesKey};

    use super::*;

    fn layout(no: u32, ids: &[&'static str]) -> Arc<FrameLayout> {
        Arc::new(FrameLayout {
            layout_no: no,
            series: ids
                .iter()
                .map(|id| SeriesKey::bare(MetricId::from_static(id)))
                .collect(),
        })
    }

    fn vals(v: &[f32]) -> Arc<[f32]> {
        v.into()
    }

    /// One series' hold; a fresh `Arc` each call, so segments compare holds by value.
    fn h(ms: u32) -> Arc<[u32]> {
        vec![ms].into()
    }

    #[test]
    fn segments_split_where_holds_change() {
        let a = layout(1, &["power.gpu"]);
        let mut ring = Ring::new();
        // Tray only: power.gpu sampled every 10 s, then a window opens and it is sampled
        // every tick.
        for ts in [0, 1_000, 2_000] {
            ring.push(ts, 1_000, 0, Arc::clone(&a), vals(&[1.0]), h(25_000));
        }
        for ts in [3_000, 4_000] {
            ring.push(ts, 1_000, 0, Arc::clone(&a), vals(&[1.0]), h(2_500));
        }
        let segs = ring.backfill(0);
        let shape: Vec<_> = segs
            .iter()
            .map(|s| (s.start_ms, s.rows.len(), s.holds[0]))
            .collect();
        assert_eq!(shape, vec![(0, 3, 25_000), (3_000, 2, 2_500)]);
    }

    #[test]
    fn segments_split_on_layout_interval_and_holes() {
        let a = layout(1, &["cpu.total"]);
        let b = layout(2, &["cpu.total", "gpu.util"]);
        let mut ring = Ring::new();
        // Three ticks at 1 s with small jitter, a 30 s hole, two more, then a new layout,
        // then the interval moves to 2 s.
        for ts in [0, 1_040, 1_990] {
            ring.push(ts, 1_000, 0, Arc::clone(&a), vals(&[1.0]), h(2_500));
        }
        for ts in [32_000, 33_000] {
            ring.push(ts, 1_000, 0, Arc::clone(&a), vals(&[2.0]), h(2_500));
        }
        ring.push(
            34_000,
            1_000,
            0,
            Arc::clone(&b),
            vals(&[3.0, 4.0]),
            h(2_500),
        );
        ring.push(
            36_000,
            2_000,
            0,
            Arc::clone(&b),
            vals(&[5.0, f32::NAN]),
            h(2_500),
        );
        ring.push(
            38_000,
            2_000,
            0,
            Arc::clone(&b),
            vals(&[6.0, 7.0]),
            h(2_500),
        );

        let segs = ring.backfill(0);
        let shape: Vec<_> = segs
            .iter()
            .map(|s| (s.layout.layout_no, s.start_ms, s.interval_ms, s.rows.len()))
            .collect();
        assert_eq!(
            shape,
            vec![
                // Placed from its newest row, 1_990.
                (1, -10, 1_000, 3),
                (1, 32_000, 1_000, 2),
                (2, 34_000, 1_000, 1),
                (2, 36_000, 2_000, 2),
            ]
        );
        assert!(segs[3].rows[0][1].is_nan(), "raw NaN kept");

        let tail = ring.backfill(33_000);
        assert_eq!(tail.first().map(|s| s.start_ms), Some(33_000));
    }

    #[test]
    fn a_first_row_off_the_grid_does_not_shift_the_rest() {
        let a = layout(1, &["cpu.total"]);
        let mut ring = Ring::new();
        // A window opened 400 ms after a boundary and the engine sampled at once; the
        // ticker's rows follow on the grid.
        for ts in [10_400, 11_000, 12_000, 13_000] {
            ring.push(ts, 1_000, 0, Arc::clone(&a), vals(&[1.0]), h(2_500));
        }
        let segs = ring.backfill(0);
        let shape: Vec<_> = segs.iter().map(|s| (s.start_ms, s.rows.len())).collect();
        assert_eq!(shape, vec![(10_000, 4)]);
    }

    #[test]
    fn keeps_one_hour() {
        let a = layout(1, &["cpu.total"]);
        let mut ring = Ring::new();
        for i in 0..4_000_i64 {
            ring.push(i * 1_000, 1_000, 0, Arc::clone(&a), vals(&[0.0]), h(2_500));
        }
        let segs = ring.backfill(i64::MIN);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].start_ms, (3_999 - 3_600) * 1_000);
        assert_eq!(ring.len(), 3_601);
    }

    #[test]
    fn a_step_back_past_everything_starts_over() {
        let a = layout(1, &["cpu.total"]);
        let mut ring = Ring::new();
        for ts in [10_000, 11_000, 12_000] {
            ring.push(ts, 1_000, 0, Arc::clone(&a), vals(&[1.0]), h(2_500));
        }
        ring.push(2_000, 1_000, 1, Arc::clone(&a), vals(&[2.0]), h(2_500));
        assert_eq!(ring.len(), 1);
        assert_eq!(ring.backfill(i64::MIN)[0].start_ms, 2_000);
    }

    /// #10: at 0.5 s a 1.1 s step back used to wipe the hour. Only the rows at or after
    /// the stepped time go, and the new timeline is its own segment.
    #[test]
    fn a_small_step_back_drops_only_the_overlap() {
        let a = layout(1, &["cpu.total"]);
        let mut ring = Ring::new();
        for i in 0..7_200_i64 {
            ring.push(i * 500, 500, 0, Arc::clone(&a), vals(&[1.0]), h(2_500));
        }
        let last = 7_199 * 500;
        // Next tick due at last + 500; the clock went back 1.1 s.
        ring.push(
            last + 500 - 1_100,
            500,
            1,
            Arc::clone(&a),
            vals(&[2.0]),
            h(2_500),
        );
        assert_eq!(
            ring.len(),
            7_200 - 2 + 1,
            "two rows overlapped the new time"
        );
        let segs = ring.backfill(i64::MIN);
        let shape: Vec<_> = segs
            .iter()
            .map(|s| (s.start_ms, s.timeline, s.rows.len()))
            .collect();
        assert_eq!(shape, [(0, 0, 7_198), (last - 600, 1, 1)]);
    }
}

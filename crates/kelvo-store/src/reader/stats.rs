//! `query_series_stats`: what a history read of unlabelled metrics measured over a range.
//! A bucket counts the time it covers, cut at now, less any gap inside it that applies to
//! the metric's module; a bucket with no reading counts nothing.

use kelvo_schema::{MetricDef, ceil_to, floor_to};

use crate::types::HistoryResult;

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

/// [`range_stats`]: the range widened to whole buckets, as the read took them, and cut at
/// now, with one entry per metric asked for, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeStats {
    pub from_ms: i64,
    pub to_ms: i64,
    pub metrics: Vec<MetricStats>,
}

/// Sums a read of `defs` over `[from_ms, to_ms)`. A bucket is measured where the metric
/// has a reading, outside the gaps that apply to its module (host-wide ones and the
/// module's own); an open gap runs to `now_ms`.
pub fn range_stats(
    read: &HistoryResult,
    defs: &[&'static MetricDef],
    from_ms: i64,
    to_ms: i64,
    now_ms: i64,
) -> RangeStats {
    let width = read.bucket_ms.max(1);
    let from = floor_to(from_ms, width);
    let to = ceil_to(to_ms, width).min(now_ms).max(from);
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

#[cfg(test)]
mod tests {
    use kelvo_schema::{Catalog, Gap, GapReason, Labels, MetricId, Module, SeriesKey, Tier};

    use super::*;
    use crate::types::{Point, SeriesPoints};

    const S: i64 = 1_000;
    const W: i64 = 10 * S;

    fn def(id: &str) -> &'static MetricDef {
        Catalog::builtin().get(id).expect("builtin metric")
    }

    /// One 10 s point per `(bucket, avg, max)`.
    fn series(metric: &'static str, points: &[(i64, f32, f32)]) -> SeriesPoints {
        SeriesPoints {
            key: SeriesKey::new(MetricId::from_static(metric), Labels::new()),
            points: points
                .iter()
                .map(|&(b, avg, max)| Point {
                    t: b * W,
                    min: avg,
                    max,
                    avg,
                })
                .collect(),
        }
    }

    fn read(series: Vec<SeriesPoints>, gaps: Vec<Gap>) -> HistoryResult {
        HistoryResult {
            tier: Tier::S10,
            bucket_ms: W,
            series,
            gaps,
        }
    }

    #[test]
    fn module_gaps_cut_only_their_module_and_the_mean_is_span_weighted() {
        let r = read(
            vec![
                series(
                    "cpu.total",
                    &[(0, 20.0, 30.0), (1, 20.0, 30.0), (2, 50.0, 90.0)],
                ),
                series(
                    "disk.read_total",
                    &[(0, 1_000.0, 1_000.0), (1, 1_000.0, 1_000.0)],
                ),
            ],
            vec![
                // Disk off over bucket 0; CPU still counts it.
                Gap::module_disabled(0, Some(W), Module::Disk),
                // CPU off over the second half of bucket 2: its 50% weighs half a bucket.
                Gap::module_disabled(2 * W + 5 * S, Some(3 * W), Module::Cpu),
            ],
        );
        let s = range_stats(
            &r,
            &[def("cpu.total"), def("disk.read_total")],
            0,
            3 * W,
            100 * W,
        );
        assert_eq!((s.from_ms, s.to_ms), (0, 3 * W));
        let [cpu, disk] = s.metrics.as_slice() else {
            panic!("two metrics: {s:?}")
        };
        assert_eq!(cpu.metric, "cpu.total");
        assert_eq!(cpu.measured_ms, 25 * S);
        // (20 * 20 s + 50 * 5 s) / 25 s.
        assert_eq!(cpu.avg, Some(26.0));
        assert_eq!(cpu.max, Some(90.0));
        assert_eq!(cpu.integral, 26.0 * 25.0);
        assert_eq!(disk.measured_ms, 10 * S, "Disk's own gap");
        assert_eq!(disk.avg, Some(1_000.0));
        assert_eq!(disk.max, Some(1_000.0));
        assert_eq!(disk.integral, 10_000.0, "bytes: B/s times measured seconds");
    }

    #[test]
    fn an_empty_range_or_no_readings_measure_nothing() {
        let cpu = series("cpu.total", &[(0, 20.0, 30.0)]);
        // A range with no points in it.
        let s = range_stats(
            &read(vec![cpu.clone()], Vec::new()),
            &[def("cpu.total")],
            5 * W,
            8 * W,
            100 * W,
        );
        assert_eq!((s.from_ms, s.to_ms), (5 * W, 8 * W));
        let m = &s.metrics[0];
        assert_eq!(
            (m.measured_ms, m.avg, m.max, m.integral),
            (0, None, None, 0.0)
        );
        // A zero-width range.
        let s = range_stats(
            &read(vec![cpu], Vec::new()),
            &[def("cpu.total")],
            W,
            W,
            100 * W,
        );
        assert_eq!((s.from_ms, s.to_ms), (W, W));
        assert_eq!(s.metrics[0].measured_ms, 0);
        // A metric with no series at all still gets its entry.
        let s = range_stats(
            &read(Vec::new(), Vec::new()),
            &[def("disk.read_total")],
            0,
            W,
            100 * W,
        );
        assert_eq!(s.metrics.len(), 1);
        assert_eq!(s.metrics[0].avg, None);
    }

    #[test]
    fn gaps_at_the_edges_and_open_gaps_clip_to_the_range_and_now() {
        let points: Vec<_> = (0..6).map(|b| (b, 100.0, 100.0)).collect();
        let r = read(
            vec![series("net.rx_total", &points)],
            vec![
                // Starts before the range and runs into bucket 1.
                Gap::host(-5 * W, Some(W + 5 * S), GapReason::Sleep).expect("host gap"),
                // Still open: runs to now, past the end of the range.
                Gap::host(4 * W, None, GapReason::Sleep).expect("host gap"),
            ],
        );
        // Widened to whole buckets: [0, 5W).
        let s = range_stats(&r, &[def("net.rx_total")], 3 * S, 5 * W - S, 100 * W);
        assert_eq!((s.from_ms, s.to_ms), (0, 5 * W));
        let m = &s.metrics[0];
        // Buckets 0..5, less bucket 0, half of 1 and bucket 4.
        assert_eq!(m.measured_ms, 25 * S);
        assert_eq!(m.integral, 2_500.0);

        // Cut at now, halfway through bucket 3; the open gap has not started.
        let s = range_stats(&r, &[def("net.rx_total")], 0, 6 * W, 3 * W + 5 * S);
        assert_eq!(s.to_ms, 3 * W + 5 * S);
        assert_eq!(s.metrics[0].measured_ms, 20 * S);
    }
}

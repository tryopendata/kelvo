//! `query_series_stats`: what a history read of unlabelled metrics measured over a range.
//! A bucket counts the time it covers, cut at now, less any gap inside it that applies to
//! the metric's module; a bucket with no reading counts nothing.

use kelvo_schema::MetricDef;

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
    let from = from_ms - from_ms.rem_euclid(width);
    let to = (to_ms + (width - to_ms.rem_euclid(width)) % width)
        .min(now_ms)
        .max(from);
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

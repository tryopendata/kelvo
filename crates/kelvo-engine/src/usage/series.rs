//! Per-app use bucket by bucket over the same ring ([`UsageRing::series_by_app`]): the
//! apps a range ranks first, each as one rate per bucket, for lines under a host total.

use std::sync::Arc;

use super::{AppSlot, USAGE_BUCKET_MS, UsageKey, UsageRing};

/// One app's use bucket by bucket ([`UsageRing::series_by_app`]).
#[derive(Clone, Debug, PartialEq)]
pub struct UsageSeriesApp {
    pub name: Arc<str>,
    /// The key's rate over the whole range's covered time (see [`UsageSeries::apps`]).
    pub avg: f64,
    /// One per bucket from [`UsageSeries::from_ms`]: `None` where no process sample
    /// covered the bucket (unmeasured, not zero use); 0 where one did and the app used
    /// nothing.
    pub values: Vec<Option<f32>>,
}

/// Per-app use per bucket over a range ([`UsageRing::series_by_app`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageSeries {
    /// The first bucket's start: the request widened to whole buckets.
    pub from_ms: i64,
    pub bucket_ms: i64,
    /// As in [`super::UsageByApp`].
    pub since_ms: Option<i64>,
    pub complete_to_ms: Option<i64>,
    pub latest_ms: Option<i64>,
    /// The apps [`UsageRing::by_app`] ranks first, in its order. Values are rates over
    /// each bucket's covered time, by key: energy in watts, CPU in percent of one core,
    /// GPU in percent of the GPU over the time GPU was measured (`None` in a bucket
    /// where it was not), disk in bytes read plus written per second, memory as the
    /// average footprint in bytes (an app absent for part of the bucket counts 0 there,
    /// so it reads at most its peak, which is what ranks it).
    pub apps: Vec<UsageSeriesApp>,
}

impl UsageRing {
    /// Use per bucket over `[from_ms, to_ms)`, widened to whole buckets, of the apps
    /// [`Self::by_app`] ranks first by `by`, in its order. The caller bounds the range:
    /// it costs one value per bucket and app however little of it the ring holds.
    pub fn series_by_app(
        &self,
        from_ms: i64,
        to_ms: i64,
        by: UsageKey,
        limit: usize,
    ) -> UsageSeries {
        let ranked = self.by_app(from_ms, to_ms, by, limit);
        let (from, to) = (ranked.from_ms, ranked.to_ms);
        // Every ranked name is a live app's: `by_app` takes them from `self.apps`.
        let (ids, names): (Vec<u32>, Vec<Arc<str>>) = ranked
            .apps
            .into_iter()
            .filter_map(|a| Some((*self.app_ids.get(&a.name)?, a.name)))
            .unzip();
        let n = usize::try_from((to - from) / USAGE_BUCKET_MS).unwrap_or(0);
        let mut values = vec![vec![None; n]; ids.len()];
        let mut sums = vec![0.0_f64; ids.len()];
        let mut secs_total = 0.0_f64;
        for b in &self.buckets {
            if b.start < from || b.start >= to {
                continue;
            }
            let secs = f64::from(match by {
                UsageKey::Gpu => b.gpu_covered_ms,
                _ => b.covered_ms,
            }) / 1_000.0;
            let Ok(i) = usize::try_from((b.start - from) / USAGE_BUCKET_MS) else {
                continue;
            };
            if secs <= 0.0 {
                // Not measured: the gap stays `None`.
                continue;
            }
            secs_total += secs;
            for ((id, row), sum) in ids.iter().zip(&mut values).zip(&mut sums) {
                // An app the bucket does not hold was measured and used nothing.
                let used = b
                    .apps
                    .binary_search_by_key(id, |(k, _)| *k)
                    .ok()
                    .and_then(|j| b.apps.get(j))
                    .map_or(0.0, |(_, s)| amount(by, s));
                *sum += used;
                if let Some(v) = row.get_mut(i) {
                    *v = Some((used / secs) as f32);
                }
            }
        }
        let apps = names
            .into_iter()
            .zip(values.into_iter().zip(sums))
            .map(|(name, (values, sum))| UsageSeriesApp {
                name,
                avg: if secs_total > 0.0 {
                    sum / secs_total
                } else {
                    0.0
                },
                values,
            })
            .collect();
        UsageSeries {
            from_ms: from,
            bucket_ms: USAGE_BUCKET_MS,
            since_ms: ranked.since_ms,
            complete_to_ms: ranked.complete_to_ms,
            latest_ms: ranked.latest_ms,
            apps,
        }
    }
}

/// What app slot `s` used of `by` over its bucket. Over the bucket's covered seconds
/// (GPU-measured, for GPU) it is the series value.
fn amount(by: UsageKey, s: &AppSlot) -> f64 {
    let u = s.used.wide();
    match by {
        UsageKey::Cpu => u.cpu_s * 100.0,
        UsageKey::Gpu => u.gpu_s * 100.0,
        UsageKey::Memory => f64::from(s.mem_byte_s),
        UsageKey::Disk => u.read_b + u.write_b,
        UsageKey::Energy => u.energy_j,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::tests::{T0, busy, names, proc};

    #[test]
    fn series_leaves_an_unmeasured_bucket_empty_and_an_idle_app_at_zero() {
        let mut ring = UsageRing::default();
        ring.push(
            T0 + 10_000,
            &[
                proc(1, "a", Some("A"), true, 20.0),
                proc(2, "b", Some("B"), true, 10.0),
            ],
        );
        // Asleep through the next bucket; B is idle once the Mac wakes.
        ring.push(T0 + 20_000, &[]);
        ring.push(T0 + 30_000, &[proc(1, "a", Some("A"), true, 30.0)]);
        let s = ring.series_by_app(T0, T0 + 30_000, UsageKey::Energy, 5);
        assert_eq!((s.from_ms, s.bucket_ms), (T0, USAGE_BUCKET_MS));
        assert_eq!(s.apps[0].values, [Some(2.0), None, Some(3.0)]);
        assert_eq!(s.apps[1].values, [Some(1.0), None, Some(0.0)]);
        // The average is over the covered time only.
        assert!((s.apps[0].avg - 2.5).abs() < 1e-6);
        assert_eq!(s.complete_to_ms, Some(T0 + 30_000));
    }

    #[test]
    fn series_spreads_a_sample_over_the_buckets_it_covers() {
        let mut ring = UsageRing::default();
        // 10 J over [T0 + 5 s, T0 + 15 s): half in each bucket, 1 W in both.
        ring.push(T0 + 15_000, &[proc(1, "a", Some("A"), true, 10.0)]);
        let s = ring.series_by_app(T0, T0 + 20_000, UsageKey::Energy, 5);
        let w = &s.apps[0].values;
        assert_eq!(w.len(), 2);
        assert!(w.iter().all(|v| v.is_some_and(|v| (v - 1.0).abs() < 1e-6)));
        // CPU reads the same way: percent of one core over each bucket's covered time.
        let mut ring = UsageRing::default();
        ring.push(T0 + 15_000, &[busy(1, "A", 60.0, 10.0)]);
        let s = ring.series_by_app(T0, T0 + 20_000, UsageKey::Cpu, 5);
        assert!(
            s.apps[0]
                .values
                .iter()
                .all(|v| v.is_some_and(|v| (v - 60.0).abs() < 1e-3))
        );
    }

    #[test]
    fn series_ranks_like_by_app_and_keeps_the_limit() {
        let mut ring = UsageRing::default();
        for i in 1..=3 {
            ring.push(
                T0 + i * 10_000,
                &[
                    proc(1, "a", Some("A"), true, 1.0),
                    proc(2, "b", Some("B"), true, 5.0 * i as f32),
                    proc(3, "c", Some("C"), true, 3.0),
                    proc(4, "d", None, false, 2.0),
                ],
            );
        }
        let ranked = ring.by_app(T0, T0 + 30_000, UsageKey::Energy, 3);
        let s = ring.series_by_app(T0, T0 + 30_000, UsageKey::Energy, 3);
        let series: Vec<&str> = s.apps.iter().map(|a| a.name.as_ref()).collect();
        assert_eq!(series, names(&ranked));
        assert_eq!(series, ["B", "C", "d"]);
        for (a, r) in s.apps.iter().zip(&ranked.apps) {
            assert!((a.avg - r.avg_w).abs() < 1e-6, "{}", a.name);
        }
        // Before any batch: no apps, the buckets still laid out.
        let empty = UsageRing::default().series_by_app(T0, T0 + 30_000, UsageKey::Energy, 3);
        assert!(empty.apps.is_empty());
        assert_eq!(empty.since_ms, None);
    }
}

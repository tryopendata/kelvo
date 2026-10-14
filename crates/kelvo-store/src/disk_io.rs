//! Disk request-time views over integral-preserving series (D-110).
use crate::{HistoryResult, Point, SeriesPoints, range_stats};
use kelvo_schema::{
    Catalog, MetricId, Module, SeriesKey,
    disk_io::{DiskIoPage, DiskIoPoint, request_ms},
};
use std::collections::{BTreeMap, BTreeSet};

pub const DISK_IO_METRICS: [&str; 8] = [
    "disk.read",
    "disk.write",
    "disk.read_ops",
    "disk.write_ops",
    "disk.read_timed_ops",
    "disk.write_timed_ops",
    "disk.read_time",
    "disk.write_time",
];

/// Each paired average is normalized by its whole output slot. Summing numerators
/// and denominators therefore weights both devices and time by completed operations.
#[expect(
    clippy::too_many_lines,
    reason = "split when substantially changed; architecture refactor section 10"
)]
pub fn disk_io_page(
    read: HistoryResult,
    device: Option<&str>,
    from_ms: i64,
    to_ms: i64,
    now_ms: i64,
    max_points: u32,
) -> DiskIoPage {
    let mut devices = BTreeSet::new();
    let mut slots: BTreeMap<i64, BTreeMap<String, [Option<f64>; 8]>> = BTreeMap::new();
    for series in &read.series {
        let metrics = if device.is_some() {
            &DISK_IO_METRICS
        } else {
            &kelvo_schema::disk_io::TOTAL_METRICS
        };
        let Some(index) = metrics
            .iter()
            .position(|id| *id == series.key.metric.as_str())
        else {
            continue;
        };
        let dev = if device.is_some() {
            let Some(dev) = series.key.labels.get("dev") else {
                continue;
            };
            dev
        } else {
            ""
        };
        if device.is_some_and(|selected| selected != dev) {
            continue;
        }
        devices.insert(dev.to_owned());
        for point in &series.points {
            if point.avg.is_finite() && point.avg >= 0. {
                slots
                    .entry(point.t)
                    .or_default()
                    .entry(dev.into())
                    .or_insert([None; 8])[index] = Some(f64::from(point.avg));
            }
        }
    }
    // Never pair observations from different intervals after merging.
    for devices in slots.values_mut() {
        for values in devices.values_mut() {
            for (ops, time) in [(4, 6), (5, 7)] {
                if values[ops].is_none() || values[time].is_none() {
                    values[ops] = None;
                    values[time] = None;
                }
            }
        }
    }
    let points = project(&slots, &devices);
    let gaps: Vec<_> = read
        .gaps
        .into_iter()
        .filter(|g| g.module.is_none_or(|m| m == Module::Disk))
        .collect();
    let total_series =
        |metric: &'static str, value: fn(&DiskIoPoint) -> Option<f64>| SeriesPoints {
            key: SeriesKey::bare(MetricId::from_static(metric)),
            points: points
                .iter()
                .filter_map(|p| {
                    value(p).map(|v| Point {
                        t: p.t,
                        min: v as f32,
                        max: v as f32,
                        avg: v as f32,
                    })
                })
                .collect(),
        };
    let totals_read = HistoryResult {
        tier: read.tier,
        bucket_ms: read.bucket_ms,
        series: vec![
            total_series("disk.read_total", |p| p.read_bps),
            total_series("disk.write_total", |p| p.write_bps),
        ],
        gaps: gaps.clone(),
    };
    let catalog = Catalog::builtin();
    let defs: Vec<_> = ["disk.read_total", "disk.write_total"]
        .iter()
        .filter_map(|id| catalog.get(id))
        .collect();
    let totals = range_stats(&totals_read, &defs, from_ms, to_ms, now_ms);
    let read_total = totals.metrics.first();
    let write_total = totals.metrics.get(1);
    let base = kelvo_schema::floor_to(from_ms, read.bucket_ms);
    let buckets = (to_ms - base + read.bucket_ms - 1) / read.bucket_ms;
    let count = (buckets + i64::from(max_points.max(1)) - 1) / i64::from(max_points.max(1));
    let mut count = count.max(1);
    // Include the leading partial coarse bucket while keeping the public epoch grid
    // and strict output bound. A one-point query may span one wider aligned bucket.
    loop {
        let width = read.bucket_ms * count;
        let start = kelvo_schema::floor_to(from_ms, width);
        if (to_ms - start + width - 1) / width <= i64::from(max_points.max(1)) {
            break;
        }
        count *= 2;
    }
    let bucket_ms = read.bucket_ms * count;
    let points = if count > 1 {
        let mut merged: BTreeMap<i64, BTreeMap<String, [(f64, u32); 8]>> = BTreeMap::new();
        for (time, values) in slots {
            let target = kelvo_schema::floor_to(time, bucket_ms);
            for (dev, values) in values {
                let cell = merged
                    .entry(target)
                    .or_default()
                    .entry(dev)
                    .or_insert([(0., 0); 8]);
                for (i, value) in values.into_iter().enumerate() {
                    if let Some(value) = value {
                        cell[i].0 += value;
                        cell[i].1 += 1;
                    }
                }
            }
        }
        let slots = merged
            .into_iter()
            .map(|(t, rows)| {
                (
                    t,
                    rows.into_iter()
                        .map(|(dev, values)| {
                            (
                                dev,
                                std::array::from_fn(|i| {
                                    (values[i].1 > 0).then(|| {
                                        values[i].0
                                            / if i >= 4 {
                                                count as f64
                                            } else {
                                                f64::from(values[i].1)
                                            }
                                    })
                                }),
                            )
                        })
                        .collect(),
                )
            })
            .collect();
        project(&slots, &devices)
    } else {
        points
    };
    DiskIoPage {
        tier: read.tier,
        bucket_ms,
        from_ms: totals.from_ms,
        to_ms: totals.to_ms,
        points,
        gaps,
        read_bytes: read_total.and_then(|v| (v.measured_ms > 0).then_some(v.integral)),
        write_bytes: write_total.and_then(|v| (v.measured_ms > 0).then_some(v.integral)),
        read_covered_ms: read_total.map_or(0, |v| v.measured_ms),
        write_covered_ms: write_total.map_or(0, |v| v.measured_ms),
    }
}

type Slots = BTreeMap<i64, BTreeMap<String, [Option<f64>; 8]>>;
fn project(slots: &Slots, devices: &BTreeSet<String>) -> Vec<DiskIoPoint> {
    slots
        .iter()
        .map(|(t, values)| {
            // Missing participating devices stay unavailable rather than turning into
            // lower totals or averaging only the fastest still-reporting drive.
            let sum = |index: usize| {
                devices
                    .iter()
                    .try_fold(0., |total, dev| Some(total + values.get(dev)?[index]?))
            };
            DiskIoPoint {
                t: *t,
                read_bps: sum(0),
                write_bps: sum(1),
                read_ops_per_sec: sum(2),
                write_ops_per_sec: sum(3),
                read_request_ms: request_ms(sum(6), sum(4)),
                write_request_ms: request_ms(sum(7), sum(5)),
            }
        })
        .collect()
}

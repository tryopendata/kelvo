use super::*;
use kelvo_engine::RecentNet;

const NOW: i64 = 1_800_000_000_000;

fn temp_dir(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("kelvo-shell-{name}-"))
        .tempdir()
        .unwrap()
}

fn local_record() -> HostRecord {
    HostRecord {
        id: kelvo_schema::HostId(uuid::Uuid::from_u128(7)),
        is_local: true,
        display_name: "Mac".into(),
        info: kelvo_schema::HostInfo {
            os: kelvo_schema::OsKind::MacOs,
            os_version: "27.0".into(),
            model: None,
            chip: None,
            chip_known: true,
            cpu_topology: Vec::new(),
            mem_total_bytes: 0,
            boot_time_ms: 0,
            gpu_dvfs_mhz: Vec::new(),
            boot_mounts: Vec::new(),
        },
    }
}

/// D-064 (#2): a store that opens but cannot take the local host makes history
/// unavailable with the reason; it does not stop the launch. `reset_history` then
/// recovers it.
#[test]
fn disk_io_requests_validate_bounds_before_reading_history() {
    let history = History::unavailable();
    let base = kelvo_schema::disk_io::DiskIoRequest {
        host: local_record().id,
        device: Some("disk0".into()),
        from_ms: 0,
        to_ms: 10_000,
        max_points: 100,
    };
    for bad in [
        kelvo_schema::disk_io::DiskIoRequest {
            max_points: 0,
            ..base.clone()
        },
        kelvo_schema::disk_io::DiskIoRequest {
            max_points: 2001,
            ..base.clone()
        },
        kelvo_schema::disk_io::DiskIoRequest {
            from_ms: -1,
            ..base.clone()
        },
        kelvo_schema::disk_io::DiskIoRequest {
            to_ms: 0,
            ..base.clone()
        },
        kelvo_schema::disk_io::DiskIoRequest {
            to_ms: MAX_NET_SPAN_MS + 1,
            ..base.clone()
        },
        kelvo_schema::disk_io::DiskIoRequest {
            device: Some("disk0/escape".into()),
            ..base.clone()
        },
    ] {
        assert!(matches!(
            history.disk_io(&bad, 10_000, Vec::new),
            Err(CommandError::InvalidArgument { .. })
        ));
    }
    assert!(history.disk_io(&base, 10_000, Vec::new).is_ok());
}

/// `reset_history` emits `history-health-changed` only with the health this returns:
/// a failed reset returns the error and the frontend keeps its cached
/// `history_unavailable` (and the Settings banner).
#[test]
fn reset_returns_health_only_when_history_is_back() {
    let tmp = temp_dir("reset-health");
    let dir = tmp.path();
    let other = Store::open(StoreConfig::new(dir.join(DB_FILE))).unwrap();
    let history = History::open(dir);
    let mut attached = 0;
    let err = history
        .reset_and_attach(NOW, |_| attached += 1)
        .unwrap_err();
    assert!(matches!(err, CommandError::StoreBusy { .. }), "{err:?}");
    assert_eq!(
        attached, 0,
        "nothing reattached to a store that is not there"
    );

    other.close().unwrap();
    let health = history
        .reset_and_attach(NOW, |h| {
            attached += 1;
            assert!(h.register_host(&local_record()).is_some());
        })
        .unwrap();
    assert_eq!(health, HistoryHealth::default());
    assert_eq!(attached, 1);

    // Reattaching can still lose the store (it refuses a host, as `register_host`
    // handles it): an error, no health.
    let err = history
            .reset_and_attach(NOW + 1, |h| {
                let conn = kelvo_store::rusqlite::Connection::open(dir.join(DB_FILE)).unwrap();
                conn.execute_batch("CREATE TRIGGER refuse_hosts BEFORE INSERT ON hosts BEGIN SELECT RAISE(ABORT, 'disk says no'); END;").unwrap();
                assert!(h.register_host(&local_record()).is_none());
            })
            .unwrap_err();
    assert!(
        matches!(&err, CommandError::HistoryUnavailable { reason: Some(_) }),
        "{err:?}"
    );
    history.close();
}

/// The request as the frontend sends it: one local day per entry, 25 boundaries each.
fn heatmap_request(days: serde_json::Value) -> HeatmapRequest {
    serde_json::from_value(serde_json::json!({
        "host": local_record().id,
        "metric": "cpu",
        "days": days,
    }))
    .unwrap()
}

#[test]
fn a_heatmap_request_reads_one_value_per_local_hour() {
    let tmp = temp_dir("heatmap");
    let dir = tmp.path();
    let history = History::open(dir);
    let w = history.register_host(&local_record()).unwrap();
    let series: std::sync::Arc<[SeriesKey]> = vec![SeriesKey::parse("cpu.total").unwrap()].into();
    const H: i64 = 3_600_000;
    // A 23-hour day starting at NOW (hour 2 does not exist), with minutes in hour 0
    // (value 10) and hour 3 (value 30).
    let mut starts: Vec<i64> = vec![NOW, NOW + H, NOW + 2 * H];
    starts.extend((3..=24).map(|h| NOW + (h - 1) * H));
    for (start, v) in [(NOW, 10.0), (NOW + 2 * H, 30.0)] {
        for m in 0..60 {
            w.write_bucket(kelvo_store::BucketRow {
                host: local_record().id,
                tier: Tier::M1,
                bucket_ts: start + m * 60_000,
                series: std::sync::Arc::clone(&series),
                stats: vec![v, v, v],
            })
            .unwrap();
        }
    }
    w.flush().unwrap();

    let req = heatmap_request(serde_json::json!([
        { "date": "2026-03-08", "hour_starts": starts },
    ]));
    let cells = req.cells().unwrap();
    let hours = history
        .read(|r| r.heatmap(req.host, &req.metric.series(), &cells))
        .unwrap();
    let days = req.days(&hours);
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].date, "2026-03-08");
    let mut want = vec![None; 24];
    want[0] = Some(10.0);
    want[3] = Some(30.0);
    assert_eq!(days[0].hours, want, "hour 2 is empty, empty hours are null");
    assert_eq!(
        serde_json::to_value(&days[0]).unwrap()["hours"][1],
        serde_json::Value::Null
    );
    history.close();
}

#[test]
fn malformed_heatmap_days_are_invalid_arguments() {
    let day = |starts: Vec<i64>| serde_json::json!({ "date": "d", "hour_starts": starts });
    let ok: Vec<i64> = (0..25).map(|h| NOW + h * 3_600_000).collect();
    let invalid = |days: serde_json::Value| {
        matches!(
            heatmap_request(days).cells(),
            Err(CommandError::InvalidArgument { .. })
        )
    };
    assert!(!invalid(serde_json::json!([day(ok.clone())])));
    assert!(
        invalid(serde_json::json!([day(ok[..24].to_vec())])),
        "24 boundaries"
    );
    let mut backwards = ok.clone();
    backwards.swap(3, 4);
    assert!(invalid(serde_json::json!([day(backwards)])));
    assert!(
        invalid(serde_json::json!([day(ok.clone()), day(ok.clone())])),
        "the second day overlaps the first"
    );
    let many: Vec<_> = (0..=MAX_HEATMAP_DAYS as i64)
        .map(|d| day(ok.iter().map(|t| t + d * 86_400_000).collect()))
        .collect();
    assert!(invalid(serde_json::Value::Array(many)));
}

fn net_bucket(apps: &[(Option<&str>, u64, u64)], rx: u64, tx: u64) -> kelvo_store::NetBucket {
    kelvo_store::NetBucket {
        measured_ms: 10_000,
        iface_rx_bytes: rx,
        iface_tx_bytes: tx,
        iface_rx_pkts: rx / 1_500,
        iface_tx_pkts: tx / 1_500,
        apps: apps
            .iter()
            .map(|&(name, rx_bytes, tx_bytes)| kelvo_store::NetApp {
                name: name.map(str::to_owned),
                rx_bytes,
                tx_bytes,
            })
            .collect(),
    }
}

/// `query_network_by_app`'s read: committed 10 s rows plus the engine's ring, which
/// replaces a stored bucket it also holds, split into apps, other apps, overhead and
/// "System and other" that add up to the interface totals.
#[test]
fn network_by_app_merges_the_ring_over_the_store_and_adds_up() {
    let tmp = temp_dir("net-by-app");
    let dir = tmp.path();
    let history = History::open(dir);
    let host = local_record().id;
    let w = history.register_host(&local_record()).unwrap();
    // NOW is on the 10 s grid. Two committed buckets; the second is also in the ring,
    // newer (the open bucket grew).
    w.write_net_bucket(
        host,
        NOW,
        net_bucket(&[(Some("Safari"), 400_000, 20_000)], 600_000, 60_000),
    )
    .unwrap();
    w.write_net_bucket(
        host,
        NOW + 10_000,
        net_bucket(&[(Some("Safari"), 1, 1)], 2, 2),
    )
    .unwrap();
    w.flush().unwrap();
    let recent = vec![
        (
            NOW + 10_000,
            net_bucket(
                &[
                    (Some("Docker Desktop"), 9_000_000, 90_000),
                    (None, 50_000, 5_000),
                ],
                9_900_000,
                300_000,
            ),
        ),
        (
            NOW + 20_000,
            net_bucket(&[(Some("Safari"), 100_000, 10_000)], 150_000, 30_000),
        ),
    ];

    let n = history
        .network_by_app(host, NOW + 3_000, NOW + 25_000, || RecentNet {
            buckets: recent.clone(),
            complete_to_ms: Some(NOW + 20_000),
        })
        .unwrap();
    assert_eq!((n.from_ms, n.to_ms), (NOW, NOW + 30_000), "whole buckets");
    assert_eq!(n.complete_to_ms, NOW + 20_000, "the open bucket's start");
    assert_eq!(n.measured_ms, 30_000);
    assert_eq!(n.resolution_ms, 10_000);
    assert_eq!(
        n.coverage,
        vec![NetworkSpan {
            from_ms: NOW,
            to_ms: NOW + 30_000,
            tier: Some(Tier::S10)
        }]
    );
    let names: Vec<_> = n
        .apps
        .iter()
        .map(|a| (a.name.as_str(), a.rx_bytes))
        .collect();
    assert_eq!(
        names,
        [("Docker Desktop", 9_000_000), ("Safari", 500_000)],
        "the ring's bucket replaced the stored one; other apps are separate"
    );
    assert_eq!(
        (n.other_apps_rx_bytes, n.other_apps_tx_bytes),
        (50_000, 5_000)
    );
    assert_eq!(n.iface_rx_bytes, 10_650_000);
    assert_eq!(
        n.overhead_rx_bytes,
        (600_000 / 1_500 + 9_900_000 / 1_500 + 150_000 / 1_500) * 66
    );
    for (iface, apps, other, overhead, system) in [
        (
            n.iface_rx_bytes,
            n.apps.iter().map(|a| a.rx_bytes).sum::<u64>(),
            n.other_apps_rx_bytes,
            n.overhead_rx_bytes,
            n.system_rx_bytes,
        ),
        (
            n.iface_tx_bytes,
            n.apps.iter().map(|a| a.tx_bytes).sum::<u64>(),
            n.other_apps_tx_bytes,
            n.overhead_tx_bytes,
            n.system_tx_bytes,
        ),
    ] {
        assert!(system > 0);
        assert_eq!(apps + other + overhead + system, iface);
    }
    assert!(!n.clamped);

    // Before anything was recorded: an empty, honest answer.
    let empty = history
        .network_by_app(host, NOW - 60_000, NOW - 30_000, || RecentNet {
            buckets: Vec::new(),
            complete_to_ms: Some(NOW + 20_000),
        })
        .unwrap();
    assert_eq!(empty.measured_ms, 0);
    assert!(empty.apps.is_empty());
    assert!(empty.coverage.iter().all(|s| s.tier.is_none()));
    assert_eq!(
        empty.complete_to_ms, empty.to_ms,
        "before the engine's edge: final"
    );

    assert!(matches!(
        history.network_by_app(host, NOW, NOW - 1, RecentNet::default),
        Err(CommandError::InvalidArgument { .. })
    ));
    history.close();
}

#[test]
fn network_by_app_promises_nothing_past_what_the_engine_closed() {
    let history = History::unavailable();
    let host = local_record().id;
    // Nothing open: the streams have reported, and the engine closed, to NOW + 10 s.
    // The range runs into the future; only its part before that edge is final.
    let n = history
        .network_by_app(host, NOW - 30_000, NOW + 60_000, || RecentNet {
            buckets: Vec::new(),
            complete_to_ms: Some(NOW + 10_000),
        })
        .unwrap();
    assert_eq!(n.to_ms, NOW + 60_000);
    assert_eq!(n.complete_to_ms, NOW + 10_000);
    // No edge (only before the engine starts): nothing promised.
    let n = history
        .network_by_app(host, NOW - 30_000, NOW + 60_000, RecentNet::default)
        .unwrap();
    assert_eq!(n.complete_to_ms, NOW - 30_000);
}

#[test]
fn network_by_app_refuses_spans_past_the_longest_retention() {
    let history = History::unavailable();
    let host = local_record().id;
    let longest = 90 * Retention::DAY_MS;
    assert!(
        history
            .network_by_app(host, NOW - longest, NOW, RecentNet::default)
            .is_ok()
    );
    let err = history
        .network_by_app(host, NOW - longest - 10_000, NOW, RecentNet::default)
        .unwrap_err();
    assert!(
        matches!(&err, CommandError::InvalidArgument { message } if message.contains("90 days")),
        "{err:?}"
    );
}

#[test]
fn network_by_app_answers_from_the_ring_while_history_is_unavailable() {
    let history = History::unavailable();
    let host = local_record().id;
    let n = history
        .network_by_app(host, NOW, NOW + 30_000, || RecentNet {
            buckets: vec![(
                NOW + 10_000,
                net_bucket(&[(Some("Safari"), 400_000, 20_000)], 600_000, 60_000),
            )],
            complete_to_ms: Some(NOW + 20_000),
        })
        .unwrap();
    assert_eq!(n.measured_ms, 10_000);
    assert_eq!(n.apps.first().map(|a| a.name.as_str()), Some("Safari"));
    assert_eq!(n.iface_rx_bytes, 600_000);
    assert_eq!(
        n.coverage.iter().map(|s| s.tier).collect::<Vec<_>>(),
        [None, Some(Tier::S10), None],
        "live-only: what the ring has, the rest not recorded"
    );
    assert_eq!(n.complete_to_ms, NOW + 20_000);
}

/// Minute rows of `battery.charge` (`charge(m)`) and `battery.charging`
/// (`charging(m)`) for minutes `minutes` after `NOW`.
fn battery_minutes(
    minutes: std::ops::Range<i64>,
    charge: impl Fn(i64) -> f32,
    charging: impl Fn(i64) -> f32,
) -> Vec<BucketRow> {
    let series: std::sync::Arc<[SeriesKey]> = vec![
        SeriesKey::parse("battery.charge").unwrap(),
        SeriesKey::parse("battery.charging").unwrap(),
    ]
    .into();
    minutes
        .map(|m| {
            let (v, c) = (charge(m), charging(m));
            BucketRow {
                host: local_record().id,
                tier: Tier::M1,
                bucket_ts: NOW + m * 60_000,
                series: std::sync::Arc::clone(&series),
                stats: vec![v, v, v, c, c, c],
            }
        })
        .collect()
}

/// 10 s rows of `net.rx_total` (1,000 B/s) and `net.tx_total` (100 B/s) for buckets
/// `buckets` after `NOW`.
fn net_buckets(buckets: impl Iterator<Item = i64>) -> Vec<BucketRow> {
    let series: std::sync::Arc<[SeriesKey]> = vec![
        SeriesKey::parse("net.rx_total").unwrap(),
        SeriesKey::parse("net.tx_total").unwrap(),
    ]
    .into();
    buckets
        .map(|b| BucketRow {
            host: local_record().id,
            tier: Tier::S10,
            bucket_ts: NOW + b * 10_000,
            series: std::sync::Arc::clone(&series),
            stats: vec![900.0, 1_100.0, 1_000.0, 90.0, 110.0, 100.0],
        })
        .collect()
}

#[test]
fn series_stats_average_peak_and_gaps_by_module() {
    const S: i64 = 1_000;
    let series: std::sync::Arc<[SeriesKey]> = vec![
        SeriesKey::parse("cpu.total").unwrap(),
        SeriesKey::parse("disk.read_total").unwrap(),
    ]
    .into();
    // `cpu.total` at 20% for buckets 0..3 and 50% (peaking at 90%) for 3..6;
    // `disk.read_total` at 1,000 B/s throughout.
    let rows = (0..6)
        .map(|b| BucketRow {
            host: local_record().id,
            tier: Tier::S10,
            bucket_ts: NOW + b * 10_000,
            series: std::sync::Arc::clone(&series),
            stats: if b < 3 {
                vec![10.0, 30.0, 20.0, 1_000.0, 1_000.0, 1_000.0]
            } else {
                vec![10.0, 90.0, 50.0, 1_000.0, 1_000.0, 1_000.0]
            },
        })
        .collect::<Vec<_>>();
    let tmp = temp_dir("series-stats");
    let dir = tmp.path();
    let history = History::open(dir);
    let w = history.register_host(&local_record()).unwrap();
    for row in rows {
        w.write_bucket(row).unwrap();
    }
    // Disk switched off over bucket 0: CPU still counts it.
    let host = local_record().id;
    w.write_gap(
        host,
        kelvo_schema::Gap::new(
            NOW,
            Some(NOW + 10 * S),
            Some(kelvo_schema::Module::Disk),
            kelvo_schema::GapReason::ModuleDisabled,
        )
        .unwrap(),
    )
    .unwrap();
    w.flush().unwrap();
    let metrics = ["cpu.total".to_owned(), "disk.read_total".to_owned()];
    let s = history
        .series_stats(host, &metrics, NOW, NOW + 60 * S, NOW + 600 * S, Vec::new)
        .unwrap();
    let [cpu, disk] = s.metrics.as_slice() else {
        panic!("two metrics: {s:?}")
    };
    assert_eq!(cpu.measured_ms, 60_000);
    assert_eq!(cpu.avg, Some(35.0));
    assert_eq!(cpu.max, Some(90.0));
    assert_eq!(disk.measured_ms, 50_000, "Disk's own gap");
    assert_eq!(disk.integral, 50_000.0);
    history.close();

    let labelled = History::unavailable().series_stats(
        host,
        &["cpu.load".to_owned()],
        NOW,
        NOW + S,
        NOW,
        Vec::new,
    );
    assert!(matches!(
        labelled,
        Err(CommandError::InvalidArgument { .. })
    ));
}

#[test]
fn series_stats_count_measured_time_through_now() {
    const S: i64 = 1_000;
    let tmp = temp_dir("network-totals");
    let dir = tmp.path();
    let history = History::open(dir);
    let w = history.register_host(&local_record()).unwrap();
    // Buckets 12 and 13 have no reading; 25.. are only in the engine.
    for row in net_buckets((0..25).filter(|b| !(12..14).contains(b))) {
        w.write_bucket(row).unwrap();
    }
    let host = local_record().id;
    // Asleep over all of bucket 10 and half of 11. CPU switched off over the first
    // buckets does not touch Network.
    w.write_gap(
        host,
        kelvo_schema::Gap::new(
            NOW + 100 * S,
            Some(NOW + 115 * S),
            None,
            kelvo_schema::GapReason::Sleep,
        )
        .unwrap(),
    )
    .unwrap();
    w.write_gap(
        host,
        kelvo_schema::Gap::new(
            NOW,
            Some(NOW + 20 * S),
            Some(kelvo_schema::Module::Cpu),
            kelvo_schema::GapReason::ModuleDisabled,
        )
        .unwrap(),
    )
    .unwrap();
    w.flush().unwrap();
    // Bucket 27 is open: now is halfway through it. 28 is past now.
    let recent = || net_buckets(20..29);
    let net = || vec!["net.rx_total".to_owned(), "net.tx_total".to_owned()];
    let totals = history
        .series_stats(
            host,
            &net(),
            NOW + 5 * S,
            NOW + 400 * S,
            NOW + 275 * S,
            recent,
        )
        .unwrap();
    // 28 buckets to now, less 12 and 13, 10 (asleep), half of 11 and half of 27.
    let measured = 240 * S;
    let rx_tx = |t: &RangeStats| {
        t.metrics
            .iter()
            .map(|m| (m.measured_ms, m.integral.round() as i64))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        (totals.from_ms, totals.to_ms),
        (NOW, NOW + 275 * S),
        "widened to the bucket, cut at now"
    );
    assert_eq!(
        rx_tx(&totals),
        [(measured, 240_000), (measured, 24_000)],
        "gaps and empty buckets not counted"
    );
    history.close();

    // Live-only: the engine's rows alone.
    let totals = History::unavailable()
        .series_stats(
            host,
            &net(),
            NOW + 200 * S,
            NOW + 270 * S,
            NOW + 275 * S,
            recent,
        )
        .unwrap();
    assert_eq!(rx_tx(&totals)[0], (70_000, 70_000));

    let bad = History::unavailable().series_stats(host, &net(), NOW + S, NOW, NOW, Vec::new);
    assert!(matches!(bad, Err(CommandError::InvalidArgument { .. })));
}

#[test]
fn battery_hours_follow_local_hours_in_a_half_hour_zone_through_now() {
    const MIN: i64 = 60_000;
    let tmp = temp_dir("battery-hours");
    let dir = tmp.path();
    let history = History::open(dir);
    let w = history.register_host(&local_record()).unwrap();
    // Charge is the minute's index; the battery charged in minute 25 and minute 105.
    let charging = |m: i64| if m == 25 || m == 105 { 1.0 } else { 0.0 };
    for row in battery_minutes(0..100, |m| m as f32, charging) {
        w.write_bucket(row).unwrap();
    }
    w.flush().unwrap();
    // UTC+5:30: local hours start at :30 UTC. NOW is a UTC hour.
    let starts = [NOW + 30 * MIN, NOW + 90 * MIN, NOW + 150 * MIN];
    // Minutes 100..110 are only in the engine (not committed yet), 95..100 in both.
    let recent = || battery_minutes(95..110, |m| m as f32, charging);
    let hours = history
        .battery_hours(local_record().id, &starts, recent)
        .unwrap();
    assert_eq!(
        hours,
        [
            BatteryHour {
                start_ms: NOW + 30 * MIN,
                charge: Some(89.0),
                charging: false,
            },
            BatteryHour {
                start_ms: NOW + 90 * MIN,
                charge: Some(109.0),
                charging: true,
            },
        ],
        "minute 25 is in the UTC hour but before the local one; 105 is not committed"
    );
    history.close();

    // Live-only: the engine's minutes alone.
    let hours = History::unavailable()
        .battery_hours(local_record().id, &starts, recent)
        .unwrap();
    assert_eq!(hours[0].charge, None, "nothing that old in the engine");
    assert_eq!(hours[1].charge, Some(109.0));

    let bad = History::unavailable().battery_hours(local_record().id, &[NOW + MIN, NOW], Vec::new);
    assert!(matches!(bad, Err(CommandError::InvalidArgument { .. })));
}

#[test]
fn battery_hours_past_the_minutes_kept_read_quarter_hours() {
    const MIN: i64 = 60_000;
    let tmp = temp_dir("battery-hours-quarters");
    let dir = tmp.path();
    let history = History::open(dir);
    let w = history.register_host(&local_record()).unwrap();
    // The hour before last is only in `tier_15m` (rolled down): charge 10..40 by
    // quarter, charging in the second. The last hour has its minutes.
    for row in battery_minutes(0..4, |q| 10.0 * (q + 1) as f32, |q| (q == 1) as u8 as f32) {
        w.write_bucket(BucketRow {
            tier: Tier::M15,
            bucket_ts: NOW - 120 * MIN + (row.bucket_ts - NOW) * 15,
            ..row
        })
        .unwrap();
    }
    for row in battery_minutes(-60..0, |m| (100 + m) as f32, |_| 0.0) {
        w.write_bucket(row).unwrap();
    }
    w.flush().unwrap();
    let starts = [NOW - 120 * MIN, NOW - 60 * MIN, NOW, NOW + 60 * MIN];
    let hours = history
        .battery_hours(local_record().id, &starts, Vec::new)
        .unwrap();
    let got: Vec<_> = hours.iter().map(|h| (h.charge, h.charging)).collect();
    assert_eq!(
        got,
        [(Some(40.0), true), (Some(99.0), false), (None, false)],
        "the old hour from its quarters, the last from its minutes"
    );
    history.close();
}

#[test]
fn history_pages_carry_each_series_hold() {
    let history = History::unavailable();
    let request = HistoryRequest {
        host: local_record().id,
        selectors: vec![kelvo_schema::SeriesSelector {
            metric: MetricId::from_static("battery.charge"),
            labels: Labels::new(),
        }],
        from_ms: NOW,
        to_ms: NOW + 3_600_000,
        tier: TierRequest::M1,
        max_points: 60,
    };
    let page = history
        .history(
            &request,
            || battery_minutes(0..3, |m| m as f32, |_| 0.0),
            |_, bucket_ms| bucket_ms * 3,
        )
        .unwrap();
    assert_eq!(page.series.len(), 1);
    assert_eq!(page.series[0].points.len(), 3);
    assert_eq!(page.series[0].hold_ms, 180_000);
}

#[test]
fn network_by_app_json_is_snake_case_numbers() {
    let n = NetworkByApp::from(kelvo_engine::attribute(kelvo_store::NetByApp {
        from_ms: NOW,
        to_ms: NOW + 10_000,
        coverage: vec![kelvo_store::NetSpan {
            from_ms: NOW,
            to_ms: NOW + 10_000,
            tier: None,
        }],
        resolution_ms: 10_000,
        measured_ms: 0,
        iface_rx_bytes: 0,
        iface_tx_bytes: 0,
        iface_rx_pkts: 0,
        iface_tx_pkts: 0,
        apps: Vec::new(),
    }));
    let json = serde_json::to_value(&n).unwrap();
    assert_eq!(
        json["coverage"],
        serde_json::json!([{ "from_ms": NOW, "to_ms": NOW + 10_000, "tier": null }])
    );
    assert_eq!(json["system_rx_bytes"], 0);
    assert_eq!(json["clamped"], false);
}

#[test]
fn health_event_json() {
    let health = HistoryHealth {
        low_disk_paused: true,
        trimmed_before_ms: Some(1_700_000_000_000),
        trimmed_limit_bytes: Some(150_000_000),
        cap_met: true,
    };
    assert_eq!(
        serde_json::to_value(crate::ipc::HistoryHealthChanged { health }).unwrap(),
        serde_json::json!({ "health": {
            "low_disk_paused": true,
            "trimmed_before_ms": 1_700_000_000_000_i64,
            "trimmed_limit_bytes": 150_000_000,
            "cap_met": true,
        }})
    );
}

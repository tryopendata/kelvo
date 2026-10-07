//! v1.1 reads over long ranges: the Timeline's 7d and 30d ranges (`auto` tier, merged
//! slots, explicit gaps), the 30-day heatmap by local hour, and the CSV export.

// clippy.toml allows unwrap inside #[test] functions only; the helpers in this test-only
// file panic on failure by design.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use common::*;
use kelvo_schema::{Gap, GapReason, HostId, Labels, MetricId, SeriesKey, SeriesSelector, Tier};
use kelvo_store::{
    ExportQuery, HistoryQuery, HistoryResult, Reader, Retention, Store, StoreError, TierChoice,
    Writer,
};

const Q: i64 = 15 * MIN;
const SERIES: [&str; 3] = [
    "cpu.total",
    "thermal.hottest",
    // Two labels: the CSV header must quote the comma.
    "net.rx{iface=en0,kind=wifi}",
];
/// The 30-day fixture ends here; it was pruned at this time.
const NOW: i64 = T0 + 30 * DAY;

fn sel(metric: &'static str) -> SeriesSelector {
    SeriesSelector {
        metric: MetricId::from_static(metric),
        labels: Labels::new(),
    }
}

fn all() -> Vec<SeriesSelector> {
    vec![sel("cpu.total"), sel("thermal.hottest"), sel("net.rx")]
}

/// `cpu.total` at a minute: its minute of the hour, so any hour averages 29.5.
fn cpu_at(ts: i64) -> f32 {
    ((ts / MIN) % 60) as f32
}

fn write_minute(w: &Writer, h: HostId, l: &Arc<[SeriesKey]>, ts: i64) {
    let cpu = cpu_at(ts);
    let day = ((ts - T0).div_euclid(DAY)) as f32;
    w.write_bucket(kelvo_store::BucketRow {
        host: h,
        tier: Tier::M1,
        bucket_ts: ts,
        series: Arc::clone(l),
        stats: vec![
            cpu - 1.0,
            cpu + 1.0,
            cpu,
            40.0 + day,
            50.0 + day,
            45.0 + day,
            0.0,
            ((ts / MIN) % 1440) as f32,
            ((ts / MIN) % 720) as f32,
        ],
    })
    .unwrap();
}

/// Is `ts` inside the fixture's sleep (01:00 to 07:00 UTC every night) or the app-off
/// afternoon (day 10, 12:00 to 14:00)?
fn asleep(ts: i64) -> bool {
    let into_day = (ts - T0).rem_euclid(DAY);
    let day = (ts - T0).div_euclid(DAY);
    (HOUR..7 * HOUR).contains(&into_day)
        || (day == 10 && (12 * HOUR..14 * HOUR).contains(&into_day))
}

/// 30 days of minutes with a sleep gap every night and one app-off gap, pruned at `NOW`
/// the way the app prunes: the oldest 23 days roll down into 15-minute rows (D-076).
fn thirty_days(dir: &TempDir) -> (Store, HostId) {
    let store = open(dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&SERIES);
    for day in 0..30 {
        let start = T0 + day * DAY;
        for m in 0..24 * 60 {
            let ts = start + m * MIN;
            if !asleep(ts) {
                write_minute(&w, h.id, &l, ts);
            }
        }
        w.write_gap(
            h.id,
            Gap::host(start + HOUR, Some(start + 7 * HOUR), GapReason::Sleep).unwrap(),
        )
        .unwrap();
        if day == 10 {
            w.write_gap(
                h.id,
                Gap::host(
                    start + 12 * HOUR,
                    Some(start + 14 * HOUR),
                    GapReason::AppNotRunning,
                )
                .unwrap(),
            )
            .unwrap();
        }
        w.flush().unwrap();
    }
    let report = w.prune(NOW, Retention::default()).unwrap();
    assert_eq!(
        report.m1_rolled,
        23 * 18 * 60 - 120,
        "23 days of awake minutes rolled"
    );
    (store, h.id)
}

fn history(r: &mut Reader, h: HostId, from: i64, to: i64, max_points: u32) -> HistoryResult {
    r.history(&HistoryQuery {
        host: h,
        selectors: all(),
        from_ms: from,
        to_ms: to,
        tier: TierChoice::Auto,
        max_points,
    })
    .unwrap()
}

fn cpu_points(res: &HistoryResult) -> &[kelvo_store::Point] {
    &res.series
        .iter()
        .find(|s| s.key.to_string() == "cpu.total")
        .unwrap()
        .points
}

#[test]
fn seven_days_read_minutes_and_thirty_days_read_quarters_with_every_gap() {
    let dir = temp_dir("long-ranges");
    let (store, h) = thirty_days(&dir);
    let mut r = store.reader().unwrap();

    // 7d at about 2 points per pixel of a 1,008-pixel plot: minutes, merged by 5.
    let week = history(&mut r, h, NOW - 7 * DAY, NOW, 2016);
    assert_eq!(week.tier, Tier::M1);
    assert_eq!(week.bucket_ms, 5 * MIN);
    let pts = cpu_points(&week);
    assert_eq!(
        pts.len(),
        7 * 18 * 12,
        "18 awake hours of 5-minute slots a day"
    );
    // A slot is the mean of its minutes: minutes 0..=4 of an hour average 2.
    assert_eq!(pts[0].avg, 2.0);
    // Fewer points: "10 MIN AVG".
    assert_eq!(
        history(&mut r, h, NOW - 7 * DAY, NOW, 1008).bucket_ms,
        10 * MIN
    );

    // 30d: quarters for the 23 rolled days, the minutes of the last 7 folded into the
    // same slots, merged by 2 into half hours.
    let month = history(&mut r, h, NOW - 30 * DAY, NOW, 1440);
    assert_eq!(month.tier, Tier::M15);
    assert_eq!(month.bucket_ms, 2 * Q);
    let pts = cpu_points(&month);
    assert!(pts.len() <= 1440);
    assert_eq!(
        pts.len(),
        30 * 18 * 2 - 4,
        "every awake half hour, minus day 10's 2 h off"
    );
    // A half hour of minutes 0..=29 averages 14.5 and of 30..=59 44.5, whether it was
    // read as two quarters (7 and 22, 37 and 52) or as minutes.
    assert!(pts.iter().all(|p| p.avg == 14.5 || p.avg == 44.5));
    // No hole and no seam at the 7-day line: half hours run on across it.
    let seam = NOW - 7 * DAY;
    assert!(pts.iter().any(|p| p.t == seam - 2 * Q));
    assert!(pts.iter().any(|p| p.t == seam));

    // Gaps come back as rows, every one; no point is drawn inside them.
    let sleeps = |res: &HistoryResult| {
        res.gaps
            .iter()
            .filter(|g| g.reason == GapReason::Sleep)
            .count()
    };
    assert_eq!(sleeps(&week), 7);
    assert_eq!(sleeps(&month), 30);
    assert_eq!(month.gaps.len(), 31, "and day 10's app-off afternoon");
    for res in [&week, &month] {
        for p in cpu_points(res) {
            assert!(!asleep(p.t), "a point at {} inside a gap", p.t);
            assert!(
                !res.gaps.iter().any(
                    |g| g.start_ms <= p.t && g.end_ms.is_some_and(|e| p.t + res.bucket_ms <= e)
                ),
                "slot at {} lies wholly inside a gap",
                p.t
            );
        }
    }
}

/// Local-hour cells in UTC for one day, from its 25 hour starts (the 24 local hours, then
/// the next midnight): what the frontend sends.
fn cells(starts: &[i64]) -> Vec<(i64, i64)> {
    assert_eq!(starts.len(), 25);
    starts.windows(2).map(|w| (w[0], w[1])).collect()
}

/// UTC ms of `y-m-d h:00Z`.
fn utc(days_from_t0: i64, hour: i64) -> i64 {
    T0 + days_from_t0 * DAY + hour * HOUR
}

#[test]
fn heatmap_hours_follow_dst_in_both_directions() {
    let dir = temp_dir("heatmap-dst");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&SERIES);
    // T0 is 2026-09-01. New York springs forward on 2026-03-08 (177 days earlier) and
    // falls back on 2026-11-01 (61 days later).
    let spring = -177;
    let fall = 61;
    // Two whole UTC days of minutes around each, but none 10:00 to 12:00 UTC on the
    // spring day.
    for day in [spring, spring + 1, fall, fall + 1] {
        for m in 0..24 * 60 {
            let ts = utc(day, 0) + m * MIN;
            if day == spring && (utc(spring, 10)..utc(spring, 12)).contains(&ts) {
                continue;
            }
            write_minute(&w, h.id, &l, ts);
        }
    }
    w.flush().unwrap();
    let mut r = store.reader().unwrap();
    let cpu = key("cpu.total");

    // 2026-03-08 in New York: midnight and 01:00 are EST (05:00Z, 06:00Z); 02:00 does not
    // exist, so its cell is empty; 03:00 EDT is 07:00Z; the next midnight is 04:00Z.
    let mut starts = vec![utc(spring, 5), utc(spring, 6), utc(spring, 7)];
    starts.extend((3..=24).map(|hr| utc(spring, 7 + hr - 3)));
    let spring_cells = cells(&starts);
    assert_eq!(
        spring_cells.iter().map(|c| c.1 - c.0).sum::<i64>(),
        23 * HOUR
    );
    let hours = r.heatmap(h.id, &cpu, &spring_cells).unwrap();
    assert_eq!(hours.len(), 24);
    assert_eq!(hours[2], None, "02:00 never happened");
    assert_eq!(hours[0], Some(29.5));
    assert_eq!(hours[3], Some(29.5));
    // 10:00Z to 12:00Z is 06:00 and 07:00 EDT: no buckets, null, never zero.
    assert_eq!(hours[6], None);
    assert_eq!(hours[7], None);
    assert_eq!(hours[8], Some(29.5));
    assert_eq!(hours.iter().filter(|v| v.is_some()).count(), 21);

    // 2026-11-01: midnight EDT is 04:00Z, 01:00 EDT 05:00Z, then 01:00 again as EST; 02:00
    // EST is 07:00Z, so local 01:00 is a two-hour cell. The next midnight is 05:00Z.
    let mut starts = vec![utc(fall, 4), utc(fall, 5)];
    starts.extend((2..=24).map(|hr| utc(fall, 7 + hr - 2)));
    let fall_cells = cells(&starts);
    assert_eq!(fall_cells.iter().map(|c| c.1 - c.0).sum::<i64>(), 25 * HOUR);
    let hours = r.heatmap(h.id, &cpu, &fall_cells).unwrap();
    assert!(hours.iter().all(|v| *v == Some(29.5)), "{hours:?}");
    // The doubled hour averages both real hours: thermal.hottest steps by day, and 01:00
    // EST is still the same UTC day, so check a series that differs between the two
    // hours instead.
    let rx = key("net.rx{iface=en0,kind=wifi}");
    let hours = r.heatmap(h.id, &rx, &fall_cells).unwrap();
    // net.rx avg is the minute of the day mod 720: 05:00Z..07:00Z are minutes 300..419.
    assert_eq!(hours[1], Some((300 + 419) as f32 / 2.0));
    // A series the layout does not have: every hour null.
    let none = r.heatmap(h.id, &key("gpu.util"), &fall_cells).unwrap();
    assert!(none.iter().all(Option::is_none));
}

#[test]
fn heatmap_mixes_quarters_and_minutes_by_width() {
    let dir = temp_dir("heatmap-mix");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&SERIES);
    // One hour of minutes (cpu.total = minute of the hour), and a second hour with only
    // its first quarter.
    let hour = T0 + 5 * HOUR;
    for m in 0..60 {
        write_minute(&w, h.id, &l, hour + m * MIN);
    }
    for m in 0..15 {
        write_minute(&w, h.id, &l, hour + HOUR + m * MIN);
    }
    w.flush().unwrap();
    // The roll cut lands half way through the first hour: two quarters roll down (averages
    // 7 and 22), the minutes 30..=59 stay.
    w.prune(hour + 30 * MIN + 7 * DAY, Retention::default())
        .unwrap();
    let db = raw(&store);
    assert_eq!(count(&db, "SELECT count(*) FROM tier_15m"), 2);
    assert_eq!(count(&db, "SELECT count(*) FROM tier_1m"), 30 + 15);

    let mut r = store.reader().unwrap();
    let cpu = key("cpu.total");
    let hours = r
        .heatmap(
            h.id,
            &cpu,
            &[
                (hour - HOUR, hour),
                (hour, hour + HOUR),
                (hour + HOUR, hour + 2 * HOUR),
            ],
        )
        .unwrap();
    // (7 * 15 + 22 * 15 + (30 + ... + 59)) / 60 = 29.5, the mean of the 60 minutes.
    assert_eq!(hours, vec![None, Some(29.5), Some(7.0)]);
}

/// A minimal CSV reader for the round trip: quoted fields with doubled quotes.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .map(|line| {
            let mut fields = Vec::new();
            let mut cur = String::new();
            let mut quoted = false;
            let mut chars = line.chars().peekable();
            while let Some(c) = chars.next() {
                match (c, quoted) {
                    ('"', true) if chars.peek() == Some(&'"') => {
                        cur.push('"');
                        chars.next();
                    }
                    ('"', _) => quoted = !quoted,
                    (',', false) => fields.push(std::mem::take(&mut cur)),
                    _ => cur.push(c),
                }
            }
            fields.push(cur);
            fields
        })
        .collect()
}

fn export(
    r: &mut Reader,
    h: HostId,
    from: i64,
    to: i64,
    tier: TierChoice,
) -> (String, kelvo_store::ExportSummary) {
    let mut out = Vec::new();
    let summary = r
        .export_csv(
            &ExportQuery {
                host: h,
                selectors: all(),
                from_ms: from,
                to_ms: to,
                tier,
            },
            &mut out,
        )
        .unwrap();
    (String::from_utf8(out).unwrap(), summary)
}

/// Every export row's values against the same range read through `history` at full
/// resolution: the same buckets, the same values, gaps as their own rows.
fn assert_round_trip(r: &mut Reader, h: HostId, from: i64, to: i64) {
    let (csv, summary) = export(r, h, from, to, TierChoice::Auto);
    let full = history(r, h, from, to, u32::MAX);
    assert_eq!(summary.tier, full.tier);
    assert_eq!(summary.bucket_ms, full.bucket_ms, "one row per bucket");
    let rows = parse_csv(&csv);
    let header = &rows[0];
    let mut expected_header = vec!["time_utc".to_string(), "time_ms".to_string()];
    for s in &full.series {
        for stat in ["avg", "min", "max"] {
            expected_header.push(format!("{}_{stat}", s.key));
        }
    }
    expected_header.push("gap_reason".into());
    expected_header.push("gap_end_ms".into());
    assert_eq!(header, &expected_header);
    let reason = header.len() - 2;

    let (gap_rows, data): (Vec<_>, Vec<_>) =
        rows[1..].iter().partition(|row| !row[reason].is_empty());
    assert_eq!(data.len() as u64, summary.rows);
    assert_eq!(gap_rows.len() as u64, summary.gap_rows);
    assert_eq!(gap_rows.len(), full.gaps.len());
    for (row, gap) in gap_rows.iter().zip(&full.gaps) {
        assert_eq!(row[1].parse::<i64>().unwrap(), gap.start_ms.max(from));
        assert_eq!(row[reason], gap.reason.as_str());
        let end = gap.end_ms.map(|e| e.to_string()).unwrap_or_default();
        assert_eq!(row[reason + 1], end);
        assert!(row[2..reason].iter().all(String::is_empty));
    }
    for row in &data {
        assert_eq!(row.len(), header.len());
        assert!(row[reason + 1].is_empty());
        assert!(
            row[2..reason].iter().any(|c| !c.is_empty()),
            "a data row with no value"
        );
    }
    // Rows are in time order, gap rows included.
    let times: Vec<i64> = rows[1..].iter().map(|r| r[1].parse().unwrap()).collect();
    assert!(times.is_sorted());

    for (i, s) in full.series.iter().enumerate() {
        let col = 2 + 3 * i;
        let parsed: Vec<(i64, f32, f32, f32)> = data
            .iter()
            .filter(|row| !row[col].is_empty())
            .map(|row| {
                (
                    row[1].parse().unwrap(),
                    row[col].parse().unwrap(),
                    row[col + 1].parse().unwrap(),
                    row[col + 2].parse().unwrap(),
                )
            })
            .collect();
        let want: Vec<(i64, f32, f32, f32)> = s
            .points
            .iter()
            .map(|p| (p.t, p.avg, p.min, p.max))
            .collect();
        assert_eq!(parsed, want, "{}", s.key);
    }
    // time_utc matches time_ms.
    assert_eq!(rows[1][0].len(), "2026-09-01T00:00:00Z".len());
}

#[test]
fn csv_export_round_trips_minutes_quarters_and_gaps() {
    let dir = temp_dir("export-round-trip");
    let (store, h) = thirty_days(&dir);
    let mut r = store.reader().unwrap();
    // Two days of minutes, with a night's sleep.
    assert_round_trip(&mut r, h, NOW - 2 * DAY, NOW);
    // 30 days: quarters, the last week's minutes folded into them, every gap.
    assert_round_trip(&mut r, h, NOW - 30 * DAY, NOW);
    // A range starting inside a gap: the gap row sits at the range start.
    assert_round_trip(&mut r, h, T0 + 10 * DAY + 13 * HOUR, T0 + 11 * DAY);

    let (csv, summary) = export(&mut r, h, NOW - 30 * DAY, NOW, TierChoice::Auto);
    assert_eq!(summary.tier, Tier::M15);
    assert_eq!(summary.rows, 30 * 18 * 4 - 8, "every awake quarter");
    assert_eq!(summary.gap_rows, 31);
    let mut lines = csv.lines();
    assert_eq!(
        lines.next().unwrap(),
        "time_utc,time_ms,cpu.total_avg,cpu.total_min,cpu.total_max,\
         \"net.rx{iface=en0,kind=wifi}_avg\",\"net.rx{iface=en0,kind=wifi}_min\",\
         \"net.rx{iface=en0,kind=wifi}_max\",thermal.hottest_avg,thermal.hottest_min,\
         thermal.hottest_max,gap_reason,gap_end_ms"
    );
    // The first quarter: cpu.total over minutes 0..=14, net.rx's avg and max are the
    // minute of the day, thermal.hottest is 45 on day 0.
    assert_eq!(
        lines.next().unwrap(),
        format!("2026-09-01T00:00:00Z,{T0},7,-1,15,7,0,14,45,40,50,,")
    );
    assert!(csv.contains(&format!(
        "\n2026-09-01T01:00:00Z,{}{},sleep,{}\n",
        T0 + HOUR,
        ",".repeat(9),
        T0 + 7 * HOUR
    )));
    assert!(csv.contains(&format!(",app_not_running,{}\n", T0 + 10 * DAY + 14 * HOUR)));

    // Nothing selected that exists: a header with no series, gap rows only.
    let mut out = Vec::new();
    let s = r
        .export_csv(
            &ExportQuery {
                host: h,
                selectors: vec![sel("gpu.util")],
                from_ms: NOW - DAY,
                to_ms: NOW,
                tier: TierChoice::Auto,
            },
            &mut out,
        )
        .unwrap();
    assert_eq!((s.rows, s.gap_rows), (0, 1));
    assert!(
        String::from_utf8(out)
            .unwrap()
            .starts_with("time_utc,time_ms,gap_reason,gap_end_ms\n")
    );
}

#[test]
fn a_module_gap_names_its_module() {
    let dir = temp_dir("export-module-gap");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&SERIES);
    write_minute(&w, h.id, &l, T0);
    w.write_gap(
        h.id,
        Gap::new(
            T0 + MIN,
            None,
            Some(kelvo_schema::Module::Gpu),
            GapReason::ModuleDisabled,
        )
        .unwrap(),
    )
    .unwrap();
    w.flush().unwrap();
    let mut r = store.reader().unwrap();
    let (csv, s) = export(&mut r, h.id, T0, T0 + HOUR, TierChoice::Fixed(Tier::M1));
    assert_eq!((s.rows, s.gap_rows), (1, 1));
    assert!(csv.ends_with(",module_disabled:gpu,\n"), "open: no end");
}

/// A writer that commits a new bucket into the store the first time the export writes
/// (after the header pass, before the rows are read): what a 5-minute commit landing
/// mid-export does.
struct CommitsMidExport<'a> {
    out: Vec<u8>,
    commit: Option<Box<dyn FnOnce() + 'a>>,
}

impl std::io::Write for CommitsMidExport<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(commit) = self.commit.take() {
            commit();
        }
        self.out.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn an_export_reads_one_snapshot() {
    let dir = temp_dir("export-snapshot");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&SERIES);
    for m in 0..10 {
        write_minute(&w, h.id, &l, T0 + m * MIN);
    }
    w.flush().unwrap();
    let q = ExportQuery {
        host: h.id,
        selectors: all(),
        from_ms: T0,
        to_ms: T0 + HOUR,
        tier: TierChoice::Fixed(Tier::M1),
    };
    let mut r = store.reader().unwrap();
    let mut out = CommitsMidExport {
        out: Vec::new(),
        commit: Some(Box::new(|| {
            write_minute(&w, h.id, &l, T0 + 30 * MIN);
            w.flush().unwrap();
        })),
    };
    let s = r.export_csv(&q, &mut out).unwrap();
    assert_eq!(s.rows, 10, "the minute committed mid-export is not in it");
    assert!(
        !String::from_utf8(out.out)
            .unwrap()
            .contains(&(T0 + 30 * MIN).to_string())
    );
    // The next export sees it.
    assert_eq!(r.export_csv(&q, &mut Vec::new()).unwrap().rows, 11);
    assert!(!r.in_transaction());
}

/// A disk that fills up as the export writes.
struct FailingOut;

impl std::io::Write for FailingOut {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("disk full"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_failed_export_leaves_no_transaction_open() {
    let dir = temp_dir("export-fails-snapshot");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&SERIES);
    write_minute(&w, h.id, &l, T0);
    w.flush().unwrap();
    let q = ExportQuery {
        host: h.id,
        selectors: all(),
        from_ms: T0,
        to_ms: T0 + HOUR,
        tier: TierChoice::Fixed(Tier::M1),
    };
    let mut r = store.reader().unwrap();
    let err = r.export_csv(&q, &mut FailingOut).unwrap_err();
    assert!(matches!(err, StoreError::Io(_)), "{err:?}");
    assert!(!r.in_transaction(), "the snapshot is released");
    // So the same reader sees what is committed afterwards.
    write_minute(&w, h.id, &l, T0 + MIN);
    w.flush().unwrap();
    assert_eq!(r.export_csv(&q, &mut Vec::new()).unwrap().rows, 2);
}

#[test]
fn a_bucket_without_a_selected_value_is_not_a_row() {
    let dir = temp_dir("export-empty-slot");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    let h = host(1);
    w.upsert_host(h.clone()).unwrap();
    let l = layout(&SERIES);
    write_minute(&w, h.id, &l, T0);
    // cpu.total not sampled in the second minute; the other series were.
    let mut b = bucket(h.id, Tier::M1, T0 + MIN, &l, 5.0);
    b.stats[0..3].copy_from_slice(&[f32::NAN; 3]);
    w.write_bucket(b).unwrap();
    write_minute(&w, h.id, &l, T0 + 2 * MIN);
    w.flush().unwrap();
    let mut r = store.reader().unwrap();
    let mut out = Vec::new();
    let s = r
        .export_csv(
            &ExportQuery {
                host: h.id,
                selectors: vec![sel("cpu.total")],
                from_ms: T0,
                to_ms: T0 + HOUR,
                tier: TierChoice::Fixed(Tier::M1),
            },
            &mut out,
        )
        .unwrap();
    assert_eq!(s.rows, 2);
    let csv = String::from_utf8(out).unwrap();
    assert!(!csv.contains(&(T0 + MIN).to_string()), "{csv}");
}

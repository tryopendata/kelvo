//! `export_csv`: a range of history written as CSV while it is read, so a 30-day export
//! never holds the range in memory. Rows come off SQLite in bucket order, one statement
//! per table, and each finished bucket is written at once. The whole export reads inside
//! one read transaction, so every statement sees the same snapshot: a commit or a
//! roll-down while it runs can neither add rows the header does not cover nor move
//! minutes out from under it.
//!
//! Format (v1-local-monitor.md, phase 1.1-C; D-079): a header row, then one row per
//! bucket that has a value: `time_utc` (ISO 8601), `time_ms`, then `<series>_avg`,
//! `<series>_min`, `<series>_max` for every series that matched a selector (in key
//! order), then `gap_reason` and `gap_end_ms`. A series with no value in a bucket has
//! empty cells, never zero; a bucket where no selected series has a value is not written.
//! Each gap overlapping the range is a row of its own at its start (or the range start),
//! with empty value cells, its reason (`module_disabled:<module>` for a switched-off
//! module) and its end (empty while it is still open). Nothing is interpolated.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;
use std::iter::Peekable;

use kelvo_schema::{Gap, HostId, SeriesKey, SeriesSelector, Tier, floor_to};
use rusqlite::params;

use super::{Acc, Reader, f32_at, load_layout};
use crate::db::{bucket_sources, tier_width};
use crate::error::{Result, StoreError};
use crate::types::TierChoice;

/// What [`Reader::export_csv`] writes.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportQuery {
    pub host: HostId,
    pub selectors: Vec<SeriesSelector>,
    /// Inclusive start, ms epoch.
    pub from_ms: i64,
    /// Exclusive end, ms epoch.
    pub to_ms: i64,
    /// `Auto` picks the tier as a history read does. An `M15` export folds the minutes
    /// still in `tier_1m` into their 15-minute rows, weighed by width (D-076).
    pub tier: TierChoice,
}

/// What [`Reader::export_csv`] wrote.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportSummary {
    pub tier: Tier,
    /// The width of each row's bucket, ms.
    pub bucket_ms: i64,
    /// The series columns, in order.
    pub series: Vec<SeriesKey>,
    /// Bucket rows written (not counting the header or gap rows).
    pub rows: u64,
    pub gap_rows: u64,
}

/// `(bucket_ts, layout_id, blob, weight)`.
type Bucket = (i64, u32, Vec<u8>, u32);
type BucketItem = rusqlite::Result<Bucket>;
type Stream<'a> = Peekable<Box<dyn Iterator<Item = BucketItem> + 'a>>;

impl Reader {
    /// Writes the range as CSV to `out` (see the module docs) and returns what it wrote.
    /// Memory use is one bucket's worth of values, whatever the range. Reads one
    /// snapshot: rows committed after the export starts are not in it. Write errors come
    /// back as [`StoreError::Io`]; `out` may then hold a partial file.
    pub fn export_csv(&mut self, q: &ExportQuery, out: &mut impl Write) -> Result<ExportSummary> {
        // Deferred: the snapshot starts at the first read and holds until the end.
        self.conn.execute_batch("BEGIN")?;
        let res = self.export_in_snapshot(q, out);
        let end = self
            .conn
            .execute_batch(if res.is_ok() { "COMMIT" } else { "ROLLBACK" });
        if end.is_err() && self.in_transaction() {
            // A reader left inside a transaction would pin its snapshot (and hold back
            // WAL checkpoints) for as long as it sits in a pool. Callers that pool readers
            // also check `in_transaction` before returning one.
            let _ = self.conn.execute_batch("ROLLBACK");
        }
        let summary = res?;
        end?;
        Ok(summary)
    }

    fn export_in_snapshot(
        &mut self,
        q: &ExportQuery,
        out: &mut impl Write,
    ) -> Result<ExportSummary> {
        let host_ref = self.host_ref(q.host)?;
        let tier = match q.tier {
            TierChoice::Fixed(t) => t,
            TierChoice::Auto => self.auto_tier(host_ref, q.from_ms, q.to_ms)?,
        };
        let width = tier_width(tier)?;
        let gaps = if q.to_ms > q.from_ms {
            self.gaps_in(host_ref, q.from_ms, q.to_ms)?
        } else {
            Vec::new()
        };
        // A bucket counts when it starts in the range, as in a history read.
        let base = floor_to(q.from_ms, width);
        let tables = bucket_sources(tier)?;

        // Pass 1: the layouts in range, so the header names every column before the
        // first row. The `(host_id, bucket_ts, layout_id)` key covers this scan.
        let mut layout_ids = BTreeSet::new();
        if q.to_ms > q.from_ms && !q.selectors.is_empty() {
            for (table, _) in tables {
                let mut stmt = self.conn.prepare_cached(&format!(
                    "SELECT DISTINCT layout_id FROM {table}
                     WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3"
                ))?;
                let ids = stmt.query_map(params![host_ref, base, q.to_ms], |r| r.get(0))?;
                for id in ids {
                    layout_ids.insert(id?);
                }
            }
        }
        let mut layouts = Vec::new();
        let mut keys = BTreeSet::new();
        for &id in &layout_ids {
            let layout = load_layout(&self.conn, &mut self.layouts, id)?;
            keys.extend(
                layout
                    .iter()
                    .filter(|k| q.selectors.iter().any(|s| s.matches(k)))
                    .cloned(),
            );
            layouts.push((id, layout));
        }
        let column: BTreeMap<&SeriesKey, usize> =
            keys.iter().enumerate().map(|(i, k)| (k, i)).collect();
        // Per layout: (column, position in the layout) of each matching series.
        let matches: HashMap<u32, Vec<(usize, usize)>> = layouts
            .iter()
            .map(|(id, layout)| {
                let m = layout
                    .iter()
                    .enumerate()
                    .filter_map(|(pos, k)| column.get(k).map(|&c| (c, pos)))
                    .collect();
                (*id, m)
            })
            .collect();
        let series: Vec<SeriesKey> = keys.into_iter().collect();

        let mut header = String::from("time_utc,time_ms");
        for key in &series {
            let name = key.to_string();
            for stat in ["avg", "min", "max"] {
                header.push(',');
                header.push_str(&csv_field(&format!("{name}_{stat}")));
            }
        }
        header.push_str(",gap_reason,gap_end_ms\n");
        out.write_all(header.as_bytes())?;

        let mut w = RowWriter {
            out,
            gaps: gaps.iter().peekable(),
            from_ms: q.from_ms,
            columns: series.len(),
            rows: 0,
            gap_rows: 0,
            line: String::new(),
        };

        // Pass 2: buckets in time order, merged across tables, one slot at a time.
        if !matches.is_empty() {
            let mut stmts = Vec::new();
            for (table, weight) in tables {
                let stmt = self.conn.prepare(&format!(
                    "SELECT bucket_ts, layout_id, blob FROM {table}
                     WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3
                     ORDER BY bucket_ts"
                ))?;
                stmts.push((stmt, *weight));
            }
            let mut streams: Vec<Stream<'_>> = Vec::new();
            for (stmt, weight) in &mut stmts {
                let weight = *weight;
                let rows = stmt.query_map(params![host_ref, base, q.to_ms], move |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, weight))
                })?;
                let rows: Box<dyn Iterator<Item = BucketItem> + '_> = Box::new(rows);
                streams.push(rows.peekable());
            }

            let mut slot: Option<i64> = None;
            let mut accs: Vec<Acc> = (0..series.len()).map(|_| Acc::default()).collect();
            while let Some((ts, layout_id, stats, weight)) = next_in_order(&mut streams)? {
                let t = floor_to(ts, width);
                if let Some(s) = slot
                    && s != t
                {
                    w.bucket(s, &mut accs)?;
                }
                slot = Some(t);
                let Some(positions) = matches.get(&layout_id) else {
                    continue;
                };
                for &(c, pos) in positions {
                    let (Some(min), Some(max), Some(avg)) = (
                        f32_at(&stats, pos * 3),
                        f32_at(&stats, pos * 3 + 1),
                        f32_at(&stats, pos * 3 + 2),
                    ) else {
                        return Err(StoreError::Corrupt(format!(
                            "bucket at {ts} is shorter than layout {layout_id}"
                        )));
                    };
                    if avg.is_nan() {
                        continue;
                    }
                    if let Some(a) = accs.get_mut(c) {
                        a.add(min, max, avg, weight);
                    }
                }
            }
            if let Some(s) = slot {
                w.bucket(s, &mut accs)?;
            }
        }
        w.gaps_until(i64::MAX)?;
        let (rows, gap_rows) = (w.rows, w.gap_rows);
        Ok(ExportSummary {
            tier,
            bucket_ms: width,
            series,
            rows,
            gap_rows,
        })
    }
}

/// The next row across `streams`, earliest bucket first. A stream whose next item is an
/// error goes first, so the error surfaces.
fn next_in_order(streams: &mut [Stream<'_>]) -> Result<Option<Bucket>> {
    let mut best: Option<(usize, i64)> = None;
    for (i, s) in streams.iter_mut().enumerate() {
        let ts = match s.peek() {
            None => continue,
            Some(Ok(row)) => row.0,
            Some(Err(_)) => i64::MIN,
        };
        if best.is_none_or(|(_, b)| ts < b) {
            best = Some((i, ts));
        }
    }
    let Some((i, _)) = best else {
        return Ok(None);
    };
    match streams.get_mut(i).and_then(Iterator::next) {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

struct RowWriter<'a, 'g, W: Write> {
    out: &'a mut W,
    gaps: Peekable<std::slice::Iter<'g, Gap>>,
    from_ms: i64,
    columns: usize,
    rows: u64,
    gap_rows: u64,
    /// Reused for every line.
    line: String,
}

impl<W: Write> RowWriter<'_, '_, W> {
    /// Writes the gap rows that start at or before `t`, in order.
    fn gaps_until(&mut self, t: i64) -> Result<()> {
        use std::fmt::Write as _;
        while let Some(gap) = self.gaps.next_if(|g| g.start_ms.max(self.from_ms) <= t) {
            let at = gap.start_ms.max(self.from_ms);
            self.line.clear();
            push_time(&mut self.line, at);
            for _ in 0..self.columns {
                self.line.push_str(",,,");
            }
            self.line.push(',');
            self.line.push_str(gap.reason.as_str());
            if let Some(module) = gap.module {
                self.line.push(':');
                self.line.push_str(module.as_str());
            }
            self.line.push(',');
            if let Some(end) = gap.end_ms {
                let _ = write!(self.line, "{end}");
            }
            self.line.push('\n');
            self.out.write_all(self.line.as_bytes())?;
            self.gap_rows += 1;
        }
        Ok(())
    }

    /// Writes the bucket at `t` from `accs` and resets them. A bucket where no selected
    /// series has a value (rows of other series only, or all NaN) is skipped.
    fn bucket(&mut self, t: i64, accs: &mut [Acc]) -> Result<()> {
        use std::fmt::Write as _;
        if accs.iter().all(|a| a.weight == 0) {
            return Ok(());
        }
        self.gaps_until(t)?;
        self.line.clear();
        push_time(&mut self.line, t);
        for a in accs.iter_mut() {
            if a.weight == 0 {
                self.line.push_str(",,,");
            } else {
                let avg = (a.sum / f64::from(a.weight)) as f32;
                // `{}` on f32 is the shortest text that parses back to the same value.
                let _ = write!(self.line, ",{avg},{},{}", a.min, a.max);
            }
            *a = Acc::default();
        }
        self.line.push_str(",,\n");
        self.out.write_all(self.line.as_bytes())?;
        self.rows += 1;
        Ok(())
    }
}

/// `time_utc,time_ms` for `ms`.
fn push_time(line: &mut String, ms: i64) {
    use std::fmt::Write as _;
    let _ = write!(line, "{},{ms}", iso_utc(ms));
}

/// `ms` as ISO 8601 UTC to the second (`2026-10-04T14:02:00Z`). `time_ms` carries the
/// exact value.
pub(crate) fn iso_utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// A CSV field, quoted when it holds a comma, quote or line break (RFC 4180).
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_utc_formats_known_instants() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_788_220_800_000), "2026-09-01T00:00:00Z");
        // A leap day, and a time of day.
        assert_eq!(iso_utc(951_782_400_000 + 3_723_000), "2000-02-29T01:02:03Z");
        assert_eq!(iso_utc(-1000), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn fields_with_commas_or_quotes_are_quoted() {
        assert_eq!(csv_field("cpu.total_avg"), "cpu.total_avg");
        assert_eq!(csv_field("net.rx{a=1,b=2}_avg"), "\"net.rx{a=1,b=2}_avg\"");
        assert_eq!(csv_field("x\"y"), "\"x\"\"y\"");
    }
}

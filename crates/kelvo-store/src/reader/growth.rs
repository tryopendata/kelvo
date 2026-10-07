//! `history_growth`: what history costs on this machine, measured from the file.
//!
//! The Settings projection ("30 days, about 80 MB") used to come only from synthetic
//! fill tests, which overstate a real host (more processes per snapshot, values that do
//! not compress). This measures the bytes each table holds now against how long Kelvo
//! has recorded within that table's retention, and scales each to a full retention. The
//! frontend keeps its model (a fixed part plus a cost per day of minutes, D-076) and
//! swaps these numbers in for the fill test's once enough has been recorded.

use std::collections::HashMap;

use rusqlite::params;

use super::Reader;
use crate::error::Result;
use crate::types::Retention;

/// Recorded time below which a measurement is not offered: 16 KiB pages and the first
/// rows of each table make a few minutes of history unrepresentative.
pub const MIN_MEASURED_MS: i64 = 3_600_000;

/// Objects with at least this many pages set the page fill: smaller ones are mostly
/// their one partly filled page.
const FILL_MIN_PAGES: i64 = 16;

/// Tables kept for [`Retention::s10_ms`].
const S10_TABLES: &[&str] = &["tier_10s"];
/// Tables kept for [`Retention::proc_snap_ms`].
const SNAP_TABLES: &[&str] = &["proc_snap", "proc_net_10s"];
/// Tables kept for [`Retention::M1_WINDOW_MS`], then rolled into 15-minute rows.
const MINUTE_TABLES: &[&str] = &["tier_1m", "proc_top_1m", "proc_net_1m"];
/// The 15-minute tables; the projection costs them from the minute tables (a fifteenth).
const QUARTER_TABLES: &[&str] = &["tier_15m", "proc_top_15m", "proc_net_15m"];

/// History cost per unit of recorded time on this machine, scaled to full retentions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryGrowth {
    /// Minutes Kelvo recorded in the last 7 days, in ms: what the rates are measured over.
    pub measured_ms: i64,
    /// Bytes that do not depend on retention once full: 24 h of 10 s rows and the
    /// process snapshot window, scaled to full, plus the small tables (series, layouts,
    /// names, gaps, events) at their current size.
    pub fixed_bytes: u64,
    /// Bytes one day of 1-minute history costs.
    pub minute_day_bytes: u64,
}

/// One table's pages in `dbstat`.
struct TablePages {
    pages: i64,
    /// Bytes of its pages.
    bytes: u64,
    /// Bytes its rows and page headers use on those pages.
    used: u64,
}

impl Reader {
    /// [`HistoryGrowth`] at `now_ms` for the whole file, every host together, or `None`
    /// until each table's retention window holds [`MIN_MEASURED_MS`] of recording: after
    /// a relaunch the 10 s window may hold a few minutes, and scaling those to a day
    /// would multiply noise (the minute in progress has 10 s rows and no minute yet).
    /// Assumes Kelvo records around the clock, as the projection does. Reads every page
    /// once (`dbstat`), so call it when Settings asks, not per tick.
    pub fn history_growth(
        &mut self,
        now_ms: i64,
        retention: &Retention,
    ) -> Result<Option<HistoryGrowth>> {
        let minute_window = Retention::M1_WINDOW_MS;
        let measured_ms = self.recorded_ms(now_ms - minute_window)?;
        let s10_recorded = self.recorded_ms(now_ms - retention.s10_ms)?;
        let snap_recorded = self.recorded_ms(now_ms - retention.proc_snap_ms)?;
        if measured_ms.min(s10_recorded).min(snap_recorded) < MIN_MEASURED_MS {
            return Ok(None);
        }
        let tables = self.table_bytes()?;
        // A young table is a page or two, mostly empty, and scaling its pages up would
        // count that empty space many times over. Growing tables are costed by the bytes
        // their rows use, at the page fill the larger tables show.
        let (big_used, big_pages) = tables
            .values()
            .filter(|t| t.pages >= FILL_MIN_PAGES)
            .fold((0u64, 0u64), |(u, b), t| (u + t.used, b + t.bytes));
        let fill = if big_used > 0 {
            big_used as f64 / big_pages as f64
        } else {
            1.0
        };
        let sum = |names: &[&str]| -> u64 {
            names
                .iter()
                .filter_map(|n| tables.get(*n))
                .map(|t| (t.used as f64 / fill) as u64)
                .sum()
        };
        // Bytes per recorded ms within `window`, times the window.
        let full = |tables: &[&str], window: i64, recorded: i64| -> u64 {
            let scale = window as f64 / recorded.min(window) as f64;
            (sum(tables) as f64 * scale) as u64
        };
        let s10 = full(S10_TABLES, retention.s10_ms, s10_recorded);
        let snap = full(SNAP_TABLES, retention.proc_snap_ms, snap_recorded);
        let classed_pages: u64 = [S10_TABLES, SNAP_TABLES, MINUTE_TABLES, QUARTER_TABLES]
            .iter()
            .flat_map(|names| names.iter())
            .filter_map(|n| tables.get(*n))
            .map(|t| t.bytes)
            .sum();
        let other = tables
            .values()
            .map(|t| t.bytes)
            .sum::<u64>()
            .saturating_sub(classed_pages);
        let minute_day_bytes = (sum(MINUTE_TABLES) as f64 * Retention::DAY_MS as f64
            / measured_ms.min(minute_window) as f64) as u64;
        Ok(Some(HistoryGrowth {
            measured_ms,
            fixed_bytes: other + s10 + snap,
            minute_day_bytes,
        }))
    }

    /// Time with minute history at or after `from_ms`, across hosts: a minute counts once
    /// however many hosts or layouts wrote it.
    fn recorded_ms(&self, from_ms: i64) -> Result<i64> {
        let minutes: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT bucket_ts) FROM tier_1m WHERE bucket_ts >= ?1",
            params![from_ms],
            |r| r.get(0),
        )?;
        Ok(minutes * 60_000)
    }

    /// Pages per table, its indexes included. Free pages are not counted.
    fn table_bytes(&self) -> Result<HashMap<String, TablePages>> {
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(s.tbl_name, d.name), COUNT(*), SUM(d.pgsize),
                    SUM(d.pgsize - d.unused)
             FROM dbstat AS d LEFT JOIN sqlite_schema AS s ON s.name = d.name
             GROUP BY 1",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                TablePages {
                    pages: r.get(1)?,
                    bytes: u64::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
                    used: u64::try_from(r.get::<_, i64>(3)?).unwrap_or(0),
                },
            ))
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }
}

//! Pruning: retention deletes in batches, roll-downs into coarser tables, and the byte
//! cap trim.

use std::collections::{BTreeMap, HashMap};

use kelvo_schema::Tier;
use rusqlite::params;

use super::State;
use crate::blob::{self, PackedProc};
use crate::db;
use crate::error::Result;
use crate::rolldown::{NetFold, StatsFold};
use crate::types::{CapTrim, PruneReport, Retention};

/// Span of process snapshots rolled down per transaction (500 minutes).
const ROLL_DOWN_SNAPSHOTS_MS: i64 = 500 * 60_000;
/// Span of minute rows (buckets or process top 5) rolled into 15-minute rows per
/// transaction: a day, 1,440 rows per layout.
const ROLL_DOWN_MINUTES_MS: i64 = Retention::DAY_MS;
/// Processes kept per minute in `proc_top_1m`.
const TOP_PER_MINUTE: usize = 5;
/// The byte cap never trims the most recent day: below that, history is not worth having
/// and the cap is reported unmet instead.
const CAP_TRIM_KEEP_MS: i64 = Retention::DAY_MS;
/// Smallest slice of history one cap-trim round removes.
const CAP_TRIM_MIN_STEP_MS: i64 = 3_600_000;
/// Bound on cap-trim rounds per prune; each round re-measures the file.
const CAP_TRIM_MAX_ROUNDS: usize = 64;

impl State {
    // --- pruning --------------------------------------------------------------------------

    fn host_refs(&self) -> Result<Vec<i64>> {
        Ok(self
            .conn
            .prepare("SELECT id FROM hosts")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub(super) fn prune(&mut self, now: i64, retention: Retention) -> Result<PruneReport> {
        self.commit()?;
        let mut report = PruneReport::default();
        let history_cut = now - retention.history_ms;
        // Whole 15-minute buckets only, so a bucket is never split across two passes.
        let roll_cut = Tier::M15
            .bucket_start(now - retention.m1_keep_ms())
            .unwrap_or(now - retention.m1_keep_ms());
        for host_ref in self.host_refs()? {
            let snaps = self.roll_down(
                host_ref,
                "proc_snap",
                "ts",
                Tier::M1,
                now - retention.proc_snap_ms,
                ROLL_DOWN_SNAPSHOTS_MS,
                |s, start, end| s.roll_down_snapshots(host_ref, start, end),
            )?;
            report.proc_top_rows += snaps.written;
            report.proc_snaps_rolled += snaps.rolled;

            // Whole minutes, so a minute's `proc_net_1m` row never outlives part of the
            // 10 s rows it was summed from (reads take one or the other per minute).
            let net_s10_cut = now - retention.proc_snap_ms.min(retention.history_ms);
            let net_s10_cut = Tier::M1.bucket_start(net_s10_cut).unwrap_or(net_s10_cut);
            let (n, _) = self.delete_batched("proc_net_10s", "bucket_ts", host_ref, net_s10_cut)?;
            report.net_rows += n;

            let s10_cut = now - retention.s10_ms;
            let (n, max) = self.delete_batched("tier_10s", "bucket_ts", host_ref, s10_cut)?;
            report.s10_rows += n;
            self.mark_pruned(host_ref, Tier::S10, max, s10_cut)?;

            // Past retention first, so the roll-down never folds rows about to go.
            let (m1, a) = self.delete_batched("tier_1m", "bucket_ts", host_ref, history_cut)?;
            let (m15, d) = self.delete_batched("tier_15m", "bucket_ts", host_ref, history_cut)?;
            // Only closed gaps that ended before the cutoff.
            let (gaps, b) = self.delete_batched("gaps", "end_ts", host_ref, history_cut)?;
            let (events, c) = self.delete_batched("events", "ts", host_ref, history_cut)?;
            let (top, _) =
                self.delete_batched("proc_top_1m", "bucket_ts", host_ref, history_cut)?;
            let (top15, _) =
                self.delete_batched("proc_top_15m", "bucket_ts", host_ref, history_cut)?;
            let (net1, _) =
                self.delete_batched("proc_net_1m", "bucket_ts", host_ref, history_cut)?;
            let (net15, _) =
                self.delete_batched("proc_net_15m", "bucket_ts", host_ref, history_cut)?;
            report.net_rows += net1 + net15;
            report.m1_rows += m1;
            report.m15_rows += m15;
            report.gaps += gaps;
            report.events += events;
            report.proc_top_rows += top + top15;
            self.mark_pruned(host_ref, Tier::M1, a.max(b).max(c), history_cut)?;
            self.mark_pruned(host_ref, Tier::M15, d, history_cut)?;

            let minutes = self.roll_down(
                host_ref,
                "tier_1m",
                "bucket_ts",
                Tier::M15,
                roll_cut,
                ROLL_DOWN_MINUTES_MS,
                |s, start, end| s.roll_down_minutes(host_ref, start, end),
            )?;
            report.m15_written += minutes.written;
            report.m1_rolled += minutes.rolled;
            self.mark(host_ref, db::M1_ROLLED, minutes.max_seq, roll_cut)?;

            let tops = self.roll_down(
                host_ref,
                "proc_top_1m",
                "bucket_ts",
                Tier::M15,
                roll_cut,
                ROLL_DOWN_MINUTES_MS,
                |s, start, end| s.roll_down_top_minutes(host_ref, start, end),
            )?;
            report.proc_top_rolled += tops.rolled;

            let nets = self.roll_down(
                host_ref,
                "proc_net_1m",
                "bucket_ts",
                Tier::M15,
                roll_cut,
                ROLL_DOWN_MINUTES_MS,
                |s, start, end| s.roll_down_net_minutes(host_ref, start, end),
            )?;
            report.net_rolled += nets.rolled;
        }
        self.incremental_vacuum()?;
        self.checkpoint_truncate()?;
        let size = crate::size_on_disk(&self.path)?;
        if size > retention.max_bytes {
            report.cap_trim = Some(self.trim_to_cap(now, retention, size)?);
        }
        report.size_bytes = crate::size_on_disk(&self.path)?;
        Ok(report)
    }

    /// Deletes the oldest history on every host, a slice at a time, until the live pages
    /// fit in the low-water mark or only the last day is left. Each slice goes from the
    /// start of history to a cutoff, so the trimmed span is "before history began", never
    /// a hole in the middle; the `pruned` marks move to the cutoff so a cursor behind it
    /// gets `Truncated` (D-041).
    fn trim_to_cap(&mut self, now: i64, retention: Retention, size_before: u64) -> Result<CapTrim> {
        let low_water = retention.low_water_bytes();
        let floor = Tier::M15
            .bucket_start(now - CAP_TRIM_KEEP_MS)
            .unwrap_or(now - CAP_TRIM_KEEP_MS);
        let mut trim = CapTrim {
            size_before,
            ..CapTrim::default()
        };
        let mut used = self.used_bytes()?;
        for _ in 0..CAP_TRIM_MAX_ROUNDS {
            if used <= low_water {
                break;
            }
            let (oldest, newest): (Option<i64>, Option<i64>) = self.conn.query_row(
                "SELECT min(t), max(t) FROM (
                   SELECT min(bucket_ts) AS t FROM tier_1m UNION ALL
                   SELECT max(bucket_ts) FROM tier_1m UNION ALL
                   SELECT min(bucket_ts) FROM tier_15m UNION ALL
                   SELECT max(bucket_ts) FROM tier_15m
                 )",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let (Some(oldest), Some(newest)) = (oldest, newest) else {
                break;
            };
            trim.earliest_ts_ms = trim.earliest_ts_ms.max(oldest);
            // Remove the share of the span that is over the mark. It undershoots: S10,
            // snapshots and the recent minutes do not shrink with it, and the oldest span
            // is 15-minute rows, which cost a fifteenth of minutes. That only costs more
            // rounds.
            let span = (newest - oldest).max(0) + 60_000;
            let excess = used - low_water;
            let share = i128::from(span) * i128::from(excess) / i128::from(used.max(1));
            let step = i64::try_from(share)
                .unwrap_or(span)
                .max(CAP_TRIM_MIN_STEP_MS);
            let cutoff = Tier::M15
                .bucket_start(oldest + step)
                .unwrap_or(oldest + step)
                .min(floor);
            if cutoff <= oldest {
                break;
            }
            for host_ref in self.host_refs()? {
                let (m15, e) = self.delete_batched("tier_15m", "bucket_ts", host_ref, cutoff)?;
                let (m1, a) = self.delete_batched("tier_1m", "bucket_ts", host_ref, cutoff)?;
                let (gaps, b) = self.delete_batched("gaps", "end_ts", host_ref, cutoff)?;
                let (events, c) = self.delete_batched("events", "ts", host_ref, cutoff)?;
                let (top15, _) =
                    self.delete_batched("proc_top_15m", "bucket_ts", host_ref, cutoff)?;
                let (top, _) = self.delete_batched("proc_top_1m", "bucket_ts", host_ref, cutoff)?;
                let (s10, d) = self.delete_batched("tier_10s", "bucket_ts", host_ref, cutoff)?;
                let (snaps, _) = self.delete_batched("proc_snap", "ts", host_ref, cutoff)?;
                for table in ["proc_net_15m", "proc_net_1m", "proc_net_10s"] {
                    let (n, _) = self.delete_batched(table, "bucket_ts", host_ref, cutoff)?;
                    trim.net_rows += n;
                }
                self.mark_pruned(host_ref, Tier::M1, a.max(b).max(c), cutoff)?;
                self.mark_pruned(host_ref, Tier::M15, e, cutoff)?;
                self.mark_pruned(host_ref, Tier::S10, d, cutoff)?;
                trim.m1_rows += m1;
                trim.m15_rows += m15;
                trim.gaps += gaps;
                trim.events += events;
                trim.proc_rows += top + top15 + snaps;
                trim.s10_rows += s10;
            }
            trim.earliest_ts_ms = cutoff;
            self.incremental_vacuum()?;
            used = self.used_bytes()?;
        }
        trim.cap_met = used <= low_water;
        self.checkpoint_truncate()?;
        Ok(trim)
    }

    /// Deletes rows of `table` with `ts_col < cutoff` in batches, one transaction each.
    /// Returns how many went and the highest `seq` among them.
    fn delete_batched(
        &mut self,
        table: &str,
        ts_col: &str,
        host_ref: i64,
        cutoff: i64,
    ) -> Result<(u64, Option<i64>)> {
        let batch = self.prune_batch;
        let sql = format!(
            "DELETE FROM {table} WHERE rowid IN (
               SELECT rowid FROM {table} WHERE host_id = ?1 AND {ts_col} < ?2 LIMIT {batch}
             ) RETURNING seq"
        );
        let (mut total, mut max_seq) = (0, None);
        loop {
            let (n, max) = self.apply(|s| {
                let mut stmt = s.conn.prepare_cached(&sql)?;
                let mut rows = stmt.query(params![host_ref, cutoff])?;
                let (mut n, mut max) = (0u64, None::<i64>);
                while let Some(r) = rows.next()? {
                    n += 1;
                    max = max.max(Some(r.get(0)?));
                }
                Ok((n, max))
            })?;
            self.commit()?;
            total += n;
            max_seq = max_seq.max(max);
            if n < batch {
                return Ok((total, max_seq));
            }
            self.yield_to_queue()?;
        }
    }

    fn mark_pruned(&mut self, host_ref: i64, tier: Tier, seq: Option<i64>, ts: i64) -> Result<()> {
        self.mark(host_ref, tier.as_str(), seq, ts)
    }

    /// Moves the `pruned` row `key` of `host_ref` forward to `(seq, ts)`; never back.
    fn mark(&mut self, host_ref: i64, key: &str, seq: Option<i64>, ts: i64) -> Result<()> {
        let Some(seq) = seq else {
            return Ok(());
        };
        self.apply(|s| {
            s.conn.execute(
                "INSERT INTO pruned (host_id, tier, seq, ts) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (host_id, tier) DO UPDATE
                   SET seq = max(seq, excluded.seq), ts = max(ts, excluded.ts)",
                params![host_ref, key, seq, ts],
            )?;
            Ok(())
        })?;
        self.commit()
    }

    /// Rolls the rows of `table` older than `cutoff` into a coarser table, oldest first,
    /// `span_ms` of them per transaction, handling queued work between transactions.
    /// `cutoff` and each span start are aligned to `bucket`'s width, so a coarse bucket is
    /// never split across two passes. `chunk` folds and deletes the rows of one span; it
    /// must delete every row of `table` in it, or this would not end.
    #[allow(clippy::too_many_arguments)] // Each is one knob of the roll-down; a struct would only rename them.
    fn roll_down(
        &mut self,
        host_ref: i64,
        table: &str,
        ts_col: &str,
        bucket: Tier,
        cutoff: i64,
        span_ms: i64,
        mut chunk: impl FnMut(&mut State, i64, i64) -> Result<Rolled>,
    ) -> Result<Rolled> {
        let cutoff = bucket.bucket_start(cutoff).unwrap_or(cutoff);
        let mut total = Rolled::default();
        loop {
            let first: Option<i64> = self.conn.query_row(
                &format!("SELECT min({ts_col}) FROM {table} WHERE host_id = ?1 AND {ts_col} < ?2"),
                params![host_ref, cutoff],
                |r| r.get(0),
            )?;
            let Some(first) = first else {
                return Ok(total);
            };
            let start = bucket.bucket_start(first).unwrap_or(first);
            let end = (start + span_ms).min(cutoff);
            let done = self.apply(|s| chunk(s, start, end))?;
            self.commit()?;
            total.written += done.written;
            total.rolled += done.rolled;
            total.max_seq = total.max_seq.max(done.max_seq);
            self.yield_to_queue()?;
        }
    }

    /// Rolls the `proc_snap` rows in `[start, end)` into `proc_top_1m` (top 5 per minute
    /// by mean CPU) and deletes them.
    fn roll_down_snapshots(&mut self, host_ref: i64, start: i64, end: i64) -> Result<Rolled> {
        let snaps: Vec<(i64, Vec<u8>)> = self
            .conn
            .prepare_cached(
                "SELECT ts, blob FROM proc_snap WHERE host_id = ?1 AND ts >= ?2 AND ts < ?3 ORDER BY ts",
            )?
            .query_map(params![host_ref, start, end], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut by_minute: BTreeMap<i64, Vec<Vec<PackedProc>>> = BTreeMap::new();
        for (ts, b) in &snaps {
            let minute = Tier::M1.bucket_start(*ts).unwrap_or(*ts);
            by_minute
                .entry(minute)
                .or_default()
                .push(blob::unpack_procs(b)?);
        }
        let mut written = 0;
        for (minute, group) in by_minute {
            let top = top_of_group(&group);
            let seq = self.alloc_seq();
            self.conn
                .prepare_cached(
                    "INSERT INTO proc_top_1m (host_id, bucket_ts, seq, blob) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (host_id, bucket_ts) DO UPDATE SET seq = excluded.seq, blob = excluded.blob",
                )?
                .execute(params![host_ref, minute, seq, blob::pack_procs(&top)])?;
            written += 1;
        }
        let deleted = self
            .conn
            .prepare_cached("DELETE FROM proc_snap WHERE host_id = ?1 AND ts >= ?2 AND ts < ?3")?
            .execute(params![host_ref, start, end])?;
        Ok(Rolled {
            written,
            rolled: deleted as u64,
            max_seq: None,
        })
    }

    /// Rolls the `tier_1m` rows in `[start, end)` (15-minute boundaries) into `tier_15m`,
    /// one row per 15-minute bucket and layout ([`StatsFold`]), and deletes them. A bucket
    /// that already has a row for the layout keeps it: buckets are rolled whole, so a
    /// second fold could only come from minutes that arrived after the first one, and
    /// would replace a full bucket with a fraction of it. A row whose length does not fit
    /// its layout is dropped with a warning rather than stopping every later prune.
    fn roll_down_minutes(&mut self, host_ref: i64, start: i64, end: i64) -> Result<Rolled> {
        let rows: Vec<(i64, u32, Vec<u8>)> = self
            .conn
            .prepare_cached(
                "SELECT bucket_ts, layout_id, blob FROM tier_1m
                 WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3 ORDER BY bucket_ts",
            )?
            .query_map(params![host_ref, start, end], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut folds: BTreeMap<(i64, u32), StatsFold> = BTreeMap::new();
        for (ts, layout, b) in &rows {
            let stats = match blob::unpack_f32s(b) {
                Ok(v) if v.len().is_multiple_of(3) => v,
                _ => {
                    tracing::warn!(ts, layout, "store: malformed minute row not rolled down");
                    continue;
                }
            };
            let bucket = Tier::M15.bucket_start(*ts).unwrap_or(*ts);
            let fold = folds
                .entry((bucket, *layout))
                .or_insert_with(|| StatsFold::new(stats.len() / 3));
            if !fold.add(&stats) {
                tracing::warn!(
                    ts,
                    layout,
                    "store: minute row of another width not rolled down"
                );
            }
        }
        let mut written = 0;
        for ((bucket, layout), fold) in folds {
            let seq = self.alloc_seq();
            let n = self
                .conn
                .prepare_cached(
                    "INSERT INTO tier_15m (host_id, bucket_ts, layout_id, seq, blob)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (host_id, bucket_ts, layout_id) DO NOTHING",
                )?
                .execute(params![
                    host_ref,
                    bucket,
                    layout,
                    seq,
                    blob::pack_f32s(&fold.finish())
                ])?;
            if n == 0 {
                self.unalloc_seq();
            }
            written += n as u64;
        }
        let (rolled, max_seq) =
            self.delete_span_returning_seq("tier_1m", "bucket_ts", host_ref, start, end)?;
        Ok(Rolled {
            written,
            rolled,
            max_seq,
        })
    }

    /// Rolls the `proc_top_1m` rows in `[start, end)` (15-minute boundaries) into
    /// `proc_top_15m`, the top 5 by mean CPU over the minutes present, and deletes them. An
    /// existing row is kept, as in [`State::roll_down_minutes`].
    fn roll_down_top_minutes(&mut self, host_ref: i64, start: i64, end: i64) -> Result<Rolled> {
        let rows: Vec<(i64, Vec<u8>)> = self
            .conn
            .prepare_cached(
                "SELECT bucket_ts, blob FROM proc_top_1m
                 WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3 ORDER BY bucket_ts",
            )?
            .query_map(params![host_ref, start, end], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut groups: BTreeMap<i64, Vec<Vec<PackedProc>>> = BTreeMap::new();
        for (ts, b) in &rows {
            let bucket = Tier::M15.bucket_start(*ts).unwrap_or(*ts);
            match blob::unpack_procs(b) {
                Ok(procs) => groups.entry(bucket).or_default().push(procs),
                Err(e) => tracing::warn!(ts, "store: process minute not rolled down: {e}"),
            }
        }
        let mut written = 0;
        for (bucket, group) in groups {
            let seq = self.alloc_seq();
            let n = self
                .conn
                .prepare_cached(
                    "INSERT INTO proc_top_15m (host_id, bucket_ts, seq, blob) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (host_id, bucket_ts) DO NOTHING",
                )?
                .execute(params![
                    host_ref,
                    bucket,
                    seq,
                    blob::pack_procs(&top_of_group(&group))
                ])?;
            if n == 0 {
                self.unalloc_seq();
            }
            written += n as u64;
        }
        let (rolled, max_seq) =
            self.delete_span_returning_seq("proc_top_1m", "bucket_ts", host_ref, start, end)?;
        Ok(Rolled {
            written,
            rolled,
            max_seq,
        })
    }

    /// Rolls the `proc_net_1m` rows in `[start, end)` (15-minute boundaries) into
    /// `proc_net_15m` by exact sums ([`NetFold`]) and deletes them. An existing row is
    /// kept, as in [`State::roll_down_minutes`]; a malformed row is dropped with a warning.
    fn roll_down_net_minutes(&mut self, host_ref: i64, start: i64, end: i64) -> Result<Rolled> {
        let rows: Vec<(i64, Vec<u8>)> = self
            .conn
            .prepare_cached(
                "SELECT bucket_ts, blob FROM proc_net_1m
                 WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3 ORDER BY bucket_ts",
            )?
            .query_map(params![host_ref, start, end], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut folds: BTreeMap<i64, NetFold> = BTreeMap::new();
        for (ts, b) in &rows {
            match blob::unpack_net(b) {
                Ok((header, apps)) => folds
                    .entry(Tier::M15.bucket_start(*ts).unwrap_or(*ts))
                    .or_default()
                    .add(&header, &apps),
                Err(e) => tracing::warn!(ts, "store: network minute not rolled down: {e}"),
            }
        }
        let mut written = 0;
        for (bucket, fold) in folds {
            let (header, apps) = fold.finish();
            let seq = self.alloc_seq();
            let n = self
                .conn
                .prepare_cached(
                    "INSERT INTO proc_net_15m (host_id, bucket_ts, seq, blob) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (host_id, bucket_ts) DO NOTHING",
                )?
                .execute(params![host_ref, bucket, seq, blob::pack_net(&header, &apps)])?;
            if n == 0 {
                self.unalloc_seq();
            }
            written += n as u64;
        }
        let (rolled, max_seq) =
            self.delete_span_returning_seq("proc_net_1m", "bucket_ts", host_ref, start, end)?;
        Ok(Rolled {
            written,
            rolled,
            max_seq,
        })
    }

    /// Deletes `host_ref`'s rows of `table` with `ts_col` in `[start, end)`; returns how
    /// many and the highest `seq` among them.
    fn delete_span_returning_seq(
        &mut self,
        table: &str,
        ts_col: &str,
        host_ref: i64,
        start: i64,
        end: i64,
    ) -> Result<(u64, Option<i64>)> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "DELETE FROM {table} WHERE host_id = ?1 AND {ts_col} >= ?2 AND {ts_col} < ?3
             RETURNING seq"
        ))?;
        let mut rows = stmt.query(params![host_ref, start, end])?;
        let (mut n, mut max) = (0u64, None::<i64>);
        while let Some(r) = rows.next()? {
            n += 1;
            max = max.max(Some(r.get(0)?));
        }
        Ok((n, max))
    }
}

/// What one roll-down pass did.
#[derive(Clone, Copy, Debug, Default)]
struct Rolled {
    /// Coarse rows written.
    written: u64,
    /// Fine rows folded and deleted.
    rolled: u64,
    /// Highest `seq` of the deleted rows, where the caller marks it.
    max_seq: Option<i64>,
}

/// Top processes of one bucket by mean CPU across its member lists: the snapshots of a
/// minute, or the minutes of 15 minutes. A process missing from a list was below that
/// list's cut (top 30 per snapshot, top 5 per minute), and counts as 0 there: an
/// approximation that can only understate it.
fn top_of_group(snapshots: &[Vec<PackedProc>]) -> Vec<PackedProc> {
    let n = snapshots.len().max(1) as f32;
    let mut acc: HashMap<u32, PackedProc> = HashMap::new();
    for snap in snapshots {
        for p in snap {
            let e = acc.entry(p.name_id).or_insert(PackedProc {
                name_id: p.name_id,
                pid: p.pid,
                cpu: 0.0,
                mem_kib: 0,
                threads: 0,
                wakeups: 0.0,
                energy: 0.0,
            });
            e.pid = p.pid;
            e.cpu += p.cpu;
            e.mem_kib = e.mem_kib.max(p.mem_kib);
            e.threads = e.threads.max(p.threads);
            e.wakeups += p.wakeups;
            e.energy += p.energy;
        }
    }
    let mut rows: Vec<PackedProc> = acc
        .into_values()
        .map(|mut p| {
            p.cpu /= n;
            p.wakeups /= n;
            p.energy /= n;
            p
        })
        .collect();
    rows.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(a.name_id.cmp(&b.name_id)));
    rows.truncate(TOP_PER_MINUTE);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(name_id: u32, cpu: f32) -> PackedProc {
        PackedProc {
            name_id,
            pid: name_id as i32,
            cpu,
            mem_kib: 10,
            threads: 1,
            wakeups: 0.0,
            energy: 0.0,
        }
    }

    #[test]
    fn top_of_minute_averages_over_all_snapshots() {
        let snaps = vec![
            vec![proc(1, 90.0), proc(2, 10.0)],
            vec![proc(2, 50.0)],
            vec![proc(3, 5.0), proc(4, 4.0), proc(5, 3.0), proc(6, 2.0)],
        ];
        let top = top_of_group(&snaps);
        let ids: Vec<u32> = top.iter().map(|p| p.name_id).collect();
        assert_eq!(ids, [1, 2, 3, 4, 5], "top 5 by mean cpu");
        assert_eq!(top[0].cpu, 30.0, "90 in one of three snapshots");
        assert_eq!(top[1].cpu, 20.0);
    }
}

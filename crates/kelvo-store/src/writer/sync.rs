//! Session start (closing gaps a previous run left open) and the controller side of
//! sync: ingesting a remote page and recording a truncation.

use std::collections::HashMap;
use std::sync::Arc;

use kelvo_schema::{Gap, GapReason, HostId, Module, SeriesKey, Tier};
use rusqlite::params;

use super::{State, check_stats};
use crate::blob;
use crate::db::{tier_table, tier_width};
use crate::error::{Result, StoreError};
use crate::types::{CursorPage, IngestReport, SessionStart};

impl State {
    // --- startup --------------------------------------------------------------------------

    /// End of the last persisted bucket of `host` across every tier.
    pub(super) fn last_data_end(&self, host_ref: i64) -> Result<Option<i64>> {
        let mut last = None;
        for tier in Tier::PERSISTED {
            let (table, width) = (tier_table(tier)?, tier_width(tier)?);
            let ts: Option<i64> = self.conn.query_row(
                &format!("SELECT max(bucket_ts) FROM {table} WHERE host_id = ?1"),
                [host_ref],
                |r| r.get(0),
            )?;
            last = last.max(ts.map(|t| t + width));
        }
        Ok(last)
    }

    pub(super) fn begin_session(&mut self, host: HostId, now: i64) -> Result<SessionStart> {
        let host_ref = self.host_ref(host)?;
        let last_data = self.last_data_end(host_ref)?;
        let open: Vec<(i64, i64)> = self
            .conn
            .prepare("SELECT id, start_ts FROM gaps WHERE host_id = ?1 AND end_ts IS NULL")?
            .query_map([host_ref], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut out = SessionStart::default();
        // A gap left open means the previous run stopped during it. What happened after
        // the last persisted bucket is unknown, so the gap ends there (or at its own
        // start) and `app_not_running` covers the rest.
        for &(id, start) in &open {
            self.close_gap_row(id, last_data.unwrap_or(start).max(start))?;
            out.closed_gaps += 1;
        }
        let last_gap_end: Option<i64> = self.conn.query_row(
            "SELECT max(end_ts) FROM gaps WHERE host_id = ?1",
            [host_ref],
            |r| r.get(0),
        )?;
        if let Some(start) = last_data.max(last_gap_end)
            && start < now
        {
            let gap = Gap::host(start, Some(now), GapReason::AppNotRunning)?;
            self.upsert_gap(host_ref, &gap)?;
            out.app_not_running = Some(gap);
        }
        let newest_net: Option<i64> = self.conn.query_row(
            "SELECT max(bucket_ts) FROM proc_net_10s WHERE host_id = ?1",
            [host_ref],
            |r| r.get(0),
        )?;
        let width = tier_width(Tier::S10)?;
        out.net_written_to_ms = newest_net.map(|ts| ts + width);
        Ok(out)
    }

    // --- sync ingest ----------------------------------------------------------------------

    pub(super) fn ingest_page(&mut self, host: HostId, page: &CursorPage) -> Result<IngestReport> {
        let host_ref = self.host_ref(host)?;
        tier_table(page.tier)?;
        let mut report = IngestReport::default();
        let mut layouts: HashMap<u32, (u32, usize)> = HashMap::new();
        for l in &page.layouts {
            let series: Arc<[SeriesKey]> = l.series.clone().into();
            let id = self.layout_id(host_ref, &series)?;
            layouts.insert(l.layout_no, (id, series.len()));
        }
        for row in &page.rows {
            let &(layout, n) = layouts
                .get(&row.layout_no)
                .ok_or(StoreError::PageLayout(row.layout_no))?;
            check_stats(n, row.stats.len())?;
            if self.upsert_bucket(
                host_ref,
                page.tier,
                row.bucket_ts,
                layout,
                &blob::pack_f32s(&row.stats),
            )? {
                report.rows_written += 1;
            } else {
                report.rows_unchanged += 1;
            }
        }
        for g in &page.gaps {
            // A gap for a module this build does not know blanks nothing it can draw
            // (D-040); a malformed gap from a peer is dropped rather than stalling sync.
            if g.gap.module == Some(Module::Unknown) || g.gap.validate().is_err() {
                report.gaps_skipped += 1;
                continue;
            }
            if self.upsert_gap(host_ref, &g.gap)? {
                report.gaps_written += 1;
            }
        }
        for e in &page.events {
            if self.upsert_event(host_ref, e.ts_ms, &e.kind, &e.payload)? {
                report.events_written += 1;
            }
        }
        self.conn
            .prepare_cached(
                "INSERT INTO cursors (host_id, tier, epoch, seq, kinds) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (host_id, tier) DO UPDATE
                   SET epoch = excluded.epoch, seq = excluded.seq, kinds = excluded.kinds",
            )?
            .execute(params![
                host_ref,
                page.tier.as_str(),
                page.epoch.hyphenated().to_string(),
                page.last_seq,
                page.kinds.to_stored()
            ])?;
        Ok(report)
    }

    pub(super) fn record_truncation(
        &mut self,
        host: HostId,
        tier: Tier,
        earliest: i64,
    ) -> Result<Option<Gap>> {
        let host_ref = self.host_ref(host)?;
        tier_width(tier)?;
        self.conn.execute(
            "DELETE FROM cursors WHERE host_id = ?1 AND tier = ?2",
            params![host_ref, tier.as_str()],
        )?;
        // Gaps are host-wide, not per tier. The 10 s tier is a 24 h window that a
        // controller offline for a day always falls behind; a gap for that would blank
        // minute data that is actually there. So only M1 and M15 truncation write one
        // (D-041), from the end of the newest minute or 15-minute bucket held: what one
        // of them still covers is not missing.
        if !matches!(tier, Tier::M1 | Tier::M15) {
            return Ok(None);
        }
        let mut last: Option<i64> = None;
        for t in [Tier::M1, Tier::M15] {
            let (table, width) = (tier_table(t)?, tier_width(t)?);
            let ts: Option<i64> = self.conn.query_row(
                &format!("SELECT max(bucket_ts) FROM {table} WHERE host_id = ?1"),
                [host_ref],
                |r| r.get(0),
            )?;
            last = last.max(ts.map(|t| t + width));
        }
        match last {
            Some(start) if start < earliest => {
                let gap = Gap::host(start, Some(earliest), GapReason::Truncated)?;
                self.upsert_gap(host_ref, &gap)?;
                Ok(Some(gap))
            }
            _ => Ok(None),
        }
    }
}

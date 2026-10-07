//! Discarding a host's rows from a time on, and clearing a host's history.

use kelvo_schema::{HostId, Tier};
use rusqlite::params;

use super::State;
use crate::error::Result;

impl State {
    pub(super) fn discard_from(&mut self, host: HostId, from: i64) -> Result<()> {
        let host_ref = self.host_ref(host)?;
        for (table, col) in [
            ("tier_10s", "bucket_ts"),
            ("tier_1m", "bucket_ts"),
            ("tier_15m", "bucket_ts"),
            ("proc_snap", "ts"),
            ("proc_top_1m", "bucket_ts"),
            ("proc_top_15m", "bucket_ts"),
            ("proc_net_10s", "bucket_ts"),
            ("proc_net_1m", "bucket_ts"),
            ("proc_net_15m", "bucket_ts"),
            ("events", "ts"),
            ("gaps", "start_ts"),
        ] {
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE host_id = ?1 AND {col} >= ?2"),
                params![host_ref, from],
            )?;
        }
        // The minute holding `from` summed 10 s rows that just went.
        let minute = Tier::M1.bucket_start(from).unwrap_or(from);
        if minute < from {
            self.recompute_net_minute(host_ref, minute)?;
        }
        let cut: Vec<i64> = self
            .conn
            .prepare("SELECT id FROM gaps WHERE host_id = ?1 AND end_ts > ?2")?
            .query_map(params![host_ref, from], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for id in cut {
            self.close_gap_row(id, from)?;
        }
        Ok(())
    }

    /// [`Writer::discard_from`]. A batch before it that fails to commit is lost as any
    /// other (its span recorded as `write_failed`), and the discard still runs.
    pub(super) fn discard_committed(&mut self, host: HostId, from: i64) -> Result<()> {
        self.commit_logged();
        self.apply(|s| s.discard_from(host, from))?;
        // A `write_failed` gap rides the next commit: it must not cover the span that was
        // just discarded.
        for spans in [&mut self.batch_span, &mut self.lost] {
            if let Some((start, end)) = spans.get(&host).copied() {
                if start >= from {
                    spans.remove(&host);
                } else {
                    spans.insert(host, (start, end.min(from)));
                }
            }
        }
        self.commit()
    }

    pub(super) fn clear_host(&mut self, host: HostId, now: i64) -> Result<()> {
        self.commit()?;
        self.apply(|s| {
            let host_ref = s.host_ref(host)?;
            // Everything up to now is gone: a remote cursor into it must resync.
            let last = s.next_seq - 1;
            for tier in Tier::PERSISTED {
                s.conn.execute(
                    "INSERT INTO pruned (host_id, tier, seq, ts) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (host_id, tier) DO UPDATE SET seq = excluded.seq, ts = excluded.ts",
                    params![host_ref, tier.as_str(), last, now],
                )?;
            }
            for table in [
                "tier_10s",
                "tier_1m",
                "tier_15m",
                "gaps",
                "events",
                "proc_snap",
                "proc_top_1m",
                "proc_top_15m",
                "proc_net_10s",
                "proc_net_1m",
                "proc_net_15m",
                "cursors",
            ] {
                s.conn
                    .execute(&format!("DELETE FROM {table} WHERE host_id = ?1"), [host_ref])?;
            }
            Ok(())
        })?;
        self.commit()?;
        self.incremental_vacuum()?;
        Ok(())
    }
}

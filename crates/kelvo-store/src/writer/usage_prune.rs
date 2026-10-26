//! Usage rollups preserve every identity and merge sufficient statistics, never top-N.
use std::collections::BTreeMap;

use kelvo_schema::floor_to;
use rusqlite::params;

use super::State;
use crate::usage::{UsageFacts, codec};
use crate::{Result, Retention};

impl State {
    pub(super) fn usage_prune(&mut self, now: i64, retention: &Retention) -> Result<()> {
        for host in self.host_refs()? {
            self.roll_usage(host, 10_000, 60_000, now - retention.s10_ms)?;
            self.roll_usage(host, 60_000, 900_000, now - retention.m1_keep_ms())?;
            let cut = now - retention.history_ms;
            self.apply(|s| {
                for table in [
                    "usage_app_buckets",
                    "usage_process_buckets",
                    "usage_coverage",
                ] {
                    s.conn.execute(
                        &format!("DELETE FROM {table} WHERE host_id=?1 AND bucket_ts+width_ms<=?2"),
                        params![host, cut],
                    )?;
                }
                s.conn.execute(
                    "DELETE FROM usage_uncertain WHERE host_id=?1 AND possible_to_ms<=?2",
                    params![host, cut],
                )?;
                s.usage_trim_identities(host, floor_to(cut, 900_000))?;
                Ok(())
            })?;
            self.commit()?;
        }
        Ok(())
    }

    pub(super) fn usage_trim_identities(&mut self, host: i64, cut: i64) -> Result<()> {
        // Last-seen times are sufficient only at aligned whole-bucket edges.
        self.conn.execute(
            "DELETE FROM usage_processes WHERE host_id=?1 AND last_ms<=?2",
            params![host, cut],
        )?;
        self.conn.execute("DELETE FROM usage_apps WHERE host_id=?1 AND last_ms<=?2 AND NOT EXISTS(SELECT 1 FROM usage_processes p WHERE p.app_id=usage_apps.id) AND NOT EXISTS(SELECT 1 FROM usage_uncertain u WHERE u.app_id=usage_apps.id)",params![host,cut])?;
        self.conn.execute("UPDATE usage_recording_start SET retained_from_ms=max(retained_from_ms,?2) WHERE host_id=?1",params![host,cut])?;
        Ok(())
    }

    fn roll_usage(&mut self, host: i64, source: i64, target: i64, cut: i64) -> Result<()> {
        let cut = floor_to(cut, target);
        loop {
            let first:Option<i64>=self.conn.query_row("SELECT min(bucket_ts) FROM usage_coverage WHERE host_id=?1 AND width_ms=?2 AND bucket_ts+width_ms<=?3",params![host,source,cut],|r|r.get(0))?;
            let Some(first) = first else { break };
            let start = floor_to(first, target);
            let end = (start + 3_600_000).min(cut);
            self.apply(|s| {
                for table in ["usage_app_buckets","usage_process_buckets","usage_coverage"] {
                    let mut folded:BTreeMap<i64,BTreeMap<u64,UsageFacts>>=BTreeMap::new();
                    {
                        let mut stmt=s.conn.prepare_cached(&format!("SELECT bucket_ts,blob FROM {table} WHERE host_id=?1 AND width_ms=?2 AND bucket_ts>=?3 AND bucket_ts<?4 ORDER BY bucket_ts"))?;
                        let mut rows=stmt.query(params![host,source,start,end])?;
                        while let Some(row)=rows.next()? {
                            let at:i64=row.get(0)?;
                            let blob:Vec<u8>=row.get(1)?;
                            let group=folded.entry(floor_to(at,target)).or_default();
                            for (id,f) in codec::decode(&blob)? {
                                group.entry(id).or_default().merge(&f);
                            }
                        }
                    }
                    for (at,group) in folded {
                        let blob=codec::encode(&group.into_iter().collect::<Vec<_>>())?;
                        s.usage_upsert(table,host,target,at,&blob)?;
                    }
                    s.conn.execute(&format!("DELETE FROM {table} WHERE host_id=?1 AND width_ms=?2 AND bucket_ts>=?3 AND bucket_ts<?4"),params![host,source,start,end])?;
                }
                Ok(())
            })?;
            self.commit()?;
            self.yield_to_queue()?;
        }
        Ok(())
    }

    pub(super) fn usage_discard(&mut self, host: i64, from: i64) -> Result<()> {
        // A coarse bucket crossing the new timeline edge cannot be partially recovered.
        // Drop it whole and disclose the missing edge instead of prorating its facts.
        for table in [
            "usage_app_buckets",
            "usage_process_buckets",
            "usage_coverage",
        ] {
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE host_id=?1 AND bucket_ts+width_ms>?2"),
                params![host, from],
            )?;
        }
        self.conn.execute(
            "DELETE FROM usage_uncertain WHERE host_id=?1 AND possible_to_ms>?2",
            params![host, from],
        )?;
        Ok(())
    }
}

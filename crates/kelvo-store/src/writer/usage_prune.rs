//! Usage rollups preserve every identity and merge sufficient statistics, never top-N.
use std::collections::{BTreeMap, HashSet};

use kelvo_schema::floor_to;
use rusqlite::params;

use super::State;
use crate::usage::{UsageFacts, codec, frames};
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
                        &format!("DELETE FROM {table} WHERE host_id=?1 AND bucket_ts+CASE WHEN width_ms=10000 THEN 300000 ELSE width_ms END<=?2"),
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
        let cut = floor_to(
            cut,
            target.max(if source == 10_000 {
                frames::BATCH_MS
            } else {
                target
            }),
        );
        loop {
            let first:Option<i64>=self.conn.query_row("SELECT min(bucket_ts) FROM usage_coverage WHERE host_id=?1 AND width_ms=?2 AND bucket_ts+CASE WHEN width_ms=10000 THEN 300000 ELSE width_ms END<=?3",params![host,source,cut],|r|r.get(0))?;
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
                            for (time,rows) in frames::decode(&blob,at,None)? {
                                let group=folded.entry(floor_to(time,target)).or_default();
                                for (id,f) in rows {group.entry(id).or_default().merge(&f);}
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

    /// A clock rollback invalidates future last-seen bounds. Check candidate
    /// identities against surviving facts before reclaiming any metadata.
    pub(super) fn usage_discard_identities(&mut self, host: i64, from: i64) -> Result<()> {
        for (identities, facts) in [
            ("usage_processes", "usage_process_buckets"),
            ("usage_apps", "usage_app_buckets"),
        ] {
            let mut candidates: HashSet<u64> = self
                .conn
                .prepare_cached(&format!(
                    "SELECT id FROM {identities} WHERE host_id=?1 AND last_ms>?2"
                ))?
                .query_map(params![host, from], |r| Ok(r.get::<_, i64>(0)? as u64))?
                .collect::<rusqlite::Result<_>>()?;
            self.conn.execute(
                &format!("UPDATE {identities} SET last_ms=?2 WHERE host_id=?1 AND last_ms>?2"),
                params![host, from],
            )?;
            if candidates.is_empty() {
                continue;
            }
            {
                let mut stmt = self.conn.prepare_cached(&format!(
                    "SELECT bucket_ts,blob FROM {facts} WHERE host_id=?1"
                ))?;
                let mut rows = stmt.query([host])?;
                while !candidates.is_empty() {
                    let Some(row) = rows.next()? else { break };
                    let at: i64 = row.get(0)?;
                    let blob: Vec<u8> = row.get(1)?;
                    for (_, observed) in frames::decode(&blob, at, Some(&candidates))? {
                        for (id, _) in observed {
                            candidates.remove(&id);
                        }
                    }
                }
            }
            let ids: Vec<_> = candidates.into_iter().collect();
            for chunk in ids.chunks(500) {
                let members = chunk
                    .iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                let references = if identities == "usage_apps" {
                    " AND NOT EXISTS(SELECT 1 FROM usage_processes p WHERE p.app_id=usage_apps.id) AND NOT EXISTS(SELECT 1 FROM usage_uncertain u WHERE u.app_id=usage_apps.id)"
                } else {
                    ""
                };
                self.conn.execute(&format!("DELETE FROM {identities} WHERE host_id=?1 AND id IN ({members}){references}"),[host])?;
            }
        }
        Ok(())
    }

    pub(super) fn usage_discard(&mut self, host: i64, from: i64) -> Result<()> {
        // Preserve exact finest frames before a rollback edge even when the
        // physical commit batch crosses it. Coarse facts cannot be split.
        let batches:Vec<(i64,Vec<u8>)>=self.conn.prepare_cached("SELECT bucket_ts,blob FROM usage_coverage WHERE host_id=?1 AND width_ms=10000 AND bucket_ts<?2 AND bucket_ts+300000>?2")?.query_map(params![host,from],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        for (at, _) in batches {
            for table in [
                "usage_app_buckets",
                "usage_process_buckets",
                "usage_coverage",
            ] {
                let blob:Vec<u8>=self.conn.query_row(&format!("SELECT blob FROM {table} WHERE host_id=?1 AND width_ms=10000 AND bucket_ts=?2"),params![host,at],|r|r.get(0))?;
                let kept: Vec<_> = frames::decode(&blob, at, None)?
                    .into_iter()
                    .filter(|(time, _)| time + 10000 <= from)
                    .collect();
                if kept.is_empty() {
                    self.conn.execute(&format!("DELETE FROM {table} WHERE host_id=?1 AND width_ms=10000 AND bucket_ts=?2"),params![host,at])?;
                } else {
                    self.usage_upsert(table, host, 10000, at, &frames::encode(&kept)?)?;
                }
            }
        }
        // A coarse bucket crossing the new timeline edge cannot be partially recovered.
        // Drop it whole and disclose the missing edge instead of prorating its facts.
        for table in [
            "usage_app_buckets",
            "usage_process_buckets",
            "usage_coverage",
        ] {
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE host_id=?1 AND width_ms<>10000 AND bucket_ts+width_ms>?2"),
                params![host, from],
            )?;
        }
        self.conn.execute(
            "DELETE FROM usage_uncertain WHERE host_id=?1 AND (possible_to_ms>?2 OR discovery_bucket_ts+10000>?2)",
            params![host, from],
        )?;
        Ok(())
    }
}

//! Usage storage rates include process identity churn independently of bucket tiers.
use super::Reader;
use crate::{
    Result,
    types::Retention,
    usage::{UsageFacts, codec, frames},
};
use rusqlite::params;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageHistoryGrowth {
    pub measured_ms: i64,
    pub finest_day_bytes: u64,
    pub minute_day_bytes: u64,
    pub quarter_day_bytes: u64,
    pub identity_day_bytes: u64,
    pub fixed_identity_bytes: u64,
}

impl Reader {
    pub(super) fn usage_growth(&self, fill: f64) -> Result<Option<UsageHistoryGrowth>> {
        let bounds: (Option<i64>, Option<i64>) = self.conn.query_row(
            "SELECT min(bucket_ts),max(bucket_ts+width_ms) FROM usage_coverage",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let (Some(first), Some(_last)) = bounds else {
            return Ok(None);
        };
        let recorded = self.usage_recorded_ms(None, None)?;
        if recorded < super::growth::MIN_MEASURED_MS {
            return Ok(None);
        }
        let mut rates = [0_u64; 3];
        for (index, width) in [10_000, 60_000, 900_000].into_iter().enumerate() {
            let rate = self.usage_tier_day(width, fill)?;
            rates[index] = if let Some(rate) = rate {
                rate
            } else {
                // Finest facts may be absent after a long stopped interval. A
                // coarser row cannot reveal a finer distribution; use the known
                // coarser cost at finer row frequency as a conservative estimate.
                self.usage_tier_day(900_000, fill)?.unwrap_or(0) * (900_000 / width) as u64
            };
        }
        let pages = |table: &str| -> Result<u64> {
            let bytes:i64=self.conn.query_row("SELECT coalesce(sum(d.pgsize-d.unused),0) FROM dbstat d LEFT JOIN sqlite_schema s ON s.name=d.name WHERE coalesce(s.tbl_name,d.name)=?1",[table],|r|r.get(0))?;
            Ok((bytes as f64 / fill) as u64)
        };
        let first: i64 = self
            .conn
            .query_row("SELECT min(from_ms) FROM usage_recording_start", [], |r| {
                r.get::<_, Option<i64>>(0)
            })?
            .unwrap_or(first);
        let (initial, total): (i64, i64) = self.conn.query_row(
            "SELECT coalesce(sum(start_time_us/1000<=?1+10000),0),count(*) FROM usage_processes",
            [first],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let process_bytes = pages("usage_processes")?;
        let initial_bytes = if total > 0 {
            process_bytes * initial as u64 / total as u64
        } else {
            0
        };
        // Identities cannot be reduced by coarse temporal buckets. Their recurring
        // observed population and uncertainty records scale with retained days.
        let linear = process_bytes.saturating_sub(initial_bytes) + pages("usage_uncertain")?;
        let measured = recorded.max(1);
        Ok(Some(UsageHistoryGrowth {
            measured_ms: recorded,
            finest_day_bytes: rates[0],
            minute_day_bytes: rates[1],
            quarter_day_bytes: rates[2],
            identity_day_bytes: (linear as f64 * Retention::DAY_MS as f64 / measured as f64) as u64,
            fixed_identity_bytes: initial_bytes
                + pages("usage_apps")?
                + pages("usage_recording_start")?,
        }))
    }

    /// Measure an existing tier, or encode an hour of finer retained observations
    /// with the production rollup/codec rather than guessing a compression ratio.
    fn usage_tier_day(&self, target: i64, fill: f64) -> Result<Option<u64>> {
        let source: Option<i64> = self.conn.query_row(
            "SELECT max(width_ms) FROM usage_coverage WHERE width_ms<=?1",
            [target],
            |r| r.get(0),
        )?;
        let Some(source) = source else {
            return Ok(None);
        };
        let at: i64 = self.conn.query_row(
            "SELECT max(bucket_ts) FROM usage_coverage WHERE width_ms=?1",
            [source],
            |r| r.get(0),
        )?;
        let blob: Vec<u8> = self.conn.query_row(
            "SELECT blob FROM usage_coverage WHERE width_ms=?1 AND bucket_ts=?2 LIMIT 1",
            params![source, at],
            |r| r.get(0),
        )?;
        let last = frames::decode(&blob, at, None)?
            .last()
            .map_or(at + source, |(time, _)| time + source);
        let start = kelvo_schema::floor_to(last - 3_600_000, target);
        let measured = self.usage_recorded_ms(Some(source), Some(start))?;
        if measured < super::growth::MIN_MEASURED_MS {
            return Ok(None);
        }
        let mut bytes = 0_u64;
        for table in [
            "usage_app_buckets",
            "usage_process_buckets",
            "usage_coverage",
        ] {
            if source == target && target == 10_000 {
                let n:i64=self.conn.query_row(&format!("SELECT coalesce(sum(length(blob)+64),0) FROM {table} WHERE width_ms=?1 AND bucket_ts>=?2"),params![source,start],|r|r.get(0))?;
                bytes += n as u64;
                continue;
            }
            let mut groups = BTreeMap::<(i64, i64), BTreeMap<u64, UsageFacts>>::new();
            let mut stmt=self.conn.prepare(&format!("SELECT host_id,bucket_ts,blob FROM {table} WHERE width_ms=?1 AND bucket_ts>=?2 ORDER BY bucket_ts"))?;
            let mut rows = stmt.query(params![source, start])?;
            while let Some(row) = rows.next()? {
                let host: i64 = row.get(0)?;
                let at: i64 = row.get(1)?;
                let blob: Vec<u8> = row.get(2)?;
                for (time, rows) in frames::decode(&blob, at, None)? {
                    let group = groups
                        .entry((host, kelvo_schema::floor_to(time, target)))
                        .or_default();
                    for (id, f) in rows {
                        group.entry(id).or_default().merge(&f);
                    }
                }
            }
            for group in groups.into_values() {
                // Integer keys, B-tree/index cells and row headers, in addition
                // to actual encoded payload. Fill is measured from SQLite pages.
                bytes += (codec::encode(&group.into_iter().collect::<Vec<_>>())?.len() + 64) as u64;
            }
        }
        Ok(Some(
            (bytes as f64 / fill * Retention::DAY_MS as f64 / measured as f64) as u64,
        ))
    }
    fn usage_recorded_ms(&self, width: Option<i64>, from: Option<i64>) -> Result<i64> {
        let mut times = std::collections::BTreeMap::new();
        let mut stmt=self.conn.prepare("SELECT width_ms,bucket_ts,blob FROM usage_coverage WHERE (?1 IS NULL OR width_ms=?1) AND (?2 IS NULL OR bucket_ts>=?2)")?;
        let mut rows = stmt.query(params![width, from])?;
        while let Some(row) = rows.next()? {
            let width: i64 = row.get(0)?;
            let at: i64 = row.get(1)?;
            let blob: Vec<u8> = row.get(2)?;
            for (time, _) in frames::decode(&blob, at, None)? {
                times.insert(time, width);
            }
        }
        Ok(times.into_values().sum())
    }
}

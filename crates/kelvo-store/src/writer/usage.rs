//! Persist every usage identity and sufficient statistic before interval ranking.
use std::collections::BTreeMap;

use kelvo_schema::HostId;
use rusqlite::{OptionalExtension, params};

use super::{Op, State, Writer};
use crate::usage::{UsageBucket, UsageFacts, codec};
use crate::{Result, StoreError};

impl State {
    pub(super) fn write_usage(&mut self, host: HostId, bucket: &UsageBucket) -> Result<()> {
        let host = self.host_ref(host)?;
        self.conn.execute("INSERT INTO usage_recording_start(host_id,from_ms) VALUES(?1,?2) ON CONFLICT(host_id) DO UPDATE SET from_ms=min(from_ms,excluded.from_ms)",params![host,bucket.from_ms])?;
        let mut apps = BTreeMap::new();
        for app in &bucket.apps {
            let id = self.usage_app_id(host, &app.name, bucket.to_ms)?;
            apps.entry(id as u64)
                .or_insert_with(UsageFacts::default)
                .merge(&app.facts);
        }
        let mut processes = BTreeMap::new();
        for p in &bucket.processes {
            let app = self.usage_app_id(host, &p.app_name, bucket.to_ms)?;
            self.conn.prepare_cached("INSERT INTO usage_processes(host_id,app_id,pid,start_time_us,name,last_ms) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(host_id,pid,start_time_us) DO UPDATE SET last_ms=max(last_ms,excluded.last_ms)")?.execute(params![host,app,p.pid,p.start_time_us,&*p.name,bucket.to_ms])?;
            let id:i64 = self.conn.prepare_cached("SELECT id FROM usage_processes WHERE host_id=?1 AND pid=?2 AND start_time_us=?3")?.query_row(params![host,p.pid,p.start_time_us],|r| r.get(0))?;
            processes
                .entry(id as u64)
                .or_insert_with(UsageFacts::default)
                .merge(&p.facts);
        }
        let source = UsageFacts {
            coverage: bucket.source,
            ..UsageFacts::default()
        };
        for (table, rows) in [
            ("usage_app_buckets", apps.into_iter().collect::<Vec<_>>()),
            ("usage_process_buckets", processes.into_iter().collect()),
            ("usage_coverage", vec![(0, source)]),
        ] {
            let blob = codec::encode(&rows)?;
            self.usage_upsert(table, host, 10_000, bucket.from_ms, &blob)?;
        }
        for u in &bucket.uncertain_network {
            if u.possible_to_ms <= u.possible_from_ms || u.records == 0 {
                return Err(StoreError::Usage(
                    "invalid uncertain network interval".into(),
                ));
            }
            let app = u
                .app_name
                .as_ref()
                .map(|n| self.usage_app_id(host, n, bucket.to_ms))
                .transpose()?;
            let seq = self.alloc_seq();
            self.conn.prepare_cached("INSERT OR IGNORE INTO usage_uncertain(host_id,app_id,possible_from_ms,possible_to_ms,rx_bytes,tx_bytes,records,seq) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)")?.execute(params![host,app,u.possible_from_ms,u.possible_to_ms,signed(u.rx_bytes)?,signed(u.tx_bytes)?,signed(u.records)?,seq])?;
        }
        Ok(())
    }

    fn usage_app_id(&mut self, host: i64, name: &str, at: i64) -> Result<i64> {
        if name.is_empty() || name.len() > 1024 || name.contains('\0') {
            return Err(StoreError::Usage("invalid app identity".into()));
        }
        self.conn.prepare_cached("INSERT INTO usage_apps(host_id,name,last_ms) VALUES(?1,?2,?3) ON CONFLICT(host_id,name) DO UPDATE SET last_ms=max(last_ms,excluded.last_ms)")?.execute(params![host,name,at])?;
        Ok(self
            .conn
            .prepare_cached("SELECT id FROM usage_apps WHERE host_id=?1 AND name=?2")?
            .query_row(params![host, name], |r| r.get(0))?)
    }

    pub(super) fn usage_upsert(
        &mut self,
        table: &str,
        host: i64,
        width: i64,
        at: i64,
        blob: &[u8],
    ) -> Result<()> {
        let previous: Option<Vec<u8>> = self
            .conn
            .prepare_cached(&format!(
                "SELECT blob FROM {table} WHERE host_id=?1 AND width_ms=?2 AND bucket_ts=?3"
            ))?
            .query_row(params![host, width, at], |r| r.get(0))
            .optional()?;
        if previous.as_deref() == Some(blob) {
            return Ok(());
        }
        let seq = self.alloc_seq();
        self.conn.prepare_cached(&format!("INSERT INTO {table}(host_id,width_ms,bucket_ts,seq,blob) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(host_id,width_ms,bucket_ts) DO UPDATE SET seq=excluded.seq,blob=excluded.blob"))?.execute(params![host,width,at,seq,blob])?;
        Ok(())
    }
}

fn signed(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| StoreError::Usage("network count exceeds storage integer range".into()))
}

impl Writer {
    /// Queue complete finest-resolution usage facts, before any floor or ranking.
    pub fn write_usage_bucket(
        &self,
        host: HostId,
        bucket: crate::usage::UsageBucket,
    ) -> Result<()> {
        crate::usage::validate_bucket(&bucket)?;
        self.queue(Op::Usage(host, bucket))
    }
}

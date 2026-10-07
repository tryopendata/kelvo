//! Row writes: buckets, gaps, events, process snapshots and network buckets.

use kelvo_schema::{Gap, GapReason, HostId, Module, Tier};
use rusqlite::{OptionalExtension, params};

use super::State;
use crate::blob::{self, PackedProc};
use crate::db::{net_table, tier_table, tier_width};
use crate::error::Result;
use crate::rolldown::NetFold;
use crate::types::{BucketRow, NetBucket, ProcRow};

impl State {
    // --- rows ---------------------------------------------------------------------------

    pub(super) fn write_bucket(&mut self, row: &BucketRow) -> Result<bool> {
        let host_ref = self.host_ref(row.host)?;
        let layout = self.layout_id(host_ref, &row.series)?;
        self.upsert_bucket(
            host_ref,
            row.tier,
            row.bucket_ts,
            layout,
            &blob::pack_f32s(&row.stats),
        )
    }

    /// Upserts on `(host, bucket_ts, layout)`. Returns whether the row changed.
    pub(super) fn upsert_bucket(
        &mut self,
        host_ref: i64,
        tier: Tier,
        bucket_ts: i64,
        layout: u32,
        stats: &[u8],
    ) -> Result<bool> {
        let table = tier_table(tier)?;
        let seq = self.alloc_seq();
        let n = self
            .conn
            .prepare_cached(&format!(
                "INSERT INTO {table} (host_id, bucket_ts, layout_id, seq, blob)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (host_id, bucket_ts, layout_id) DO UPDATE
                   SET seq = excluded.seq, blob = excluded.blob
                   WHERE blob IS NOT excluded.blob"
            ))?
            .execute(params![host_ref, bucket_ts, layout, seq, stats])?;
        if n == 0 {
            self.unalloc_seq();
        }
        Ok(n > 0)
    }

    /// Upserts on `(host, start, reason, module)`. A closed gap is never reopened by an
    /// older copy of itself. Returns whether the row changed.
    pub(super) fn upsert_gap(&mut self, host_ref: i64, gap: &Gap) -> Result<bool> {
        let module = gap.module.map(Module::as_str);
        let reason = gap.reason.as_str();
        let existing: Option<(i64, Option<i64>)> = self
            .conn
            .prepare_cached(
                "SELECT id, end_ts FROM gaps
                 WHERE host_id = ?1 AND start_ts = ?2 AND reason = ?3 AND module IS ?4",
            )?
            .query_row(params![host_ref, gap.start_ms, reason, module], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        match existing {
            Some((_, end)) if end == gap.end_ms || gap.end_ms.is_none() => Ok(false),
            Some((id, _)) => {
                let seq = self.alloc_seq();
                self.conn
                    .prepare_cached("UPDATE gaps SET end_ts = ?1, seq = ?2 WHERE id = ?3")?
                    .execute(params![gap.end_ms, seq, id])?;
                Ok(true)
            }
            None => {
                let seq = self.alloc_seq();
                self.conn
                    .prepare_cached(
                        "INSERT INTO gaps (host_id, start_ts, end_ts, module, reason, seq)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    )?
                    .execute(params![
                        host_ref,
                        gap.start_ms,
                        gap.end_ms,
                        module,
                        reason,
                        seq
                    ])?;
                Ok(true)
            }
        }
    }

    pub(super) fn open_gap(&mut self, host: HostId, gap: &Gap) -> Result<()> {
        let host_ref = self.host_ref(host)?;
        let open: Option<i64> = self
            .conn
            .prepare_cached(
                "SELECT id FROM gaps
                 WHERE host_id = ?1 AND reason = ?2 AND module IS ?3 AND end_ts IS NULL",
            )?
            .query_row(
                params![
                    host_ref,
                    gap.reason.as_str(),
                    gap.module.map(Module::as_str)
                ],
                |r| r.get(0),
            )
            .optional()?;
        if open.is_none() {
            self.upsert_gap(host_ref, gap)?;
        }
        Ok(())
    }

    pub(super) fn close_gap(
        &mut self,
        host: HostId,
        reason: GapReason,
        module: Option<Module>,
        start_ms: Option<i64>,
        end_ms: i64,
    ) -> Result<()> {
        let host_ref = self.host_ref(host)?;
        let ids: Vec<i64> = self
            .conn
            .prepare_cached(
                "SELECT id FROM gaps
                 WHERE host_id = ?1 AND reason = ?2 AND module IS ?3 AND end_ts IS NULL",
            )?
            .query_map(
                params![host_ref, reason.as_str(), module.map(Module::as_str)],
                |r| r.get(0),
            )?
            .collect::<rusqlite::Result<_>>()?;
        if ids.is_empty()
            && let Some(start) = start_ms
        {
            // The open was lost with a batch that failed to commit (review #9).
            let gap = Gap::new(start, Some(end_ms.max(start)), module, reason)?;
            self.upsert_gap(host_ref, &gap)?;
        }
        for id in ids {
            self.close_gap_row(id, end_ms)?;
        }
        Ok(())
    }

    /// Sets `end_ts` (never before `start_ts`) and a new `seq`.
    pub(super) fn close_gap_row(&mut self, id: i64, end_ms: i64) -> Result<()> {
        let seq = self.alloc_seq();
        self.conn
            .prepare_cached("UPDATE gaps SET end_ts = max(start_ts, ?1), seq = ?2 WHERE id = ?3")?
            .execute(params![end_ms, seq, id])?;
        Ok(())
    }

    /// Upserts on `(host, ts, kind)`. Returns whether the row changed.
    pub(super) fn upsert_event(
        &mut self,
        host_ref: i64,
        ts_ms: i64,
        kind: &str,
        payload: &[u8],
    ) -> Result<bool> {
        let existing: Option<(i64, Vec<u8>)> = self
            .conn
            .prepare_cached(
                "SELECT id, payload FROM events WHERE host_id = ?1 AND ts = ?2 AND kind = ?3",
            )?
            .query_row(params![host_ref, ts_ms, kind], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        match existing {
            Some((_, p)) if p == payload => Ok(false),
            Some((id, _)) => {
                let seq = self.alloc_seq();
                self.conn
                    .prepare_cached("UPDATE events SET payload = ?1, seq = ?2 WHERE id = ?3")?
                    .execute(params![payload, seq, id])?;
                Ok(true)
            }
            None => {
                let seq = self.alloc_seq();
                self.conn
                    .prepare_cached(
                        "INSERT INTO events (host_id, ts, kind, payload, seq) VALUES (?1, ?2, ?3, ?4, ?5)",
                    )?
                    .execute(params![host_ref, ts_ms, kind, payload, seq])?;
                Ok(true)
            }
        }
    }

    pub(super) fn write_proc_snapshot(
        &mut self,
        host: HostId,
        ts_ms: i64,
        rows: &[ProcRow],
    ) -> Result<()> {
        let host_ref = self.host_ref(host)?;
        let mut packed = Vec::with_capacity(rows.len());
        for r in rows {
            packed.push(PackedProc {
                name_id: self.proc_name_id(host_ref, &r.name)?,
                pid: r.pid,
                cpu: r.cpu_pct,
                mem_kib: u32::try_from(r.mem_bytes / 1024).unwrap_or(u32::MAX),
                threads: r.threads,
                wakeups: r.idle_wakeups_per_s,
                energy: r.energy,
            });
        }
        let seq = self.alloc_seq();
        let n = self
            .conn
            .prepare_cached(
                "INSERT INTO proc_snap (host_id, ts, seq, blob) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (host_id, ts) DO UPDATE SET seq = excluded.seq, blob = excluded.blob
                   WHERE blob IS NOT excluded.blob",
            )?
            .execute(params![host_ref, ts_ms, seq, blob::pack_procs(&packed)])?;
        if n == 0 {
            self.unalloc_seq();
        }
        Ok(())
    }

    pub(super) fn write_net_bucket(
        &mut self,
        host: HostId,
        bucket_ts: i64,
        bucket: &NetBucket,
    ) -> Result<()> {
        let host_ref = self.host_ref(host)?;
        let width = tier_width(Tier::S10)?;
        let mut fold = NetFold::default();
        fold.add_header(&blob::NetHeader {
            measured_ms: bucket
                .measured_ms
                .min(u32::try_from(width).unwrap_or(u32::MAX)),
            rx_bytes: bucket.iface_rx_bytes,
            tx_bytes: bucket.iface_tx_bytes,
            rx_pkts: bucket.iface_rx_pkts,
            tx_pkts: bucket.iface_tx_pkts,
        });
        for app in &bucket.apps {
            let id = self.net_name_id(host_ref, app.name.as_deref(), bucket_ts)?;
            fold.add_app(id, app.rx_bytes, app.tx_bytes);
        }
        let (header, rows) = fold.finish();
        self.upsert_net(
            Tier::S10,
            host_ref,
            bucket_ts,
            &blob::pack_net(&header, &rows),
        )?;
        let minute = Tier::M1.bucket_start(bucket_ts).unwrap_or(bucket_ts);
        self.recompute_net_minute(host_ref, minute)
    }

    /// Upserts a network row on `(host, bucket_ts)`. Returns whether the row changed.
    pub(super) fn upsert_net(
        &mut self,
        tier: Tier,
        host_ref: i64,
        bucket_ts: i64,
        blob: &[u8],
    ) -> Result<bool> {
        let table = net_table(tier)?;
        let seq = self.alloc_seq();
        let n = self
            .conn
            .prepare_cached(&format!(
                "INSERT INTO {table} (host_id, bucket_ts, seq, blob) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (host_id, bucket_ts) DO UPDATE SET seq = excluded.seq, blob = excluded.blob
                   WHERE blob IS NOT excluded.blob"
            ))?
            .execute(params![host_ref, bucket_ts, seq, blob])?;
        if n == 0 {
            self.unalloc_seq();
        }
        Ok(n > 0)
    }

    /// Rewrites the `proc_net_1m` row of `minute` as the sum of its `proc_net_10s` rows,
    /// or deletes it when the minute has none.
    pub(super) fn recompute_net_minute(&mut self, host_ref: i64, minute: i64) -> Result<()> {
        let width = tier_width(Tier::M1)?;
        let blobs: Vec<Vec<u8>> = self
            .conn
            .prepare_cached(
                "SELECT blob FROM proc_net_10s
                 WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3",
            )?
            .query_map(params![host_ref, minute, minute + width], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        if blobs.is_empty() {
            self.conn
                .prepare_cached("DELETE FROM proc_net_1m WHERE host_id = ?1 AND bucket_ts = ?2")?
                .execute(params![host_ref, minute])?;
            return Ok(());
        }
        let mut fold = NetFold::default();
        for b in &blobs {
            let (header, rows) = blob::unpack_net(b)?;
            fold.add(&header, &rows);
        }
        let (header, rows) = fold.finish();
        self.upsert_net(Tier::M1, host_ref, minute, &blob::pack_net(&header, &rows))?;
        Ok(())
    }
}

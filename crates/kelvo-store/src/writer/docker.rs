//! Single-writer Docker admission, relational inserts and terminal commit receipts.
use super::{Op, State, Writer};
use crate::docker::{invalid, pack, table, unpack};
use crate::{Result, StoreError, docker::*};
use kelvo_schema::{HostId, Tier, lock::LockExt};
use rusqlite::{OptionalExtension, params};
use std::sync::{Arc, Mutex, mpsc};

#[derive(Default)]
pub(super) struct Admission {
    pub pending_bytes: usize,
    pub pending_writes: usize,
    pub logical_bytes: u64,
    pub allowance: u64,
    pub paused: bool,
}
pub(super) struct Applied {
    pub generation: i64,
    pub revision: i64,
    pub reply: mpsc::SyncSender<DockerAck>,
}
impl Applied {
    pub fn finish(self, outcome: DockerOutcome, reason: Option<String>) {
        let _ = self.reply.send(DockerAck {
            generation: self.generation,
            revision: self.revision,
            outcome,
            reason,
        });
    }
}
pub(super) struct Pending {
    pub batch: DockerBatch,
    pub bytes: usize,
    pub reply: mpsc::SyncSender<DockerAck>,
}
impl Writer {
    /// Enqueue without waiting for SQLite. The returned receiver emits exactly one terminal
    /// receipt. A persisted receipt means COMMIT succeeded, not merely the operation savepoint.
    pub fn write_docker(&self, batch: DockerBatch) -> Result<mpsc::Receiver<DockerAck>> {
        let bytes = batch.logical_bytes()?;
        let (reply, rx) = mpsc::sync_channel(1);
        let pending = Pending {
            batch,
            bytes,
            reply,
        };
        {
            let mut a = self.docker_admission.lock_ok();
            if a.pending_writes >= MAX_PENDING_WRITES
                || a.pending_bytes.saturating_add(bytes) > MAX_PENDING_BYTES
                || self.s10_paused()
            {
                let _ = pending.reply.send(DockerAck {
                    generation: pending.batch.generation,
                    revision: pending.batch.revision,
                    outcome: DockerOutcome::Rejected,
                    reason: Some("Docker queue full or low disk".into()),
                });
                return Ok(rx);
            }
            a.pending_bytes += bytes;
            a.pending_writes += 1;
        }
        if let Err(err) = self.tx.send(Op::Docker(Box::new(pending)))
            && let Op::Docker(pending) = err.0
        {
            let mut a = self.docker_admission.lock_ok();
            a.pending_bytes -= bytes;
            a.pending_writes -= 1;
            let _ = pending.reply.send(DockerAck {
                generation: pending.batch.generation,
                revision: pending.batch.revision,
                outcome: DockerOutcome::Failed,
                reason: Some("store writer stopped".into()),
            });
        }
        Ok(rx)
    }
    /// Coordinator must hold this around SQLite snapshot establishment + immutable ring
    /// capture, and while applying ACKs that release ring persistence pins. Never flush
    /// while holding it: COMMIT acquires this same short lock.
    pub fn docker_snapshot_lock(&self) -> Arc<Mutex<()>> {
        Arc::clone(&self.docker_coordination)
    }
    pub fn set_docker_history_budget(&self, total_bytes: u64) {
        self.docker_admission.lock_ok().allowance = allowance(total_bytes);
    }
    pub fn docker_storage_status(&self) -> DockerStorageStatus {
        let a = self.docker_admission.lock_ok();
        DockerStorageStatus {
            logical_bytes: a.logical_bytes,
            pending_bytes: a.pending_bytes,
            pending_writes: a.pending_writes,
            allowance_bytes: a.allowance,
            paused: a.paused,
        }
    }
    /// Bounded shutdown barrier. A timeout leaves the queued flush owned by the
    /// writer; callers must not infer that unacknowledged data became durable.
    pub fn flush_docker(&self, timeout: std::time::Duration) -> Result<()> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.tx
            .send(Op::Flush(reply))
            .map_err(|_| StoreError::WriterGone)?;
        rx.recv_timeout(timeout)
            .map_err(|e| invalid(&format!("Docker flush: {e}")))?
    }
    /// Reset is serialized with queued writes. Caller bumps its generation and drops old
    /// ring generations before it can issue new requests.
    pub fn reset_docker(&self, host: HostId, generation: i64) -> Result<()> {
        self.call(|r| Op::ResetDocker(host, generation, r))
    }
}
impl State {
    pub(super) fn handle_docker(&mut self, pending: Pending) {
        let admission = self.docker_admission.lock_ok();
        let cap = admission.allowance;
        let reserved = admission.pending_bytes as u64;
        let paused = admission.paused;
        drop(admission);
        let res = self
            .docker_reclaim(cap.saturating_mul(4) / 5, 256)
            .and_then(|()| {
                let used = self.docker_used()?;
                let mut a = self.docker_admission.lock_ok();
                a.logical_bytes = used;
                if (paused && used >= cap.saturating_mul(4) / 5)
                    || used.saturating_add(reserved) > cap
                {
                    a.paused = true;
                    return Err(invalid("Docker logical allowance exhausted"));
                }
                a.paused = false;
                drop(a);
                self.apply(|s| {
                    s.insert_docker(&pending.batch)?;
                    if s.docker_used()? > cap {
                        return Err(invalid("Docker logical allowance exhausted"));
                    }
                    Ok(())
                })
            });
        match res {
            Ok(()) => {
                let mut a = self.docker_admission.lock_ok();
                a.pending_bytes = a.pending_bytes.saturating_sub(pending.bytes);
                a.pending_writes = a.pending_writes.saturating_sub(1);
                self.docker_pending.push(Applied {
                    generation: pending.batch.generation,
                    revision: pending.batch.revision,
                    reply: pending.reply,
                });
            }
            Err(err) => {
                let outcome = if matches!(&err,StoreError::Docker(s) if s=="stale generation") {
                    DockerOutcome::Reset
                } else if matches!(&err,StoreError::Docker(s) if s.contains("allowance")) {
                    DockerOutcome::Rejected
                } else {
                    DockerOutcome::Failed
                };
                self.finish_docker(pending, outcome, Some(err.to_string()));
            }
        }
    }
    pub(super) fn finish_docker(
        &mut self,
        p: Pending,
        outcome: DockerOutcome,
        reason: Option<String>,
    ) {
        let mut a = self.docker_admission.lock_ok();
        a.pending_bytes = a.pending_bytes.saturating_sub(p.bytes);
        a.pending_writes = a.pending_writes.saturating_sub(1);
        drop(a);
        let _ = p.reply.send(DockerAck {
            generation: p.batch.generation,
            revision: p.batch.revision,
            outcome,
            reason,
        });
    }
    pub(super) fn docker_used(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row(
                "SELECT (SELECT coalesce(sum(logical_bytes),0) FROM docker_accounting) + (SELECT coalesce(sum(2*(128+length(daemon_id)+length(runtime))),0) FROM docker_engines)",
                [],
                |r| r.get::<_, i64>(0),
            )?
            .max(0) as u64)
    }
    fn engine(&mut self, b: &DockerBatch) -> Result<(i64, i64)> {
        let host = self.host_ref(b.host)?;
        let reset: i64 = self.conn.query_row(
            "SELECT coalesce((SELECT value FROM meta WHERE key=?1),0)",
            [format!("docker_generation:{}", b.host)],
            |r| r.get(0),
        )?;
        if b.generation < reset {
            return Err(invalid("stale generation"));
        }
        // Retain generation tombstones, but never allow daemon churn to grow them
        // without bound. Existing identities continue recording at the limit.
        let full: bool = self.conn.query_row("SELECT (SELECT count(*) FROM docker_engines)>=256 AND NOT EXISTS(SELECT 1 FROM docker_engines WHERE host_id=?1 AND daemon_id=?2)", params![host,b.daemon_id], |r| r.get(0))?;
        if full {
            return Err(invalid("Docker engine identity allowance exhausted"));
        }
        self.conn.execute("INSERT INTO docker_engines(host_id,daemon_id,runtime,last_seen_ms,generation) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(host_id,daemon_id) DO NOTHING",params![host,b.daemon_id,b.runtime,b.end_ms,b.generation])?;
        let (engine, generation): (i64, i64) = self.conn.query_row(
            "SELECT id,generation FROM docker_engines WHERE host_id=?1 AND daemon_id=?2",
            params![host, b.daemon_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if generation > b.generation {
            return Err(invalid("stale generation"));
        }
        self.conn.execute("UPDATE docker_engines SET runtime=?1,last_seen_ms=max(last_seen_ms,?2),generation=?3,durable_revision=CASE WHEN generation=?3 THEN max(durable_revision,?4) ELSE ?4 END WHERE id=?5",params![b.runtime,b.end_ms,b.generation,b.revision,engine])?;
        Ok((host, engine))
    }
    fn entity(&self, host: i64, engine: i64, external: &str) -> Result<i64> {
        let kind = if external.is_empty() {
            "aggregate"
        } else {
            "container"
        };
        self.conn.execute("INSERT INTO docker_entities(host_id,engine_id,kind,external_id) VALUES(?1,?2,?3,?4) ON CONFLICT(engine_id,kind,external_id) DO NOTHING",params![host,engine,kind,external])?;
        Ok(self.conn.query_row(
            "SELECT id FROM docker_entities WHERE engine_id=?1 AND kind=?2 AND external_id=?3",
            params![engine, kind, external],
            |r| r.get(0),
        )?)
    }
    fn insert_docker(&mut self, b: &DockerBatch) -> Result<()> {
        let (host, engine) = self.engine(b)?;
        for e in &b.entities {
            let entity = self.entity(host, engine, &e.external_id)?;
            self.conn.execute("INSERT INTO docker_metadata(entity_id,engine_id,host_id,effective_ms,revision,name,image,project,service,lifecycle,explicit_limit,effective_limit,capacity,last_seen_ms,tombstone,extras) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16) ON CONFLICT(entity_id,effective_ms,revision) DO NOTHING",params![entity,engine,host,e.effective_ms,e.revision,e.name,e.image,e.project,e.service,e.lifecycle,e.explicit_limit,e.effective_limit,e.capacity,e.last_seen_ms,e.tombstone,e.extras])?;
        }
        for p in &b.populations {
            let inserted=self.conn.execute("INSERT INTO docker_populations(host_id,engine_id,revision,effective_ms) VALUES(?1,?2,?3,?4) ON CONFLICT(engine_id,revision) DO NOTHING",params![host,engine,p.revision,p.effective_ms])?;
            let id: i64 = self.conn.query_row(
                "SELECT id FROM docker_populations WHERE engine_id=?1 AND revision=?2",
                params![engine, p.revision],
                |r| r.get(0),
            )?;
            if inserted == 0 {
                let existing:Vec<String>=self.conn.prepare("SELECT e.external_id FROM docker_population_members m JOIN docker_entities e ON e.id=m.entity_id WHERE m.population_id=?1 ORDER BY e.external_id")?.query_map([id],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
                let mut members = p.members.clone();
                members.sort();
                members.dedup();
                if existing != members {
                    return Err(invalid("population revision is immutable"));
                }
                continue;
            }
            for external in &p.members {
                let entity = self.entity(host, engine, external)?;
                self.conn.execute("INSERT OR IGNORE INTO docker_population_members(population_id,entity_id,engine_id,host_id) VALUES(?1,?2,?3,?4)",params![id,entity,engine,host])?;
            }
        }
        for bucket in &b.buckets {
            self.insert_docker_bucket(host, engine, bucket)?;
        }
        for event in &b.events {
            let entity = event
                .entity
                .as_ref()
                .map(|e| self.entity(host, engine, e))
                .transpose()?;
            self.conn.execute("INSERT INTO docker_events(host_id,engine_id,entity_id,event_key,time_ns,time_ms,action,payload) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(engine_id,event_key) DO NOTHING",params![host,engine,entity,event.key(&b.daemon_id)?,event.time_ns,event.time_ns/1_000_000,event.action,pack(&event.attributes)?])?;
        }
        for c in &b.coverage {
            self.insert_docker_coverage(host, engine, c)?;
        }
        if !b.buckets.is_empty() {
            let open: Vec<(i64,i64)> = self.conn.prepare("SELECT id,start_ms FROM docker_coverage WHERE engine_id=?1 AND entity_id IS NULL AND reason='engine_switch' AND end_ms IS NULL")?.query_map([engine],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<std::result::Result<_,_>>()?;
            for (id, start) in open {
                if let Some(end) = resumed_after(&self.conn, engine, start)? {
                    self.conn.execute("UPDATE docker_coverage SET end_ms=?1,expected_ms=?1-start_ms,revision=max(revision,?2) WHERE id=?3",params![end,b.revision,id])?;
                }
            }
        }
        if let Some(c) = &b.cursor {
            self.conn.execute(
                "INSERT INTO docker_event_cursors(engine_id,time_ns,event_key,generation)
                 VALUES(?1,?2,?3,?4)
                 ON CONFLICT(engine_id) DO UPDATE SET
                   time_ns=excluded.time_ns,
                   event_key=excluded.event_key,
                   generation=excluded.generation
                 WHERE excluded.generation>generation OR
                   (excluded.generation=generation AND
                    (excluded.time_ns,excluded.event_key)>=(time_ns,event_key))",
                params![engine, c.time_ns, c.event_key, b.generation],
            )?;
        }
        Ok(())
    }
    fn insert_docker_coverage(&self, host: i64, engine: i64, c: &DockerCoverage) -> Result<()> {
        let entity = c
            .entity
            .as_ref()
            .map(|e| self.entity(host, engine, e))
            .transpose()?;
        let updated=self.conn.execute("UPDATE docker_coverage SET end_ms=coalesce(?1,end_ms),revision=?2,sampled_ms=?3,expected_ms=CASE WHEN ?1 IS NULL AND end_ms IS NOT NULL THEN expected_ms ELSE ?4 END WHERE engine_id=?5 AND entity_id IS ?6 AND metric IS ?7 AND start_ms=?8 AND reason=?9 AND revision<=?2",params![c.end_ms,c.revision,c.sampled_ms,c.expected_ms,engine,entity,c.metric,c.start_ms,c.reason])?;
        if updated > 0 {
            return Ok(());
        }
        self.conn.execute("INSERT INTO docker_coverage(host_id,engine_id,entity_id,metric,start_ms,end_ms,reason,revision,sampled_ms,expected_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![host,engine,entity,c.metric,c.start_ms,c.end_ms,c.reason,c.revision,c.sampled_ms,c.expected_ms])?;
        Ok(())
    }
    fn insert_docker_bucket(&self, host: i64, engine: i64, b: &DockerBucket) -> Result<()> {
        let entity = self.entity(host, engine, &b.entity)?;
        let keys: Vec<_> = b.metrics.iter().map(|m| m.key.as_str()).collect();
        let hash = String::from_utf8(
            pack(&keys)?
                .iter()
                .flat_map(|v| format!("{v:02x}").into_bytes())
                .collect(),
        )
        .map_err(|_| invalid("layout encoding"))?;
        self.conn.execute(
            "INSERT INTO docker_layouts(version,hash) VALUES(1,?1) ON CONFLICT(hash) DO NOTHING",
            [&hash],
        )?;
        let layout: i64 = self.conn.query_row(
            "SELECT id FROM docker_layouts WHERE hash=?1",
            [&hash],
            |r| r.get(0),
        )?;
        for (ordinal, key) in keys.iter().enumerate() {
            let (unit, kind) = metric_catalog(key).ok_or_else(|| invalid("unknown metric"))?;
            self.conn.execute("INSERT INTO docker_metrics(metric_key,unit,kind) VALUES(?1,?2,?3) ON CONFLICT(metric_key) DO NOTHING",params![key,unit,kind])?;
            self.conn.execute("INSERT OR IGNORE INTO docker_layout_members(layout_id,ordinal,metric_id) SELECT ?1,?2,id FROM docker_metrics WHERE metric_key=?3",params![layout,ordinal as i64,key])?;
        }
        let t = table(b.tier)?;
        let changed=self.conn.execute(&format!("INSERT INTO {t}(entity_id,bucket_ms,layout_id,engine_id,host_id,revision,payload) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(entity_id,bucket_ms,layout_id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload WHERE excluded.revision>{t}.revision"),params![entity,b.bucket_ms,layout,engine,host,b.revision,pack(&(1_u8,&b.metrics))?])?;
        if changed == 0 {
            return Ok(());
        }
        self.conn.execute(
            &format!(
                "DELETE FROM {t}_populations WHERE entity_id=?1 AND bucket_ms=?2 AND layout_id=?3"
            ),
            params![entity, b.bucket_ms, layout],
        )?;
        for (segment, p) in b.populations.iter().enumerate() {
            let population: i64 = self.conn.query_row(
                "SELECT id FROM docker_populations WHERE engine_id=?1 AND revision=?2",
                params![engine, p.revision],
                |r| r.get(0),
            )?;
            self.conn.execute(&format!("INSERT INTO {t}_populations(entity_id,bucket_ms,layout_id,engine_id,host_id,segment,start_ms,end_ms,population_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)"),params![entity,b.bucket_ms,layout,engine,host,segment as i64,p.start_ms,p.end_ms,population])?;
        }
        if b.tier == Tier::S10 {
            self.recompute_docker(
                host,
                engine,
                entity,
                layout,
                b.bucket_ms,
                Tier::S10,
                Tier::M1,
            )?;
        }
        if b.tier == Tier::M1 {
            self.recompute_docker(
                host,
                engine,
                entity,
                layout,
                b.bucket_ms,
                Tier::M1,
                Tier::M15,
            )?;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)] // All identifiers are required to enforce ownership in both tiers.
    fn recompute_docker(
        &self,
        host: i64,
        engine: i64,
        entity: i64,
        layout: i64,
        time: i64,
        source: Tier,
        target: Tier,
    ) -> Result<()> {
        let src = table(source)?;
        let dst = table(target)?;
        let width = target.bucket_ms().ok_or(StoreError::NotPersisted(target))?;
        let start = time.div_euclid(width) * width;
        let end = start + width;
        let rows:Vec<(i64,Vec<u8>)>=self.conn.prepare(&format!("SELECT revision,payload FROM {src} WHERE entity_id=?1 AND bucket_ms>=?2 AND bucket_ms<?3 AND layout_id=?4 ORDER BY bucket_ms"))?.query_map(params![entity,start,end,layout],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        let mut folded: Vec<WeightedMetric> = Vec::new();
        let mut revision = 0;
        for (r, payload) in rows {
            revision = revision.max(r);
            let (version, metrics): (u8, Vec<WeightedMetric>) = unpack(&payload)?;
            if version != 1 {
                return Err(invalid("unknown bucket version"));
            }
            if folded.is_empty() {
                folded = metrics;
            } else {
                for (a, b) in folded.iter_mut().zip(&metrics) {
                    a.merge(b)?;
                }
            }
        }
        // Expected duration covers the entire coarse bucket, so holes stay partial.
        for metric in &mut folded {
            metric.expected_ms = width as u64;
        }
        self.conn.execute(&format!("INSERT INTO {dst}(entity_id,bucket_ms,layout_id,engine_id,host_id,revision,payload) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(entity_id,bucket_ms,layout_id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload"),params![entity,start,layout,engine,host,revision,pack(&(1_u8,&folded))?])?;
        self.conn.execute(
            &format!(
                "DELETE FROM {dst}_populations WHERE entity_id=?1 AND bucket_ms=?2 AND layout_id=?3"
            ),
            params![entity, start, layout],
        )?;
        self.conn.execute(&format!("INSERT INTO {dst}_populations(entity_id,bucket_ms,layout_id,engine_id,host_id,segment,start_ms,end_ms,population_id) SELECT entity_id,?2,layout_id,engine_id,host_id,row_number() OVER (ORDER BY bucket_ms,segment),start_ms,end_ms,population_id FROM {src}_populations WHERE entity_id=?1 AND bucket_ms>=?2 AND bucket_ms<?3 AND layout_id=?4"),params![entity,start,end,layout])?;
        if target == Tier::M1 {
            self.recompute_docker(host, engine, entity, layout, start, Tier::M1, Tier::M15)?;
        }
        Ok(())
    }
}
impl State {
    /// Reclaims oldest rows in a bounded pass. Referenced populations/layouts and the
    /// latest metadata revision remain until their last relational owner disappears.
    pub(super) fn docker_reclaim(&mut self, target: u64, limit: usize) -> Result<()> {
        if self.docker_used()? <= target {
            return Ok(());
        }
        self.apply(|s| {
            for _ in 0..limit {
                if s.docker_used()?<=target {break;}
                let oldest:Option<(String,i64)>=s.conn.query_row("SELECT kind,id FROM (
                    SELECT 'docker_10s' kind,rowid id,min(bucket_ms) ts FROM docker_10s
                    UNION ALL SELECT 'docker_1m',rowid,min(bucket_ms) FROM docker_1m
                    UNION ALL SELECT 'docker_15m',rowid,min(bucket_ms) FROM docker_15m
                    UNION ALL SELECT 'docker_events',id,min(time_ms) FROM docker_events
                    UNION ALL SELECT 'docker_coverage',id,min(end_ms) FROM docker_coverage WHERE end_ms IS NOT NULL
                  ) WHERE id IS NOT NULL ORDER BY ts LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
                let Some((kind,id))=oldest else {break;};
                // `kind` is one of the SQL literals above, never caller input.
                s.conn.execute(&format!("DELETE FROM {kind} WHERE rowid=?1"),[id])?;
            }
            // Drop obsolete metadata in one bounded pass, retaining the revision in
            // effect at the earliest surviving sample/event (including its predecessor).
            s.conn.execute("DELETE FROM docker_metadata WHERE rowid IN (
                SELECT m.rowid FROM docker_metadata m WHERE EXISTS (
                  SELECT 1 FROM docker_metadata n WHERE n.entity_id=m.entity_id
                   AND (n.effective_ms,n.revision)>(m.effective_ms,m.revision)
                   AND n.effective_ms<=coalesce((SELECT min(t) FROM (
                      SELECT min(bucket_ms) t FROM docker_10s WHERE entity_id=m.entity_id
                      UNION ALL SELECT min(bucket_ms) FROM docker_1m WHERE entity_id=m.entity_id
                      UNION ALL SELECT min(bucket_ms) FROM docker_15m WHERE entity_id=m.entity_id
                      UNION ALL SELECT min(time_ms) FROM docker_events WHERE entity_id=m.entity_id
                   )),9223372036854775807)
                ) ORDER BY m.effective_ms LIMIT ?1)",[limit.min(5000) as i64])?;
            if s.docker_used()?>target {
                // Metadata-only churn is reclaimable too. Current monitored members and
                // every entity referenced by retained history remain relational owners.
                s.conn.execute("DELETE FROM docker_entities WHERE id IN (
                    SELECT e.id FROM docker_entities e WHERE NOT EXISTS(SELECT 1 FROM docker_10s b WHERE b.entity_id=e.id)
                     AND NOT EXISTS(SELECT 1 FROM docker_1m b WHERE b.entity_id=e.id)
                     AND NOT EXISTS(SELECT 1 FROM docker_15m b WHERE b.entity_id=e.id)
                     AND NOT EXISTS(SELECT 1 FROM docker_events v WHERE v.entity_id=e.id)
                     AND NOT EXISTS(SELECT 1 FROM docker_coverage c WHERE c.entity_id=e.id)
                     AND NOT EXISTS(SELECT 1 FROM docker_population_members p WHERE p.entity_id=e.id)
                    ORDER BY (SELECT min(effective_ms) FROM docker_metadata m WHERE m.entity_id=e.id) LIMIT ?1)",[limit.min(128) as i64])?;
            }
            s.docker_gc()?;
            Ok(())
        })
    }
    pub(super) fn docker_gc(&self) -> Result<()> {
        self.conn.execute_batch("DELETE FROM docker_populations WHERE NOT EXISTS(SELECT 1 FROM docker_10s_populations b WHERE b.population_id=docker_populations.id) AND NOT EXISTS(SELECT 1 FROM docker_1m_populations b WHERE b.population_id=docker_populations.id) AND NOT EXISTS(SELECT 1 FROM docker_15m_populations b WHERE b.population_id=docker_populations.id) AND revision < (SELECT max(p.revision) FROM docker_populations p WHERE p.engine_id=docker_populations.engine_id);
            DELETE FROM docker_layouts WHERE NOT EXISTS(SELECT 1 FROM docker_10s b WHERE b.layout_id=docker_layouts.id) AND NOT EXISTS(SELECT 1 FROM docker_1m b WHERE b.layout_id=docker_layouts.id) AND NOT EXISTS(SELECT 1 FROM docker_15m b WHERE b.layout_id=docker_layouts.id);
            DELETE FROM docker_metrics WHERE NOT EXISTS(SELECT 1 FROM docker_layout_members m WHERE m.metric_id=docker_metrics.id);")?;
        Ok(())
    }
    pub(super) fn docker_prune(
        &mut self,
        now: i64,
        history_ms: i64,
        total_bytes: u64,
    ) -> Result<()> {
        self.docker_admission.lock_ok().allowance = allowance(total_bytes);
        for (t, age, col) in [
            ("docker_10s", 86_400_000, "bucket_ms"),
            ("docker_1m", 7 * 86_400_000, "bucket_ms"),
            ("docker_15m", MAX_RANGE_MS, "bucket_ms"),
            ("docker_events", MAX_RANGE_MS, "time_ms"),
            ("docker_coverage", MAX_RANGE_MS, "end_ms"),
        ] {
            let cut = now.saturating_sub(age.min(history_ms));
            loop {
                let n=self.apply(|s|Ok(s.conn.execute(&format!("DELETE FROM {t} WHERE rowid IN(SELECT rowid FROM {t} WHERE {col}<?1 LIMIT ?2)"),params![cut,s.prune_batch.min(i64::MAX as u64) as i64])?))?;
                self.commit()?;
                if n == 0 {
                    break;
                }
                self.yield_to_queue()?;
            }
        }
        self.apply(|s| {
            // Keep the predecessor revision at the retained left edge, and all newer facts.
            s.conn.execute("DELETE FROM docker_metadata WHERE effective_ms<?1 AND EXISTS(SELECT 1 FROM docker_metadata newer WHERE newer.entity_id=docker_metadata.entity_id AND newer.effective_ms<?1 AND (newer.effective_ms,newer.revision)>(docker_metadata.effective_ms,docker_metadata.revision))",[now.saturating_sub(history_ms.min(MAX_RANGE_MS))])?;
            s.conn.execute("DELETE FROM docker_populations WHERE effective_ms<?1 AND NOT EXISTS(SELECT 1 FROM docker_10s_populations b WHERE b.population_id=docker_populations.id) AND NOT EXISTS(SELECT 1 FROM docker_1m_populations b WHERE b.population_id=docker_populations.id) AND NOT EXISTS(SELECT 1 FROM docker_15m_populations b WHERE b.population_id=docker_populations.id)",[now.saturating_sub(history_ms.min(MAX_RANGE_MS))])?;
            s.conn.execute("DELETE FROM docker_entities WHERE id IN(SELECT e.id FROM docker_entities e WHERE EXISTS(SELECT 1 FROM docker_metadata m WHERE m.entity_id=e.id) AND (SELECT max(last_seen_ms) FROM docker_metadata m WHERE m.entity_id=e.id)<?1 AND NOT EXISTS(SELECT 1 FROM docker_10s b WHERE b.entity_id=e.id) AND NOT EXISTS(SELECT 1 FROM docker_1m b WHERE b.entity_id=e.id) AND NOT EXISTS(SELECT 1 FROM docker_15m b WHERE b.entity_id=e.id) AND NOT EXISTS(SELECT 1 FROM docker_events v WHERE v.entity_id=e.id) AND NOT EXISTS(SELECT 1 FROM docker_coverage c WHERE c.entity_id=e.id) AND NOT EXISTS(SELECT 1 FROM docker_population_members p WHERE p.entity_id=e.id))",[now.saturating_sub(history_ms.min(MAX_RANGE_MS))])?;
            s.docker_gc()
        })?;
        for _ in 0..16 {
            let target = allowance(total_bytes) * 4 / 5;
            let before = self.docker_used()?;
            self.docker_reclaim(target, self.prune_batch.min(5000) as usize)?;
            self.commit()?;
            if self.docker_used()? <= target || self.docker_used()? == before {
                break;
            }
            self.yield_to_queue()?;
        }
        let used = self.docker_used()?;
        let mut a = self.docker_admission.lock_ok();
        a.logical_bytes = used;
        if used < a.allowance * 4 / 5 {
            a.paused = false;
        }
        Ok(())
    }
    pub(super) fn docker_discard(&self, host: i64, from: i64) -> Result<()> {
        for (t, width) in [
            ("docker_10s", 10_000),
            ("docker_1m", 60_000),
            ("docker_15m", 900_000),
        ] {
            // Drop crossing buckets: partial recomputation without all source observations
            // would invent a fully observed remainder.
            self.conn.execute(
                &format!("DELETE FROM {t} WHERE host_id=?1 AND bucket_ms+?2>?3"),
                params![host, width, from],
            )?;
        }
        self.conn.execute(
            "DELETE FROM docker_events WHERE host_id=?1 AND time_ms>=?2",
            params![host, from],
        )?;
        self.conn.execute(
            "DELETE FROM docker_metadata WHERE host_id=?1 AND effective_ms>=?2",
            params![host, from],
        )?;
        self.conn.execute(
            "DELETE FROM docker_coverage WHERE host_id=?1 AND start_ms>=?2",
            params![host, from],
        )?;
        self.conn.execute("UPDATE docker_coverage SET end_ms=?2,expected_ms=min(expected_ms,?2-start_ms),sampled_ms=min(sampled_ms,?2-start_ms) WHERE host_id=?1 AND (end_ms>?2 OR end_ms IS NULL)",params![host,from])?;
        self.conn.execute("DELETE FROM docker_event_cursors WHERE engine_id IN(SELECT id FROM docker_engines WHERE host_id=?1) AND time_ns/1000000>=?2",params![host,from])?;
        self.docker_gc()
    }
    pub(super) fn docker_reset_rows(&mut self, host: HostId, generation: i64) -> Result<()> {
        let h = self.host_ref(host)?;
        for t in [
            "docker_10s",
            "docker_1m",
            "docker_15m",
            "docker_events",
            "docker_coverage",
            "docker_metadata",
            "docker_population_members",
            "docker_populations",
            "docker_entities",
        ] {
            self.conn
                .execute(&format!("DELETE FROM {t} WHERE host_id=?1"), [h])?;
        }
        self.conn.execute("DELETE FROM docker_event_cursors WHERE engine_id IN(SELECT id FROM docker_engines WHERE host_id=?1)",[h])?;
        self.conn.execute("DELETE FROM docker_accounting WHERE engine_id IN(SELECT id FROM docker_engines WHERE host_id=?1)",[h])?;
        self.conn.execute("UPDATE docker_engines SET generation=CASE WHEN ?2=0 THEN generation+1 ELSE max(generation,?2) END,durable_revision=0 WHERE host_id=?1",params![h,generation])?;
        self.conn.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=CASE WHEN excluded.value=0 THEN value+1 ELSE max(value,excluded.value) END",params![format!("docker_generation:{host}"),generation])?;
        self.docker_gc()?;
        Ok(())
    }
    pub(super) fn reset_docker(&mut self, host: HostId, generation: i64) -> Result<()> {
        // Roll back the old Docker ops with the shared batch? Host history must survive:
        // commit it, then erase Docker in a distinct atomic transaction.
        self.commit_logged();
        self.apply(|s| s.docker_reset_rows(host, generation))?;
        self.commit()?;
        Ok(())
    }
}

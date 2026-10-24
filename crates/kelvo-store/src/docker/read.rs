//! Bounded read transactions. Call begin under the writer/ring coordination lock.
mod catalog;

use super::*;
use crate::{Result, StoreError};
use kelvo_schema::HostId;
use rusqlite::{Connection, OptionalExtension, params};
use std::cell::OnceCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
static READERS: AtomicUsize = AtomicUsize::new(0);
const MAX_READERS: usize = 4;
const MAX_LIFETIME: Duration = Duration::from_secs(2);

pub struct DockerReadSnapshot<'a> {
    conn: &'a Connection,
    engine: Option<i64>,
    host: HostId,
    started: Instant,
    retained_range: OnceCell<Option<(i64, i64)>>,
    pub generation: i64,
    pub durable_revision: i64,
}
impl Drop for DockerReadSnapshot<'_> {
    fn drop(&mut self) {
        let _ = self.conn.execute_batch("ROLLBACK");
        READERS.fetch_sub(1, Ordering::AcqRel);
    }
}
pub(crate) fn begin<'a>(
    conn: &'a Connection,
    host: HostId,
    daemon: &str,
) -> Result<DockerReadSnapshot<'a>> {
    if !conn.is_autocommit() {
        return Err(invalid("reader already has a snapshot"));
    }
    READERS
        .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < MAX_READERS).then_some(n + 1)
        })
        .map_err(|_| invalid("four Docker history readers are active"))?;
    let result = (|| {
        conn.execute_batch("BEGIN")?;
        // This SELECT actually establishes the WAL read snapshot. BEGIN alone does not.
        let row:Option<(i64,i64,i64)>=conn.query_row("SELECT e.id,e.generation,e.durable_revision FROM docker_engines e JOIN hosts h ON h.id=e.host_id WHERE h.uuid=?1 AND e.daemon_id=?2",params![host.to_string(),daemon],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let (engine, generation, durable_revision) =
            row.map(|(e, g, r)| (Some(e), g, r)).unwrap_or((None, 0, 0));
        Ok(DockerReadSnapshot {
            conn,
            host,
            engine,
            generation,
            durable_revision,
            started: Instant::now(),
            retained_range: OnceCell::new(),
        })
    })();
    if result.is_err() {
        let _ = conn.execute_batch("ROLLBACK");
        READERS.fetch_sub(1, Ordering::AcqRel);
    }
    result
}
impl DockerReadSnapshot<'_> {
    fn check(&self, bytes: usize) -> Result<()> {
        if bytes > MAX_QUERY_BYTES {
            return Err(invalid("history exceeds response byte bound"));
        }
        if self.started.elapsed() > MAX_LIFETIME {
            return Err(invalid("history snapshot expired"));
        }
        Ok(())
    }
    /// Latest metadata for each container with any retained bucket in the range.
    /// Reads all tiers in this transaction so stopped, removed and recreated names
    /// retain their original identities. The caller ranks before truncating.
    pub fn comparison_candidates(&self, start: i64, end: i64) -> Result<Vec<DockerEntity>> {
        bounds(start, end, 0)?;
        let mut stmt = self.conn.prepare("SELECT e.external_id,m.effective_ms,m.revision,m.name,m.image,m.project,m.service,m.lifecycle,m.explicit_limit,m.effective_limit,m.capacity,m.last_seen_ms,m.tombstone,m.extras FROM docker_entities e JOIN docker_metadata m ON m.rowid=(SELECT rowid FROM docker_metadata latest WHERE latest.entity_id=e.id ORDER BY latest.effective_ms DESC,latest.revision DESC LIMIT 1) WHERE e.engine_id=?1 AND e.kind='container' AND (EXISTS(SELECT 1 FROM docker_10s b WHERE b.entity_id=e.id AND b.bucket_ms>=?2 AND b.bucket_ms<?3) OR EXISTS(SELECT 1 FROM docker_1m b WHERE b.entity_id=e.id AND b.bucket_ms>=?2 AND b.bucket_ms<?3) OR EXISTS(SELECT 1 FROM docker_15m b WHERE b.entity_id=e.id AND b.bucket_ms>=?2 AND b.bucket_ms<?3)) ORDER BY e.external_id")?;
        let mut rows = stmt.query(params![self.engine, start, end])?;
        let mut result = Vec::new();
        let mut bytes = 0;
        while let Some(r) = rows.next()? {
            let entity = DockerEntity {
                external_id: r.get(0)?,
                effective_ms: r.get(1)?,
                revision: r.get(2)?,
                name: r.get(3)?,
                image: r.get(4)?,
                project: r.get(5)?,
                service: r.get(6)?,
                lifecycle: r.get(7)?,
                explicit_limit: r.get(8)?,
                effective_limit: r.get(9)?,
                capacity: r.get(10)?,
                last_seen_ms: r.get(11)?,
                tombstone: r.get(12)?,
                extras: r.get(13)?,
            };
            bytes += entity.extras.len() + entity.name.len() + entity.image.len() + 512;
            self.check(bytes)?;
            result.push(entity);
        }
        Ok(result)
    }
    fn entity(&self, id: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM docker_entities WHERE engine_id=?1 AND kind=?2 AND external_id=?3",
                params![
                    self.engine,
                    if id.is_empty() {
                        "aggregate"
                    } else {
                        "container"
                    },
                    id
                ],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub fn retained_range(&self) -> Result<Option<(i64, i64)>> {
        self.check(0)?;
        // Every candidate in a comparison shares this immutable WAL snapshot.
        if let Some(range) = self.retained_range.get() {
            return Ok(*range);
        }
        let (start,end):(Option<i64>,Option<i64>)=self.conn.query_row("SELECT min(first),max(last) FROM (SELECT min(bucket_ms) first,max(bucket_ms+10000) last FROM docker_10s WHERE engine_id=?1 UNION ALL SELECT min(bucket_ms),max(bucket_ms+60000) FROM docker_1m WHERE engine_id=?1 UNION ALL SELECT min(bucket_ms),max(bucket_ms+900000) FROM docker_15m WHERE engine_id=?1)",[self.engine],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let range = start.zip(end);
        let _ = self.retained_range.set(range);
        Ok(range)
    }
    pub fn cursor(&self) -> Result<Option<DockerCursor>> {
        self.check(0)?;
        Ok(self
            .conn
            .query_row(
                "SELECT time_ns,event_key FROM docker_event_cursors WHERE engine_id=?1",
                [self.engine],
                |r| {
                    Ok(DockerCursor {
                        time_ns: r.get(0)?,
                        event_key: r.get(1)?,
                    })
                },
            )
            .optional()?)
    }
    pub fn history(&self, q: &DockerHistoryQuery) -> Result<DockerHistory> {
        bounds(q.start_ms, q.end_ms, 900_000)?;
        if q.max_points == 0 || q.max_points > MAX_POINTS {
            return Err(invalid("invalid point bound"));
        }
        let t = table(q.tier)?;
        let Some(entity) = self.entity(&q.entity)? else {
            return Ok(DockerHistory::default());
        };
        let mut out = DockerHistory::default();
        let mut bytes = 0;
        let mut stmt=self.conn.prepare_cached(&format!("SELECT bucket_ms,layout_id,revision,payload FROM {t} WHERE entity_id=?1 AND bucket_ms>=?2 AND bucket_ms<?3 ORDER BY bucket_ms,layout_id LIMIT ?4"))?;
        let mut rows = stmt.query(params![
            entity,
            q.start_ms,
            q.end_ms,
            (q.max_points + 1) as i64
        ])?;
        let mut bucket_keys = std::collections::HashMap::new();
        while let Some(r) = rows.next()? {
            let payload: Vec<u8> = r.get(3)?;
            bytes += payload.len();
            self.check(bytes)?;
            if out.buckets.len() == q.max_points {
                return Err(invalid("resolution exceeds point bound"));
            }
            let (version, metrics): (u8, Vec<WeightedMetric>) = unpack(&payload)?;
            if version != 1 {
                return Err(invalid("unknown bucket version"));
            }
            let time = r.get(0)?;
            let layout: i64 = r.get(1)?;
            bucket_keys.insert((time, layout), out.buckets.len());
            out.buckets.push(DockerBucket {
                entity: q.entity.clone(),
                tier: q.tier,
                bucket_ms: time,
                revision: r.get(2)?,
                metrics,
                populations: Vec::new(),
            });
        }
        // One bounded relation query, never one metadata/population lookup per bucket.
        let mut stmt=self.conn.prepare_cached(&format!("SELECT b.bucket_ms,b.layout_id,p.revision,b.start_ms,b.end_ms FROM {t}_populations b JOIN docker_populations p ON p.id=b.population_id WHERE b.entity_id=?1 AND b.bucket_ms>=?2 AND b.bucket_ms<?3 ORDER BY b.bucket_ms,b.layout_id,b.segment LIMIT ?4"))?;
        let mut rows = stmt.query(params![
            entity,
            q.start_ms,
            q.end_ms,
            (MAX_POINTS * 128 + 1) as i64
        ])?;
        while let Some(r) = rows.next()? {
            bytes += 64;
            self.check(bytes)?;
            let key = (r.get::<_, i64>(0)?, r.get::<_, i64>(1)?);
            if let Some(i) = bucket_keys.get(&key)
                && let Some(b) = out.buckets.get_mut(*i)
            {
                b.populations.push(PopulationSegment {
                    revision: r.get(2)?,
                    start_ms: r.get(3)?,
                    end_ms: r.get(4)?,
                });
            }
        }
        let mut stmt=self.conn.prepare_cached("SELECT effective_ms,revision,name,image,project,service,lifecycle,explicit_limit,effective_limit,capacity,last_seen_ms,tombstone,extras FROM docker_metadata WHERE entity_id=?1 AND ((effective_ms>=?2 AND effective_ms<?3) OR (effective_ms,revision)=(SELECT effective_ms,revision FROM docker_metadata WHERE entity_id=?1 AND effective_ms<?2 ORDER BY effective_ms DESC,revision DESC LIMIT 1)) ORDER BY effective_ms,revision LIMIT 10001")?;
        let mut rows = stmt.query(params![entity, q.start_ms, q.end_ms])?;
        while let Some(r) = rows.next()? {
            let e = DockerEntity {
                external_id: q.entity.clone(),
                effective_ms: r.get(0)?,
                revision: r.get(1)?,
                name: r.get(2)?,
                image: r.get(3)?,
                project: r.get(4)?,
                service: r.get(5)?,
                lifecycle: r.get(6)?,
                explicit_limit: r.get(7)?,
                effective_limit: r.get(8)?,
                capacity: r.get(9)?,
                last_seen_ms: r.get(10)?,
                tombstone: r.get(11)?,
                extras: r.get(12)?,
            };
            bytes += pack(&e)?.len();
            self.check(bytes)?;
            if out.metadata.len() >= 10_000 {
                return Err(invalid("metadata response count exceeded"));
            }
            out.metadata.push(e);
        }
        out.coverage = self.coverage(Some(entity), q.start_ms, q.end_ms, &mut bytes)?;
        Ok(out)
    }
    pub fn event_coverage(
        &self,
        entity: Option<&str>,
        start: i64,
        end: i64,
    ) -> Result<Vec<DockerCoverage>> {
        bounds(start, end, 0)?;
        let entity = entity
            .map(|id| self.entity(id).map(|id| id.unwrap_or(-1)))
            .transpose()?;
        self.coverage(entity, start, end, &mut 0)
    }
    fn coverage(
        &self,
        entity: Option<i64>,
        start: i64,
        end: i64,
        bytes: &mut usize,
    ) -> Result<Vec<DockerCoverage>> {
        let mut stmt=self.conn.prepare_cached("SELECT c.start_ms,c.end_ms,c.metric,c.reason,c.revision,c.sampled_ms,c.expected_ms,e.external_id FROM docker_coverage c LEFT JOIN docker_entities e ON e.id=c.entity_id WHERE c.engine_id=?1 AND c.start_ms<?3 AND (c.end_ms IS NULL OR c.end_ms>?2) AND (?4 IS NULL OR c.entity_id IS NULL OR c.entity_id=?4) ORDER BY c.start_ms,c.id LIMIT 10001")?;
        let mut rows = stmt.query(params![self.engine, start, end, entity])?;
        let mut result = Vec::new();
        while let Some(r) = rows.next()? {
            let mut c = DockerCoverage {
                start_ms: r.get(0)?,
                end_ms: r.get(1)?,
                metric: r.get(2)?,
                reason: r.get(3)?,
                revision: r.get(4)?,
                sampled_ms: r.get(5)?,
                expected_ms: r.get(6)?,
                entity: r.get(7)?,
            };
            if super::resumes_on_observation(&c.reason)
                && c.end_ms.is_none()
                && c.entity.is_none()
                && c.metric.is_none()
                && let Some(engine) = self.engine
                && let Some(end) = super::resumed_after(self.conn, engine, c.start_ms)?
            {
                c.end_ms = Some(end);
                c.expected_ms = end - c.start_ms;
            }
            if c.end_ms.is_some_and(|closed| closed <= start) {
                continue;
            }
            *bytes += pack(&c)?.len();
            self.check(*bytes)?;
            if result.len() >= 10_000 {
                return Err(invalid("coverage response count exceeded"));
            }
            result.push(c);
        }
        Ok(result)
    }
    pub fn events(&self, q: &DockerEventQuery) -> Result<Vec<DockerEventRow>> {
        bounds(q.start_ms, q.end_ms, 0)?;
        let limit = if q.limit == 0 { 200 } else { q.limit.min(1000) };
        let entity = q
            .entity
            .as_deref()
            .map(|e| self.entity(e))
            .transpose()?
            .flatten();
        if q.entity.is_some() && entity.is_none() {
            return Ok(Vec::new());
        }
        let (filter, id) = if entity.is_some() {
            ("v.entity_id", entity)
        } else {
            ("v.engine_id", self.engine)
        };
        let (after_time, after_id) = q.after.clone().unwrap_or((i64::MIN, String::new()));
        let mut stmt=self.conn.prepare_cached(&format!("SELECT v.id,v.event_key,v.time_ns,v.action,v.payload,e.external_id FROM docker_events v LEFT JOIN docker_entities e ON e.id=v.entity_id WHERE {filter}=?1 AND v.time_ms>=?2 AND v.time_ms<?3 AND (v.time_ns,v.event_key)>(?4,?5) ORDER BY v.time_ns,v.event_key LIMIT ?6"))?;
        let mut rows = stmt.query(params![
            id,
            q.start_ms,
            q.end_ms,
            after_time,
            after_id,
            limit as i64
        ])?;
        let mut out = Vec::new();
        let mut bytes = 0;
        while let Some(r) = rows.next()? {
            let payload: Vec<u8> = r.get(4)?;
            bytes += payload.len() + 256;
            self.check(bytes)?;
            out.push(DockerEventRow {
                id: r.get(0)?,
                event_key: r.get(1)?,
                event: DockerEvent {
                    time_ns: r.get(2)?,
                    action: r.get(3)?,
                    attributes: unpack(&payload)?,
                    entity: r.get(5)?,
                },
            });
        }
        Ok(out)
    }
}
fn bounds(start: i64, end: i64, alignment_slop: i64) -> Result<()> {
    if end <= start || end.saturating_sub(start) > MAX_RANGE_MS + alignment_slop {
        return Err(StoreError::Docker(
            "history range must be positive and at most 30 days".into(),
        ));
    }
    Ok(())
}

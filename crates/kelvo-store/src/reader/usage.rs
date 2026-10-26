//! Snapshot-safe durable/recent attribution. App reads never load process blobs.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};

use kelvo_schema::durable_usage::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::Reader;
use crate::usage::{
    UsageBucket, codec,
    query::{QueryFacts, figures, rank},
};
use crate::{Result, StoreError};

impl Reader {
    pub fn historical_usage_after(
        &mut self,
        request: &HistoricalUsageRequest,
        recent: impl FnOnce() -> Vec<UsageBucket>,
    ) -> Result<HistoricalUsageReply> {
        request
            .validate()
            .map_err(|e| StoreError::Usage(e.into()))?;
        let host = self.host_ref(request.host)?;
        let tx = self.conn.unchecked_transaction()?;
        // Establish the durable snapshot before taking recent data. New commits can
        // never disappear between the durable and recent halves of this read.
        tx.query_row(
            "SELECT count(*) FROM usage_recording_start WHERE host_id=?1",
            [host],
            |r| r.get::<_, i64>(0),
        )?;
        let recent = recent();
        let mut facts = load(&tx, host, request.from_ms, request.to_ms, None, &recent)?;
        tx.commit()?;
        let range = facts.range(request.from_ms, request.to_ms);
        let mut apps: Vec<HistoricalUsageApp> = if request.app_names.is_empty() {
            facts
                .apps
                .iter()
                .map(|(name, f)| HistoricalUsageApp {
                    name: name.clone(),
                    figures: figures(Some(f), &range),
                    process_details_available: f.coverage[..4].iter().any(|c| c.measured_ms > 0),
                })
                .collect()
        } else {
            request
                .app_names
                .iter()
                .map(|name| HistoricalUsageApp {
                    name: name.clone(),
                    figures: figures(facts.apps.get(name), &range),
                    process_details_available: facts
                        .apps
                        .get(name)
                        .is_some_and(|f| f.coverage[..4].iter().any(|c| c.measured_ms > 0)),
                })
                .collect()
        };
        if request.app_names.is_empty() {
            apps.sort_by(|a, b| {
                rank(&b.figures, request.by)
                    .total_cmp(&rank(&a.figures, request.by))
                    .then(a.name.cmp(&b.name))
            });
            apps.truncate(10);
        }
        let names: HashSet<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        let mut uncertainty: BTreeMap<Option<String>, UsageNetworkUncertainty> = BTreeMap::new();
        for mut u in facts.uncertainty.drain(..) {
            if u.app_name.as_deref().is_none_or(|n| !names.contains(n)) {
                u.app_name = None;
            }
            uncertainty
                .entry(u.app_name.clone())
                .and_modify(|old| {
                    old.possible_from_ms = old.possible_from_ms.min(u.possible_from_ms);
                    old.possible_to_ms = old.possible_to_ms.max(u.possible_to_ms);
                    old.rx_bytes = old.rx_bytes.saturating_add(u.rx_bytes);
                    old.tx_bytes = old.tx_bytes.saturating_add(u.tx_bytes);
                    old.records = old.records.saturating_add(u.records);
                })
                .or_insert(u);
        }
        let ranking_complete = range
            .coverage
            .iter()
            .find(|c| c.metric == request.by)
            .is_some_and(|c| c.coverage.complete);
        Ok(HistoricalUsageReply {
            range,
            by: request.by,
            apps,
            ranking_complete,
            network_uncertainty: uncertainty.into_values().collect(),
        })
    }

    pub fn usage_processes_after(
        &mut self,
        request: &UsageProcessRequest,
        recent: impl FnOnce() -> Vec<UsageBucket>,
    ) -> Result<UsageProcessReply> {
        request
            .validate()
            .map_err(|e| StoreError::Usage(e.into()))?;
        let host = self.host_ref(request.host)?;
        let epoch = self.epoch;
        let tx = self.conn.unchecked_transaction()?;
        tx.query_row(
            "SELECT count(*) FROM usage_recording_start WHERE host_id=?1",
            [host],
            |r| r.get::<_, i64>(0),
        )?;
        let recent = recent();
        let facts = load(
            &tx,
            host,
            request.from_ms,
            request.to_ms,
            Some(&request.app_name),
            &recent,
        )?;
        tx.commit()?;
        let range = facts.range(request.from_ms, request.to_ms);
        let mut processes: Vec<HistoricalUsageProcess> = facts
            .processes
            .iter()
            .map(|((pid, start, name), f)| HistoricalUsageProcess {
                pid: *pid,
                start_time_us: *start,
                name: name.clone(),
                figures: figures(Some(f), &range),
            })
            .collect();
        processes.sort_by(|a, b| {
            rank(&b.figures, request.by)
                .total_cmp(&rank(&a.figures, request.by))
                .then(a.pid.cmp(&b.pid))
                .then(a.start_time_us.cmp(&b.start_time_us))
        });
        let revision = revision(&facts);
        let mut offset = 0;
        if let Some(cursor) = &request.cursor {
            let c: ProcessCursor = serde_json::from_str(cursor)
                .map_err(|_| StoreError::Usage("invalid process continuation".into()))?;
            if c.epoch != epoch
                || c.host != request.host
                || c.app != request.app_name
                || c.from != request.from_ms
                || c.to != request.to_ms
                || c.by != request.by
                || c.revision != revision
            {
                return Err(StoreError::Usage(
                    "process history changed; restart expansion".into(),
                ));
            }
            offset = processes
                .iter()
                .position(|p| p.pid == c.pid && p.start_time_us == c.start)
                .map(|p| p + 1)
                .ok_or_else(|| StoreError::Usage("process continuation no longer exists".into()))?;
        }
        let count = usize::from(request.page_size);
        let more = processes.len() > offset.saturating_add(count);
        let shown: Vec<_> = processes.into_iter().skip(offset).take(count).collect();
        let next_cursor = if more {
            shown
                .last()
                .map(|p| {
                    serde_json::to_string(&ProcessCursor {
                        epoch,
                        host: request.host,
                        app: request.app_name.clone(),
                        from: request.from_ms,
                        to: request.to_ms,
                        by: request.by,
                        revision,
                        pid: p.pid,
                        start: p.start_time_us,
                    })
                })
                .transpose()
                .map_err(|e| StoreError::Usage(e.to_string()))?
        } else {
            None
        };
        Ok(UsageProcessReply {
            range,
            app_name: request.app_name.clone(),
            processes: shown,
            network_app_level_only: true,
            next_cursor,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct ProcessCursor {
    epoch: uuid::Uuid,
    host: kelvo_schema::HostId,
    app: String,
    from: i64,
    to: i64,
    by: HistoricalUsageMetric,
    revision: u64,
    pid: i32,
    start: i64,
}

fn revision(facts: &QueryFacts) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    facts.recorded_from.hash(&mut h);
    facts.retained_from.hash(&mut h);
    facts.boundary.hash(&mut h);
    for c in facts.source {
        c.supported.hash(&mut h);
        c.measured_ms.hash(&mut h);
        c.causes.hash(&mut h);
    }
    let mut spans = facts.spans.clone();
    spans.sort_by_key(|s| (s.from_ms, s.to_ms, s.resolution_ms));
    for s in spans {
        s.from_ms.hash(&mut h);
        s.to_ms.hash(&mut h);
        s.resolution_ms.hash(&mut h);
    }
    for u in &facts.uncertainty {
        u.app_name.hash(&mut h);
        u.possible_from_ms.hash(&mut h);
        u.possible_to_ms.hash(&mut h);
        u.rx_bytes.hash(&mut h);
        u.tx_bytes.hash(&mut h);
        u.records.hash(&mut h);
    }
    for (key, f) in &facts.processes {
        key.hash(&mut h);
        for v in [f.cpu_seconds, f.gpu_seconds, f.energy_j, f.mem_byte_ms] {
            v.to_bits().hash(&mut h);
        }
        f.mem_peak_bytes.hash(&mut h);
        f.present_ms.hash(&mut h);
        for c in f.coverage {
            c.measured_ms.hash(&mut h);
            c.causes.hash(&mut h);
            c.supported.hash(&mut h);
        }
    }
    h.finish()
}

fn load(
    conn: &Connection,
    host: i64,
    from: i64,
    to: i64,
    process_app: Option<&str>,
    recent: &[UsageBucket],
) -> Result<QueryFacts> {
    let (recorded_from, retained_from) = recording_bounds(conn, host)?;
    let recent: Vec<_> = recent
        .iter()
        .filter(|b| b.from_ms >= retained_from)
        .cloned()
        .collect();
    let recent = recent.as_slice();
    let mut out = QueryFacts {
        recorded_from,
        retained_from,
        ..QueryFacts::default()
    };
    let apps: HashMap<u64, String> = conn
        .prepare_cached("SELECT id,name FROM usage_apps WHERE host_id=?1")?
        .query_map([host], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let process_ids: HashMap<u64, (i32, i64, String)> = if let Some(app) = process_app {
        conn.prepare_cached("SELECT p.id,p.pid,p.start_time_us,p.name FROM usage_processes p JOIN usage_apps a ON a.id=p.app_id WHERE p.host_id=?1 AND a.name=?2")?.query_map(params![host,app],|r|Ok((r.get::<_,i64>(0)? as u64,(r.get(1)?,r.get(2)?,r.get(3)?))))?.collect::<rusqlite::Result<_>>()?
    } else {
        HashMap::new()
    };
    let include: HashSet<u64> = process_ids.keys().copied().collect();
    let table = if process_app.is_some() {
        "usage_process_buckets"
    } else {
        "usage_app_buckets"
    };
    let mut stmt=conn.prepare_cached(&format!("SELECT b.bucket_ts,b.width_ms,b.seq,b.blob,c.blob FROM {table} b JOIN usage_coverage c ON c.host_id=b.host_id AND c.width_ms=b.width_ms AND c.bucket_ts=b.bucket_ts WHERE b.host_id=?1 AND b.bucket_ts<?3 AND b.bucket_ts+b.width_ms>?2 ORDER BY b.bucket_ts,b.width_ms"))?;
    let mut rows = stmt.query(params![host, from, to])?;
    while let Some(row) = rows.next()? {
        let at: i64 = row.get(0)?;
        let width: i64 = row.get(1)?;
        if at < from || at + width > to {
            out.boundary = true;
            continue;
        }
        if recent
            .iter()
            .any(|b| b.from_ms < at + width && b.to_ms > at)
        {
            continue;
        }
        let seq: i64 = row.get(2)?;
        out.revision = out.revision.max(seq);
        let blob: Vec<u8> = row.get(3)?;
        let source: Vec<u8> = row.get(4)?;
        let source = codec::decode(&source)?
            .into_iter()
            .next()
            .map(|(_, f)| f.coverage)
            .ok_or_else(|| StoreError::Corrupt("missing usage source coverage".into()))?;
        out.observe(at, at + width, source);
        let decoded = if process_app.is_some() {
            codec::decode_selected(&blob, Some(&include))?
        } else {
            codec::decode(&blob)?
        };
        for (id, facts) in decoded {
            if process_app.is_some() {
                let key = process_ids
                    .get(&id)
                    .ok_or_else(|| StoreError::Corrupt("unknown usage process".into()))?;
                out.processes.entry(key.clone()).or_default().merge(&facts);
            } else {
                let name = apps
                    .get(&id)
                    .ok_or_else(|| StoreError::Corrupt("unknown usage app".into()))?;
                out.apps.entry(name.clone()).or_default().merge(&facts);
            }
        }
    }
    add_recent(&mut out, from, to, process_app, recent)?;
    let excluded = recent
        .iter()
        .map(|b| b.from_ms.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let mut uncertain=conn.prepare_cached(&format!("SELECT a.name,min(u.possible_from_ms),max(u.possible_to_ms),sum(u.rx_bytes),sum(u.tx_bytes),sum(u.records) FROM usage_uncertain u LEFT JOIN usage_apps a ON a.id=u.app_id WHERE u.host_id=?1 AND u.possible_from_ms<?3 AND u.possible_to_ms>?2 AND u.discovery_bucket_ts NOT IN ({excluded}) GROUP BY u.app_id"))?;
    out.uncertainty.extend(
        uncertain
            .query_map(params![host, from, to], |r| {
                Ok(UsageNetworkUncertainty {
                    app_name: r.get(0)?,
                    possible_from_ms: r.get(1)?,
                    possible_to_ms: r.get(2)?,
                    rx_bytes: nonnegative(r, 3)?,
                    tx_bytes: nonnegative(r, 4)?,
                    records: nonnegative(r, 5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    Ok(out)
}

fn nonnegative(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(index)?).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(e),
        )
    })
}

fn add_recent(
    out: &mut QueryFacts,
    from: i64,
    to: i64,
    process_app: Option<&str>,
    recent: &[UsageBucket],
) -> Result<()> {
    for b in recent {
        for u in crate::usage::grouped_uncertainty(&b.uncertain_network)? {
            if u.possible_from_ms < to && u.possible_to_ms > from {
                out.uncertainty.push(u);
            }
        }
        if b.from_ms < from || b.to_ms > to {
            if b.from_ms < to && b.to_ms > from {
                out.boundary = true;
            }
            continue;
        }
        out.observe(b.from_ms, b.to_ms, b.source);
        if let Some(app) = process_app {
            for p in &b.processes {
                if &*p.app_name == app {
                    out.processes
                        .entry((p.pid, p.start_time_us, p.name.to_string()))
                        .or_default()
                        .merge(&p.facts);
                }
            }
        } else {
            for a in &b.apps {
                out.apps
                    .entry(a.name.to_string())
                    .or_default()
                    .merge(&a.facts);
            }
        }
    }
    Ok(())
}

fn recording_bounds(conn: &Connection, host: i64) -> Result<(Option<i64>, i64)> {
    let recorded_from = conn
        .query_row(
            "SELECT from_ms FROM usage_recording_start WHERE host_id=?1",
            [host],
            |r| r.get(0),
        )
        .optional()?;
    let retained_from = conn
        .query_row(
            "SELECT retained_from_ms FROM usage_recording_start WHERE host_id=?1",
            [host],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);
    Ok((recorded_from, retained_from))
}

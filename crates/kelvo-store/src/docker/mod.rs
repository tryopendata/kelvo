//! Docker persistence has an independent clock and entity-local metric layouts.
//! Empty entity IDs denote the engine aggregate; all other IDs are daemon container IDs.
use crate::{Result, StoreError};
use kelvo_schema::{HostId, Tier};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(crate) mod read;
pub use read::DockerReadSnapshot;

/// A resumed observation closes an old engine switch, including rows left open
/// by an earlier app instance. Population segments retain exact interval starts
/// through rollups, so this does not invent a boundary from a bucket timestamp.
pub(crate) fn resumed_after(
    conn: &rusqlite::Connection,
    engine: i64,
    start: i64,
) -> Result<Option<i64>> {
    use rusqlite::OptionalExtension;
    let mut earliest = None;
    for (table, width) in [
        ("docker_10s", 10_000),
        ("docker_1m", 60_000),
        ("docker_15m", 900_000),
    ] {
        let sql = format!(
            "SELECT p.start_ms FROM {table} b JOIN {table}_populations p ON p.entity_id=b.entity_id AND p.bucket_ms=b.bucket_ms AND p.layout_id=b.layout_id WHERE b.engine_id=?1 AND b.bucket_ms>=?2 AND p.start_ms>=?3 ORDER BY b.bucket_ms,p.start_ms LIMIT 1"
        );
        let observed: Option<i64> = conn
            .query_row(
                &sql,
                rusqlite::params![engine, start.div_euclid(width) * width, start],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(observed) = observed {
            earliest = Some(earliest.map_or(observed, |old: i64| old.min(observed)));
        }
    }
    Ok(earliest)
}
/// Explicit lifecycle registration: reset/discard/prune use relational handlers in
/// writer/docker.rs, not generic host timestamp deletion.
#[cfg(test)]
pub(crate) const LIFECYCLE_TABLES: &[&str] = &[
    "docker_engines",
    "docker_entities",
    "docker_metadata",
    "docker_populations",
    "docker_population_members",
    "docker_10s",
    "docker_1m",
    "docker_15m",
    "docker_10s_populations",
    "docker_1m_populations",
    "docker_15m_populations",
    "docker_events",
    "docker_coverage",
];
pub(crate) const SCHEMA: &str = include_str!("schema.sql");
pub const MAX_BATCH_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PENDING_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PENDING_WRITES: usize = 128;
pub const MAX_QUERY_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_POINTS: usize = 10_000;
pub const MAX_RANGE_MS: i64 = 30 * 86_400_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WeightedMetric {
    pub key: String,
    pub min: f64,
    pub max: f64,
    /// Integral of value over measured milliseconds; never average an average.
    pub sum: f64,
    pub weight_ms: u64,
    pub expected_ms: u64,
}
impl WeightedMetric {
    pub fn average(&self) -> Option<f64> {
        (self.weight_ms > 0).then(|| self.sum / self.weight_ms as f64)
    }
    pub fn merge(&mut self, other: &Self) -> Result<()> {
        if self.key != other.key {
            return Err(invalid("metric identity changed during fold"));
        }
        if other.weight_ms > 0 {
            if self.weight_ms == 0 {
                self.min = other.min;
                self.max = other.max;
            } else {
                self.min = self.min.min(other.min);
                self.max = self.max.max(other.max);
            }
        }
        self.sum += other.sum;
        self.weight_ms = self
            .weight_ms
            .checked_add(other.weight_ms)
            .ok_or_else(|| invalid("duration overflow"))?;
        self.expected_ms = self
            .expected_ms
            .checked_add(other.expected_ms)
            .ok_or_else(|| invalid("duration overflow"))?;
        if !self.sum.is_finite() {
            return Err(invalid("metric sum overflow"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PopulationSegment {
    pub revision: i64,
    pub start_ms: i64,
    pub end_ms: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DockerBucket {
    pub entity: String,
    pub tier: Tier,
    pub bucket_ms: i64,
    pub revision: i64,
    pub metrics: Vec<WeightedMetric>,
    pub populations: Vec<PopulationSegment>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct DockerEntity {
    pub external_id: String,
    pub effective_ms: i64,
    pub revision: i64,
    pub name: String,
    pub image: String,
    pub project: Option<String>,
    pub service: Option<String>,
    pub lifecycle: String,
    pub explicit_limit: Option<i64>,
    pub effective_limit: Option<i64>,
    pub capacity: Option<i64>,
    pub last_seen_ms: i64,
    pub tombstone: bool,
    /// Versioned, bounded allowlisted metadata only. Never environment or command data.
    pub extras: Vec<u8>,
}
/// Persisted metadata only; daemon endpoints and live reachability are not inferred.
#[derive(Clone, Debug)]
pub struct DockerCatalogEngine {
    pub id: String,
    pub runtime: String,
}
#[derive(Clone, Debug)]
pub struct DockerCatalogEntity {
    pub engine_id: String,
    pub metadata: DockerEntity,
}
#[derive(Clone, Debug, Default)]
pub struct DockerCatalog {
    pub engines: Vec<DockerCatalogEngine>,
    pub entities: Vec<DockerCatalogEntity>,
    pub total: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DockerPopulation {
    pub revision: i64,
    pub effective_ms: i64,
    pub members: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerEvent {
    pub entity: Option<String>,
    pub time_ns: i64,
    pub action: String,
    /// Bounded, sorted allowlist: name, image, exitCode, signal, health_status.
    pub attributes: BTreeMap<String, String>,
}
impl DockerEvent {
    pub fn key(&self, daemon: &str) -> Result<String> {
        let bytes = pack(&(
            daemon,
            &self.entity,
            self.time_ns,
            &self.action,
            &self.attributes,
        ))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerCursor {
    pub time_ns: i64,
    pub event_key: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerCoverage {
    pub entity: Option<String>,
    pub metric: Option<String>,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub reason: String,
    pub revision: i64,
    pub sampled_ms: i64,
    pub expected_ms: i64,
}
#[derive(Clone, Debug)]
pub struct DockerBatch {
    pub host: HostId,
    pub daemon_id: String,
    pub generation: i64,
    pub revision: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub runtime: String,
    pub entities: Vec<DockerEntity>,
    pub buckets: Vec<DockerBucket>,
    pub events: Vec<DockerEvent>,
    pub coverage: Vec<DockerCoverage>,
    pub cursor: Option<DockerCursor>,
    pub populations: Vec<DockerPopulation>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockerOutcome {
    Persisted,
    Rejected,
    Failed,
    Reset,
}
#[derive(Clone, Debug)]
pub struct DockerAck {
    pub generation: i64,
    pub revision: i64,
    pub outcome: DockerOutcome,
    pub reason: Option<String>,
}
#[derive(Clone, Debug)]
pub struct DockerHistoryQuery {
    pub entity: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub tier: Tier,
    pub max_points: usize,
}
#[derive(Clone, Debug, Default)]
pub struct DockerHistory {
    pub buckets: Vec<DockerBucket>,
    pub metadata: Vec<DockerEntity>,
    pub coverage: Vec<DockerCoverage>,
}
#[derive(Clone, Debug)]
pub struct DockerEventRow {
    pub id: i64,
    pub event_key: String,
    pub event: DockerEvent,
}
#[derive(Clone, Debug)]
pub struct DockerEventQuery {
    pub entity: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub after: Option<(i64, String)>,
    pub limit: usize,
}
#[derive(Clone, Debug, Default)]
pub struct DockerStorageStatus {
    pub logical_bytes: u64,
    pub pending_bytes: usize,
    pub pending_writes: usize,
    pub allowance_bytes: u64,
    pub paused: bool,
}
pub fn allowance(total_history_bytes: u64) -> u64 {
    (256 * 1024 * 1024).min(total_history_bytes / 4)
}
pub(crate) fn invalid(reason: &str) -> StoreError {
    StoreError::Docker(reason.into())
}
pub(crate) fn table(tier: Tier) -> Result<&'static str> {
    match tier {
        Tier::S10 => Ok("docker_10s"),
        Tier::M1 => Ok("docker_1m"),
        Tier::M15 => Ok("docker_15m"),
        _ => Err(StoreError::NotPersisted(tier)),
    }
}
pub(crate) fn pack(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes).map_err(|e| StoreError::Cbor(e.to_string()))?;
    Ok(bytes)
}
pub(crate) fn unpack<T: for<'a> Deserialize<'a>>(bytes: &[u8]) -> Result<T> {
    ciborium::from_reader(bytes).map_err(|e| StoreError::Cbor(e.to_string()))
}

impl DockerBatch {
    /// Includes 100% overhead for indexes/row headers plus entity/layout reference ownership.
    pub fn logical_bytes(&self) -> Result<usize> {
        self.validate()?;
        let payload = pack_limited(
            &(
                &self.entities,
                &self.buckets,
                &self.events,
                &self.coverage,
                &self.cursor,
                &self.populations,
            ),
            MAX_BATCH_BYTES / 2,
        )?;
        let bytes = payload.len().saturating_mul(2).saturating_add(512);
        if bytes > MAX_BATCH_BYTES {
            return Err(invalid("batch exceeds 2 MiB"));
        }
        Ok(bytes)
    }
    fn validate(&self) -> Result<()> {
        if self.daemon_id.is_empty()
            || self.daemon_id.len() > 256
            || self.runtime.len() > 128
            || self.generation < 0
            || self.revision < 0
            || self.end_ms < self.start_ms
        {
            return Err(invalid("invalid batch identity, revision or interval"));
        }
        if self.entities.len() > 10_001
            || self.buckets.len() > 1024
            || self.events.len() > 1000
            || self.coverage.len() > 1024
            || self.populations.len() > 128
        {
            return Err(invalid("batch count bound exceeded"));
        }
        for e in &self.entities {
            if e.external_id.len() > 256
                || e.name.len() > 1024
                || e.image.len() > 1024
                || e.project.as_ref().is_some_and(|s| s.len() > 256)
                || e.service.as_ref().is_some_and(|s| s.len() > 256)
                || e.lifecycle.len() > 64
                || e.extras.len() > 16 * 1024
            {
                return Err(invalid("metadata bound exceeded"));
            }
        }
        for b in &self.buckets {
            table(b.tier)?;
            let width = b.tier.bucket_ms().ok_or(StoreError::NotPersisted(b.tier))?;
            if b.bucket_ms.rem_euclid(width) != 0 {
                return Err(StoreError::Misaligned {
                    ts_ms: b.bucket_ms,
                    width_ms: width,
                });
            }
            if b.entity.len() > 256
                || b.metrics.len() > 32
                || b.populations.len() > 128
                || b.revision > self.revision
                || b.revision < 0
            {
                return Err(invalid("bucket bound exceeded"));
            }
            let mut keys = std::collections::HashSet::new();
            for m in &b.metrics {
                if metric_catalog(&m.key).is_none()
                    || !keys.insert(&m.key)
                    || !m.min.is_finite()
                    || !m.max.is_finite()
                    || !m.sum.is_finite()
                    || m.min > m.max
                    || m.weight_ms > m.expected_ms
                    || m.expected_ms > width as u64
                {
                    return Err(invalid("invalid metric or duration"));
                }
            }
            for p in &b.populations {
                if !b.entity.is_empty()
                    || p.end_ms < p.start_ms
                    || p.start_ms < b.bucket_ms
                    || p.end_ms > b.bucket_ms.saturating_add(width)
                {
                    return Err(invalid("invalid population segment"));
                }
            }
        }
        for e in &self.events {
            if e.entity.as_ref().is_some_and(|v| v.len() > 256)
                || e.action.len() > 128
                || e.attributes.len() > 5
                || e.time_ns < 0
                || e.attributes.iter().any(|(k, v)| {
                    !matches!(
                        k.as_str(),
                        "name" | "image" | "exitCode" | "signal" | "health_status"
                    ) || v.len() > 1024
                })
            {
                return Err(invalid("event allowlist or bound exceeded"));
            }
        }
        for c in &self.coverage {
            if c.end_ms.is_some_and(|end| end < c.start_ms)
                || c.sampled_ms < 0
                || c.expected_ms < c.sampled_ms
                || c.reason.len() > 128
                || c.entity.as_ref().is_some_and(|s| s.len() > 256)
                || c.metric.as_ref().is_some_and(|s| s.len() > 128)
            {
                return Err(invalid("invalid coverage"));
            }
        }
        for p in &self.populations {
            if p.members.len() > 128 || p.members.iter().any(|id| id.is_empty() || id.len() > 256) {
                return Err(invalid("invalid monitored population"));
            }
        }
        Ok(())
    }
}
/// Stable catalog names; units are not inferred from a display label.
pub fn metric_catalog(key: &str) -> Option<(&'static str, &'static str)> {
    match key {
        "cpu_percent" => Some(("percent", "gauge")),
        "cpu_cores" => Some(("cores", "gauge")),
        "memory_working_set" | "memory_raw" | "memory_limit" | "engine_capacity" => {
            Some(("bytes", "gauge"))
        }
        "network_rx" | "network_tx" | "block_read" | "block_write" => {
            Some(("bytes_per_second", "gauge"))
        }
        _ => None,
    }
}

/// CBOR writes stop at the limit rather than allocating an oversized batch first.
fn pack_limited(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    struct Bounded {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl std::io::Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.bytes.len().saturating_add(bytes.len()) > self.limit {
                return Err(std::io::Error::other("Docker batch byte bound"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut out = Bounded {
        bytes: Vec::new(),
        limit,
    };
    ciborium::into_writer(value, &mut out).map_err(|_| invalid("batch exceeds 2 MiB"))?;
    Ok(out.bytes)
}

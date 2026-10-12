//! Independent Docker observations. Runtime memory and container memory are never additive.
use crate::{HostId, JsSafeInt};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct DockerSettings {
    pub enabled: bool,
    /// Canonical Unix socket, persisted independently of the CLI context.
    pub selected_engine: Option<String>,
    pub monitored_containers: Vec<String>,
    pub explicit_selection: bool,
    pub show_popover: bool,
    pub show_tray_count: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerEngine {
    pub id: String,
    pub daemon_id: Option<String>,
    pub runtime: String,
    pub context: Option<String>,
    pub socket: String,
    pub reachable: bool,
    pub preferred: bool,
    pub error: Option<String>,
    pub api_version: Option<String>,
    pub capacity_bytes: Option<f64>,
    #[specta(type = Option<JsSafeInt>)]
    pub capacity_observed_ms: Option<i64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DockerStatus {
    Disabled,
    Connecting,
    Connected,
    IdleSuspended,
    Paused,
    Unavailable,
    PermissionDenied,
    Incompatible,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DockerLifecycle {
    Created,
    Running,
    Paused,
    Restarting,
    Exited,
    Removing,
    Dead,
    Removed,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DockerHealth {
    NoCheck,
    Starting,
    Healthy,
    Unhealthy,
    Unknown,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerMetrics {
    pub cpu_pct: Option<f64>,
    pub working_set_bytes: Option<f64>,
    pub raw_memory_bytes: Option<f64>,
    pub effective_limit_bytes: Option<f64>,
    pub network_rx_bps: Option<f64>,
    pub network_tx_bps: Option<f64>,
    pub block_read_bps: Option<f64>,
    pub block_write_bps: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerContainer {
    pub engine_id: String,
    pub id: String,
    pub name: String,
    pub image: String,
    pub project: Option<String>,
    pub service: Option<String>,
    pub lifecycle: DockerLifecycle,
    pub health: DockerHealth,
    pub restart_count: Option<u32>,
    pub metadata_available: bool,
    /// Receipt time of the last successful inspect (or confirmed inspect 404),
    /// independent of inventory and metric observation timestamps.
    #[serde(default)]
    #[specta(type = Option<JsSafeInt>)]
    pub metadata_observed_ms: Option<i64>,
    pub observed_restarts: u32,
    pub repeated_restarts: bool,
    pub observed_restart_window_ms: u32,
    pub oom_killed: bool,
    pub explicit_limit_bytes: Option<f64>,
    pub cpu_quota: Option<f64>,
    pub restart_policy: Option<String>,
    pub ports: Vec<String>,
    pub mounts: Vec<String>,
    pub created: Option<String>,
    #[specta(type = JsSafeInt)]
    pub last_seen_ms: i64,
    #[specta(type = Option<JsSafeInt>)]
    pub observed_ms: Option<i64>,
    pub interval_ms: Option<u32>,
    pub monitored: bool,
    pub metrics: DockerMetrics,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerAggregate {
    pub monitoring_limit: u32,
    pub cpu_cores: Option<f64>,
    pub working_set_bytes: Option<f64>,
    pub capacity_bytes: Option<f64>,
    pub cpu_measured: u32,
    pub memory_measured: u32,
    pub expected: u32,
    pub running: u32,
    pub total: u32,
    pub monitored: u32,
    pub partial: bool,
    pub inventory_complete: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerRuntimeFootprint {
    pub scope: String,
    pub explanation: String,
    pub cpu_pct: Option<f64>,
    pub footprint_bytes: Option<f64>,
    #[specta(type = Option<JsSafeInt>)]
    pub observed_ms: Option<i64>,
    pub processes: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerCoverage {
    #[specta(type = JsSafeInt)]
    pub start_ms: i64,
    #[specta(type = Option<JsSafeInt>)]
    pub end_ms: Option<i64>,
    pub container_id: Option<String>,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerPersistence {
    pub status: String,
    pub logical_bytes: f64,
    pub allowance_bytes: f64,
    #[specta(type = Option<JsSafeInt>)]
    pub retained_from_ms: Option<i64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerSnapshot {
    pub host_id: HostId,
    pub generation: u32,
    pub status: DockerStatus,
    pub message: Option<String>,
    pub engine: Option<DockerEngine>,
    pub settings: DockerSettings,
    pub containers: Vec<DockerContainer>,
    pub aggregate: DockerAggregate,
    pub runtime: DockerRuntimeFootprint,
    pub coverage: Vec<DockerCoverage>,
    #[specta(type = JsSafeInt)]
    pub observed_ms: i64,
    /// Publication clock; observation time advances only when data arrives.
    #[specta(type = JsSafeInt)]
    pub now_ms: i64,
    pub interval_ms: u32,
    pub population_revision: u32,
    pub persistence: DockerPersistence,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerEvent {
    pub id: String,
    pub engine_id: String,
    pub container_id: Option<String>,
    /// Full nanoseconds are strings at the JS boundary.
    pub time_ns: String,
    #[specta(type = JsSafeInt)]
    pub time_ms: i64,
    pub action: String,
    pub name: Option<String>,
    pub exit_code: Option<String>,
    pub attributes: std::collections::BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerHistoryRequest {
    pub engine_id: String,
    pub container_id: Option<String>,
    #[specta(type = JsSafeInt)]
    pub from_ms: i64,
    #[specta(type = JsSafeInt)]
    pub to_ms: i64,
    pub max_points: u32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerHistoryPoint {
    #[specta(type = JsSafeInt)]
    pub time_ms: i64,
    pub duration_ms: u32,
    pub metrics: DockerMetrics,
    pub measured_ms: u32,
    pub expected_ms: u32,
    /// Complete measured coverage of this bucket.
    pub covered: bool,
    #[specta(type = JsSafeInt)]
    pub population_revision: i64,
    #[specta(type = JsSafeInt)]
    pub revision: i64,
    pub explicit_limit_bytes: Option<f64>,
    pub engine_capacity_bytes: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerHistoryPage {
    pub generation: u32,
    pub points: Vec<DockerHistoryPoint>,
    pub coverage: Vec<DockerCoverage>,
    pub metadata: Vec<DockerContainer>,
    pub persistence: DockerPersistence,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerEventsRequest {
    pub engine_id: String,
    pub container_id: Option<String>,
    #[specta(type = JsSafeInt)]
    pub from_ms: i64,
    #[specta(type = JsSafeInt)]
    pub to_ms: i64,
    pub limit: u32,
    pub before_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerEventsPage {
    pub events: Vec<DockerEvent>,
    pub next_id: Option<String>,
    pub coverage: Vec<DockerCoverage>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerLogsRequest {
    pub engine_id: String,
    pub container_id: String,
    pub follow: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerLogBatch {
    pub generation: u32,
    pub lines: Vec<String>,
    pub truncated: bool,
    pub complete: bool,
    pub error: Option<String>,
}

impl<'de> Deserialize<'de> for DockerLifecycle {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "created" => Self::Created,
            "running" => Self::Running,
            "paused" => Self::Paused,
            "restarting" => Self::Restarting,
            "exited" => Self::Exited,
            "removing" => Self::Removing,
            "dead" => Self::Dead,
            "removed" => Self::Removed,
            _ => Self::Unknown,
        })
    }
}
impl<'de> Deserialize<'de> for DockerHealth {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "no_check" => Self::NoCheck,
            "starting" => Self::Starting,
            "healthy" => Self::Healthy,
            "unhealthy" => Self::Unhealthy,
            _ => Self::Unknown,
        })
    }
}

/// Retained metadata browsing never probes or enables a Docker engine.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct DockerRetainedRequest {
    pub engine_id: Option<String>,
    pub search: String,
    pub offset: u32,
    pub limit: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct DockerRetainedEngine {
    pub id: String,
    pub runtime: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct DockerRetainedPage {
    pub engines: Vec<DockerRetainedEngine>,
    pub containers: Vec<DockerContainer>,
    pub total: u32,
    pub next_offset: Option<u32>,
}

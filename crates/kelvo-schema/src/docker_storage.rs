//! Local, on-demand Docker storage views. These are not metric series or remote IPC.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DockerStorageKind {
    Container,
    Image,
    Volume,
    BuildCache,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DockerStorageAction {
    Delete,
    Stop,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DockerStorageSelection {
    pub kind: DockerStorageKind,
    pub id: String,
    pub action: DockerStorageAction,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DockerStorageMount {
    pub kind: String,
    pub name: Option<String>,
    pub source: Option<String>,
    pub destination: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerStorageResource {
    pub kind: DockerStorageKind,
    pub id: String,
    pub name: String,
    pub project: Option<String>,
    pub size_bytes: Option<f64>,
    /// Conservative independently reclaimable bytes; never sum shared image/cache sizes.
    pub reclaimable_bytes: Option<f64>,
    pub references: Vec<String>,
    #[specta(type = Option<crate::JsSafeInt>)]
    pub age_ms: Option<i64>,
    pub age_basis: Option<String>,
    /// Repository-based inference, not a database inspection.
    pub database: Option<String>,
    pub mounts: Vec<DockerStorageMount>,
    pub auto_remove: Option<bool>,
    pub can_delete: bool,
    pub can_stop: bool,
    pub reason: Option<String>,
    /// Hash of identity/state metadata used only for local confirmation revalidation.
    pub fingerprint: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerStorageCategory {
    pub kind: DockerStorageKind,
    pub size_bytes: Option<f64>,
    pub reclaimable_bytes: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerStorageSnapshot {
    pub engine_id: String,
    pub generation: u32,
    #[specta(type = crate::JsSafeInt)]
    pub observed_ms: i64,
    pub complete: bool,
    pub resources: Vec<DockerStorageResource>,
    pub categories: Vec<DockerStorageCategory>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DockerStorageItemStatus {
    Succeeded,
    Skipped,
    Failed,
    OutcomeUnknown,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DockerStorageItemResult {
    pub selection: DockerStorageSelection,
    pub status: DockerStorageItemStatus,
    pub message: String,
    pub reclaimed_bytes: Option<f64>,
}

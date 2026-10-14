//! Remote command contracts; wire names and field shapes stay stable.
use super::*;
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct RemoteHostConfig {
    pub id: String,
    pub name: String,
    pub lan: Option<String>,
    pub tailscale: Option<String>,
    pub enabled: bool,
    pub mirror_budget_mb: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RemoteState {
    Connecting,
    Connected,
    Offline,
    Disabled,
    Error,
}
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RemoteRoute {
    Lan,
    Tailscale,
}
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RemoteSync {
    Idle,
    CatchingUp,
    Current,
    Unavailable,
}
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct RemoteHostStatus {
    pub state: RemoteState,
    pub route: Option<RemoteRoute>,
    pub last_seen_ms: Option<f64>,
    pub error: Option<String>,
    pub sync: RemoteSync,
    pub clock_offset_ms: f64,
    pub cpu_pct: Option<f64>,
    pub memory_pct: Option<f64>,
}
impl Default for RemoteHostStatus {
    fn default() -> Self {
        Self {
            state: RemoteState::Offline,
            route: None,
            last_seen_ms: None,
            error: None,
            sync: RemoteSync::Idle,
            clock_offset_ms: 0.0,
            cpu_pct: None,
            memory_pct: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct RemoteHostRecord {
    pub config: RemoteHostConfig,
    pub status: RemoteHostStatus,
}
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct RemoteHostPreparation {
    pub installed: bool,
    pub compatible: bool,
    pub host_id: Option<String>,
    pub name: String,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
pub struct RemoteSshHostTrust {
    pub token: String,
    pub alias: String,
    pub hostname: String,
    pub port: u16,
    pub fingerprints: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RemoteSshConfigStatus {
    Missing,
    Present,
}

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
    /// The version the host last reported (`Hello.app_version`).
    pub agent_version: Option<String>,
    /// A Linux host whose agent is older than the one this build bundles; the host
    /// list offers "Update agent".
    pub update_available: bool,
    /// The id the host answered with when it is not the paired one. The host list
    /// offers "Re-pair as a new host" (D-115).
    pub identity_changed: Option<String>,
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
            agent_version: None,
            update_available: false,
            identity_changed: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct RemoteHostRecord {
    pub config: RemoteHostConfig,
    pub os: kelvo_schema::OsKind,
    pub status: RemoteHostStatus,
}

/// What answered at an SSH alias, which decides the setup path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RemoteHostKind {
    /// Another Mac, running Kelvo.app (D-104).
    Mac,
    /// A Linux host, running the headless agent (D-115).
    Linux,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct RemoteHostPreparation {
    pub kind: RemoteHostKind,
    pub installed: bool,
    /// Connecting needs no install or update.
    pub compatible: bool,
    /// The installed agent is newer than this app: it connects as it is and is never
    /// replaced with this build's older one. Linux only; always false for a Mac.
    pub newer_installed: bool,
    pub host_id: Option<String>,
    pub name: String,
    pub message: Option<String>,
    /// What installing on a Linux host involves; `None` for a Mac.
    pub linux: Option<LinuxPreparation>,
}

/// How the agent runs on a Linux host: root installs a system service, anyone else a
/// user service (D-115).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentLayout {
    System,
    User,
}

/// Whether the agent's login can use the Docker socket to name containers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RemoteDockerAccess {
    /// No Docker socket on the host.
    Absent,
    /// There is a socket this login cannot use; containers show by short id.
    Unreadable,
    Readable,
}

/// The Linux disclosure: what setup will put on the host and what it found there.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct LinuxPreparation {
    pub layout: RemoteAgentLayout,
    /// The SSH login the agent installs for.
    pub user: String,
    /// os-release `PRETTY_NAME`.
    pub distro: Option<String>,
    /// `uname -r`.
    pub kernel: Option<String>,
    /// `uname -m`.
    pub arch: String,
    pub systemd_version: Option<u32>,
    /// A user service keeps running after logout only with lingering on. Always true
    /// for the system layout.
    pub linger_ok: bool,
    /// `getenforce`: `Enforcing`, `Permissive` or `Disabled`; `None` without SELinux.
    pub selinux: Option<String>,
    pub binary_path: String,
    pub data_dir: String,
    /// Free space on the data directory's volume.
    #[specta(type = Option<kelvo_schema::JsSafeInt>)]
    pub free_bytes: Option<u64>,
    /// The low-disk reserve plus the history budget; `None` when an agent is already
    /// installed (its history already holds its share) or the volume size is unknown.
    #[specta(type = Option<kelvo_schema::JsSafeInt>)]
    pub needed_bytes: Option<u64>,
    /// The agent's history size limit (decimal MB).
    pub budget_mb: u32,
    /// The installed agent's version, when one is installed.
    pub existing_version: Option<String>,
    /// Whether the agent can use the Docker socket to name containers.
    pub docker: RemoteDockerAccess,
    /// Non-blocking notes from the probe (the linger command for a user service).
    pub warnings: Vec<String>,
    /// The saved host already using the installed agent's id. Adding needs a new
    /// identity first (`reset_remote_host_identity`).
    pub duplicate_of: Option<String>,
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

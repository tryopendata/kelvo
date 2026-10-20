//! Remote monitoring of other Macs (D-104) and Linux agents (D-116): shell adapters,
//! saved hosts and host-specific history. Transport, protocol state machines and agent
//! installation live in kelvo-remote.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use kelvo_remote::{
    ControlRequest, ControlResponse, EndpointConfig, HostServer, Mirror, RemoteCommand,
    RemoteSource, SocketPaths,
};
use kelvo_schema::lock::LockExt;
use kelvo_schema::{HostId, HostRecord};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::error::CommandError;
use crate::history::History;
use crate::live::LiveFeed;
use crate::state::AppState;

/// A missing per-user config is guidance, not a connection failure: OpenSSH can
/// also use defaults, the system config, and an agent. Never read private keys.
fn ssh_config_status(path: &std::path::Path) -> Result<RemoteSshConfigStatus, CommandError> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(RemoteSshConfigStatus::Present),
        Ok(_) => Err(failure("~/.ssh/config must be a file, not a directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(RemoteSshConfigStatus::Missing)
        }
        Err(error) => Err(failure(format!("Could not check ~/.ssh/config: {error}"))),
    }
}

#[tauri::command]
#[specta::specta]
pub fn remote_ssh_config_status(app: AppHandle) -> Result<RemoteSshConfigStatus, CommandError> {
    ssh_config_status(&app.path().home_dir().map_err(failure)?.join(".ssh/config"))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedHost {
    config: RemoteHostConfig,
    record: HostRecord,
    /// What the SSH session runs. Hosts saved before Linux support stored the Mac
    /// helper's path as `helper_path`, which reads as [`RemoteCommand::MacHelper`].
    #[serde(default, alias = "helper_path")]
    command: RemoteCommand,
}
impl SavedHost {
    fn endpoint(&self) -> EndpointConfig {
        EndpointConfig {
            lan: self.config.lan.clone(),
            tailscale: self.config.tailscale.clone(),
            command: self.command.clone(),
        }
    }
    fn is_linux(&self) -> bool {
        matches!(self.command, RemoteCommand::Agent { .. })
    }
}

/// Reads `remote-hosts.json` one host at a time, so a host this build can't read is
/// skipped instead of dropping every saved host. Returns what could not be read.
fn parse_saved(bytes: &[u8]) -> (BTreeMap<HostId, SavedHost>, Vec<String>) {
    let entries: BTreeMap<String, serde_json::Value> = match serde_json::from_slice(bytes) {
        Ok(entries) => entries,
        Err(e) => return (BTreeMap::new(), vec![e.to_string()]),
    };
    let mut saved = BTreeMap::new();
    let mut errors = Vec::new();
    for (key, value) in entries {
        match (
            Uuid::parse_str(&key),
            serde_json::from_value::<SavedHost>(value),
        ) {
            (Ok(id), Ok(host)) => {
                saved.insert(HostId(id), host);
            }
            (Err(e), _) => errors.push(format!("{key}: {e}")),
            (_, Err(e)) => errors.push(format!("{key}: {e}")),
        }
    }
    (saved, errors)
}
/// The shell adapter and transport sink share one store without per-query allocation.
struct RegisteredMirror {
    history: Arc<History>,
    sink: Arc<Mirror>,
}

/// Local history and every mirror have independent lifecycle and write failures.
#[derive(Default)]
pub struct RemoteManager {
    saved: Mutex<BTreeMap<HostId, SavedHost>>,
    setups: Mutex<BTreeMap<String, tokio::sync::watch::Sender<bool>>>,
    trust: Mutex<BTreeMap<String, (std::time::Instant, kelvo_remote::trust::TrustCandidate)>>,
    statuses: Mutex<BTreeMap<HostId, RemoteHostStatus>>,
    mirrors: Mutex<BTreeMap<HostId, RegisteredMirror>>,
    sources: Mutex<BTreeMap<HostId, Arc<RemoteSource>>>,
    monitor: Mutex<Option<crate::remote_network::Monitor>>,
    generations: Mutex<BTreeMap<HostId, Uuid>>,
    watchers: Mutex<BTreeMap<HostId, tauri::async_runtime::JoinHandle<()>>>,
    pub(crate) operation: tokio::sync::Mutex<()>,
    stop: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
    metrics_stop: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
}
impl RemoteManager {
    pub fn history(&self, host: HostId) -> Result<Arc<History>, CommandError> {
        self.mirrors
            .lock_ok()
            .get(&host)
            .map(|m| m.history.clone())
            .ok_or(CommandError::UnknownHost { host })
    }
    pub fn now_ms(&self, host: HostId) -> i64 {
        self.sources
            .lock_ok()
            .get(&host)
            .and_then(|s| s.clock().now_ms())
            .unwrap_or_else(kelvo_engine::wall_ms)
    }
    pub fn controller_battery_changed(&self, on_battery: bool) {
        for source in self.sources.lock_ok().values() {
            source.set_controller_on_battery(on_battery);
        }
    }
    pub fn recent(&self, host: HostId, from: i64, to: i64) -> Vec<kelvo_store::BucketRow> {
        self.mirrors
            .lock_ok()
            .get(&host)
            .map_or_else(Vec::new, |m| m.sink.recent_rows(from, to))
    }
}
impl From<kelvo_remote::RemoteError> for CommandError {
    fn from(error: kelvo_remote::RemoteError) -> Self {
        use kelvo_remote::RemoteError;
        let message = error.to_string();
        match error {
            RemoteError::Authentication(_) => Self::RemoteAuth { message },
            RemoteError::HostKey(_) => Self::HostKeyUnknown { message },
            RemoteError::Incompatible(_) => Self::RemoteIncompatible { message },
            RemoteError::Identity(_) | RemoteError::IdentityMismatch { .. } => {
                Self::RemoteIdentity { message }
            }
            RemoteError::AgentNotRunning(_) => Self::RemoteAgentNotRunning { message },
            RemoteError::Transport(_)
            | RemoteError::Io(_)
            | RemoteError::Codec(_)
            | RemoteError::Protocol(_) => Self::RemoteUnreachable { message },
        }
    }
}

impl From<kelvo_remote::agent_install::AgentInstallError> for CommandError {
    fn from(error: kelvo_remote::agent_install::AgentInstallError) -> Self {
        use kelvo_remote::agent_install::AgentInstallError as E;
        let message = error.to_string();
        match error {
            E::Remote(e) => e.into(),
            E::UnsupportedArch(_)
            | E::NoSystemd { .. }
            | E::NoUserManager { .. }
            | E::NoDigestTool => Self::RemoteIncompatible { message },
            E::LowDisk { .. } => Self::RemoteLowDisk { message },
            E::OtherLayout { .. } | E::AlreadyInstalled { .. } | E::NotInstalled { .. } => {
                Self::RemoteAgentConflict { message }
            }
            E::NotRunning { .. } => Self::RemoteAgentNotRunning { message },
            // Nothing was replaced; the transfer can be retried.
            E::DigestMismatch { .. } => Self::RemoteUnreachable { message },
            E::UpdateFailed { restored, .. } => Self::RemoteAgentUpdateFailed { message, restored },
        }
    }
}
impl From<agent_bundle::AgentBundleError> for CommandError {
    fn from(error: agent_bundle::AgentBundleError) -> Self {
        let message = error.to_string();
        match error {
            agent_bundle::AgentBundleError::UnsupportedArch { .. } => {
                Self::RemoteIncompatible { message }
            }
            _ => Self::Internal { message },
        }
    }
}

fn failure(e: impl std::fmt::Display) -> CommandError {
    CommandError::Internal {
        message: e.to_string(),
    }
}
fn host_id(id: &str) -> Result<HostId, CommandError> {
    Uuid::parse_str(id).map(HostId).map_err(failure)
}
fn data_dir(app: &AppHandle) -> Result<PathBuf, CommandError> {
    app.path().app_data_dir().map_err(failure)
}
fn saved_path(app: &AppHandle) -> Result<PathBuf, CommandError> {
    Ok(data_dir(app)?.join("remote-hosts.json"))
}
fn save(app: &AppHandle) -> Result<(), CommandError> {
    let state = app.state::<AppState>();
    let bytes = serde_json::to_vec_pretty(&*state.remotes.saved.lock_ok()).map_err(failure)?;
    let path = saved_path(app)?;
    let tmp = path.with_extension("json.tmp");
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp).map_err(failure)?;
    file.write_all(&bytes).map_err(failure)?;
    file.sync_all().map_err(failure)?;
    std::fs::rename(tmp, path).map_err(failure)
}
fn validate(c: &RemoteHostConfig) -> Result<(), CommandError> {
    if c.name.trim().is_empty()
        || c.name.len() > 128
        || !(10..=10_000).contains(&c.mirror_budget_mb)
    {
        return Err(CommandError::InvalidArgument {
            message: "Enter a name and a mirror budget between 10 and 10000 MB".into(),
        });
    }
    if c.lan.as_deref().is_none_or(str::is_empty)
        && c.tailscale.as_deref().is_none_or(str::is_empty)
    {
        return Err(CommandError::InvalidArgument {
            message: "Enter a LAN or Tailscale SSH alias".into(),
        });
    }
    for alias in [&c.lan, &c.tailscale].into_iter().flatten() {
        if alias.starts_with('-')
            || alias.chars().any(|c| c.is_whitespace() || c.is_control())
            || alias.len() > 255
        {
            return Err(CommandError::InvalidArgument {
                message: "Use an SSH alias or user@host, without arguments".into(),
            });
        }
    }
    Ok(())
}

mod agent_bundle;
mod commands;
mod contracts;
mod registration;
mod server;
mod setup;
pub use commands::*;
pub use contracts::*;
use registration::{discard_host_async, register};
pub use server::{refresh_metrics, shutdown, start};
use setup::probe_routes;
pub use setup::*;
#[cfg(test)]
use setup::{most_actionable, setup_step};
#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;

#[cfg(test)]
mod command_regressions {
    use super::*;

    #[test]
    fn ssh_config_check_distinguishes_missing_files_and_invalid_paths() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        assert_eq!(
            ssh_config_status(&config).unwrap(),
            RemoteSshConfigStatus::Missing
        );
        std::fs::write(&config, "Host studio\n HostName studio.local\n").unwrap();
        assert_eq!(
            ssh_config_status(&config).unwrap(),
            RemoteSshConfigStatus::Present
        );
        assert!(ssh_config_status(dir.path()).is_err());
    }

    #[test]
    fn route_failures_report_the_actionable_error_with_every_message() {
        let error = most_actionable(vec![
            CommandError::RemoteUnreachable {
                message: "lan: Connection refused".into(),
            },
            CommandError::HostKeyUnknown {
                message: "tailscale: Host key verification failed.".into(),
            },
        ])
        .unwrap();
        let CommandError::HostKeyUnknown { message } = error else {
            panic!("expected host_key_unknown, got {error:?}");
        };
        assert!(message.contains("Connection refused"));
        assert!(message.contains("Host key verification failed"));
    }

    #[test]
    fn remote_errors_preserve_actionable_kinds() {
        use kelvo_remote::RemoteError;
        for (error, kind) in [
            (RemoteError::Authentication("login".into()), "remote_auth"),
            (RemoteError::HostKey("unknown".into()), "host_key_unknown"),
            (
                RemoteError::Incompatible("arm64".into()),
                "remote_incompatible",
            ),
            (
                RemoteError::Identity("different Mac".into()),
                "remote_identity",
            ),
            (
                RemoteError::Transport("offline".into()),
                "remote_unreachable",
            ),
            (
                RemoteError::Protocol("invalid".into()),
                "remote_unreachable",
            ),
            (
                RemoteError::Io(std::io::Error::other("offline")),
                "remote_unreachable",
            ),
        ] {
            let value = serde_json::to_value(CommandError::from(error)).unwrap();
            assert_eq!(value["kind"], kind);
            assert!(!value["message"].as_str().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn stopping_sharing_preserves_login_and_removes_even_if_configure_fails() {
        for fail in [false, true] {
            let hosts = Mutex::new(std::collections::HashMap::from([("mac", true)]));
            let remote_settings = &Mutex::new((true, true));
            let result = finish_removal(
                true,
                configure_stop_sharing(|request| async move {
                    let ControlRequest::Configure {
                        allow_remote_monitoring,
                        launch_at_login,
                        show_menu_bar_icon,
                    } = request
                    else {
                        panic!("expected configure")
                    };
                    assert!(launch_at_login.is_none());
                    assert!(show_menu_bar_icon.is_none());
                    if fail {
                        return Err(CommandError::RemoteUnreachable {
                            message: "offline".into(),
                        });
                    }
                    remote_settings.lock_ok().0 = allow_remote_monitoring;
                    let (sharing, login) = *remote_settings.lock_ok();
                    Ok(ControlResponse {
                        ok: true,
                        message: "Sharing stopped".into(),
                        host: None,
                        allow_remote_monitoring: Some(sharing),
                        show_menu_bar_icon: None,
                        launch_at_login: Some(login),
                    })
                }),
                async {
                    hosts.lock_ok().remove("mac");
                    Ok(())
                },
            )
            .await
            .unwrap();
            assert!(hosts.lock_ok().is_empty());
            assert_eq!(remote_settings.lock_ok().0, fail);
            assert!(remote_settings.lock_ok().1, "launch at login stays on");
            assert_eq!(result.is_some(), fail);
        }
    }
}

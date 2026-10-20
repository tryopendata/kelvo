//! The Linux path of Add host (D-115): prepare, install, connect, update and recover.
//!
//! Recovery never touches an agent that was there before setup started: a failed
//! update is rolled back by [`agent_install::update`] itself, and only an agent this
//! setup freshly installed is removed again when a later step fails.
use super::*;
use kelvo_remote::RemoteCommand;
use kelvo_remote::agent_install::{self, LinuxProbe};

/// The agent's history size limit: the Mac's default (`HistorySettings`), which the
/// agent's `configure` accepts.
pub(in crate::remote) const AGENT_BUDGET_MB: u32 =
    kelvo_schema::settings::HistorySettings::DEFAULT_SIZE_LIMIT_MB;

/// The disclosure for `probe`: what will be installed where, and what was found.
pub(in crate::remote) fn preparation(
    alias: &str,
    probe: &LinuxProbe,
    saved: &BTreeMap<HostId, SavedHost>,
) -> Result<RemoteHostPreparation, CommandError> {
    let target = probe.target()?;
    let existing = probe.existing();
    let existing_version = existing
        .and_then(|e| e.version.as_ref())
        .map(|v| v.version.clone());
    let existing_id = existing
        .and_then(|e| e.status.as_ref())
        .and_then(|s| s.host_id);
    let system = target.layout == kelvo_remote::AgentLayout::System;
    let needed_bytes = match (existing, probe.disk_total_bytes) {
        (None, Some(total)) => Some(
            kelvo_store::low_disk_threshold(total)
                .saturating_add(u64::from(AGENT_BUDGET_MB).saturating_mul(1_000_000)),
        ),
        _ => None,
    };
    let path = |p: &std::path::Path| p.to_string_lossy().into_owned();
    Ok(RemoteHostPreparation {
        kind: RemoteHostKind::Linux,
        installed: existing.is_some(),
        compatible: existing_version.as_deref() == Some(env!("CARGO_PKG_VERSION")),
        host_id: existing_id.map(|id| id.to_string()),
        name: probe.hostname.clone().unwrap_or_else(|| alias.to_owned()),
        message: None,
        linux: Some(LinuxPreparation {
            layout: if system {
                RemoteAgentLayout::System
            } else {
                RemoteAgentLayout::User
            },
            user: probe.user.clone(),
            distro: probe.pretty_name.clone(),
            kernel: probe.kernel.clone(),
            arch: probe.arch.clone(),
            systemd_version: probe.systemd_version,
            linger_ok: system || probe.linger == Some(true),
            selinux: probe.selinux.clone(),
            binary_path: path(&target.paths.binary),
            data_dir: path(&target.paths.data_dir),
            free_bytes: probe.disk_free_bytes,
            needed_bytes,
            budget_mb: AGENT_BUDGET_MB,
            existing_version,
            docker_readable: probe.docker_socket,
            warnings: probe.warnings(),
            duplicate_of: existing_id
                .and_then(|id| saved.get(&id))
                .map(|h| h.config.name.clone()),
        }),
    })
}

/// The bundled agent for a host's architecture, read and checked against the digest
/// compiled into this build.
pub(in crate::remote) struct AgentBinary {
    pub bytes: Vec<u8>,
    pub sha256: String,
}
impl AgentBinary {
    fn bundled(app: &AppHandle, probe: &LinuxProbe) -> Result<Self, CommandError> {
        let triple = agent_bundle::agent_triple(&probe.arch)?;
        let (_, bytes) = agent_bundle::bundled_agent(app, triple)?;
        Ok(Self::new(bytes))
    }
    pub(in crate::remote) fn new(bytes: Vec<u8>) -> Self {
        Self {
            sha256: agent_bundle::sha256_hex(&bytes),
            bytes,
        }
    }
}

/// What setup did to the host's agent, so recovery undoes exactly that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::remote) enum Installed {
    /// The agent was already there and current; nothing changed.
    Existing,
    /// The agent was there and was updated (an update that fails rolls itself back).
    Updated,
    /// There was no agent; this setup installed it.
    Fresh,
}

/// Installs or updates the agent as `binary` asks: `None` connects to the agent already
/// there. A fresh install that fails part-way is removed again.
pub(in crate::remote) async fn install_agent(
    alias: &str,
    probe: &LinuxProbe,
    binary: Option<&AgentBinary>,
) -> Result<Installed, CommandError> {
    match (probe.existing(), binary) {
        (Some(_), None) => Ok(Installed::Existing),
        (Some(_), Some(b)) => {
            agent_install::update(alias, probe, &b.bytes, &b.sha256).await?;
            Ok(Installed::Updated)
        }
        (None, Some(b)) => {
            match agent_install::install(alias, probe, &b.bytes, &b.sha256, AGENT_BUDGET_MB).await {
                Ok(_) => Ok(Installed::Fresh),
                Err(e) => Err(recover(alias, Installed::Fresh, e.into()).await),
            }
        }
        (None, None) => Err(CommandError::RemoteAgentConflict {
            message: "No Kelvo agent is installed on this host. Install it to connect".into(),
        }),
    }
}

/// A session with the agent `probe` describes, through the configured routes.
pub(in crate::remote) async fn connect(
    config: &RemoteHostConfig,
    command: RemoteCommand,
    local: HostId,
    expected: Option<HostId>,
) -> Result<kelvo_remote::SshSession, CommandError> {
    kelvo_remote::connect(
        &EndpointConfig {
            lan: config.lan.clone(),
            tailscale: config.tailscale.clone(),
            command,
        },
        local,
        expected,
    )
    .await
    .map_err(CommandError::from)
}

/// Undoes a fresh install after `error`; leaves an agent that was there before alone.
pub(in crate::remote) async fn recover(
    alias: &str,
    installed: Installed,
    error: CommandError,
) -> CommandError {
    if installed != Installed::Fresh {
        return error;
    }
    let report = agent_install::uninstall(alias, true).await;
    if report.is_clean() {
        return error;
    }
    tracing::warn!("removing the agent after a failed setup: {report:?}");
    let message = format!(
        "{error}. Kelvo could not remove the agent it installed: {}",
        failed_steps(&report)
    );
    match error {
        CommandError::RemoteAgentUpdateFailed { restored, .. } => {
            CommandError::RemoteAgentUpdateFailed { message, restored }
        }
        _ => failure(message),
    }
}

/// The failed steps of an uninstall, for a warning.
pub(in crate::remote) fn failed_steps(report: &agent_install::UninstallReport) -> String {
    report
        .steps
        .iter()
        .filter_map(|s| match &s.outcome {
            agent_install::StepOutcome::Failed(detail) => Some(format!("{}: {detail}", s.name)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Add host for a Linux probe: install or update the agent when asked, connect,
/// register. Steps with side effects settle before cancellation is observed.
pub(super) async fn add(
    app: &AppHandle,
    alias: &str,
    probe: LinuxProbe,
    mut config: RemoteHostConfig,
    install: bool,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<RemoteHostRecord, CommandError> {
    let state = app.state::<AppState>();
    let binary = if install {
        Some(AgentBinary::bundled(app, &probe)?)
    } else {
        None
    };
    check_setup_cancel(cancel)?;
    let installed = install_agent(alias, &probe, binary.as_ref()).await?;
    let attempt = async {
        check_setup_cancel(cancel)?;
        let command = probe.target()?.remote_command();
        let session =
            setup_step(cancel, connect(&config, command.clone(), state.local, None)).await?;
        let mut record = HostRecord::from_identity(session.hello.host.clone(), false);
        session.close().await;
        if record.id == state.local {
            return Err(failure("This host reports this Mac's identity"));
        }
        if let Some(other) = state.remotes.saved.lock_ok().get(&record.id) {
            return Err(CommandError::RemoteIdentity {
                message: format!(
                    "This host reports the same identity as {}, which is already in your host list",
                    other.config.name
                ),
            });
        }
        check_setup_cancel(cancel)?;
        config.id = record.id.to_string();
        record.display_name = config.name.clone();
        register_new(app, config, record, command).await
    };
    match attempt.await {
        Ok(record) => Ok(record),
        Err(error) => Err(recover(alias, installed, error).await),
    }
}

/// Replaces a saved Linux host's agent with the bundled one, keeping its identity and
/// history, then reconnects. A failed update restores the previous agent.
pub(super) async fn update_saved(app: AppHandle, id: String) -> Result<(), CommandError> {
    let state = app.state::<AppState>();
    let _guard = state.remotes.operation.lock().await;
    let id = host_id(&id)?;
    let saved = state
        .remotes
        .saved
        .lock_ok()
        .get(&id)
        .cloned()
        .ok_or(CommandError::UnknownHost { host: id })?;
    if !saved.is_linux() {
        return Err(CommandError::InvalidArgument {
            message: "Only Linux hosts run the Kelvo agent; update Kelvo on a Mac from Add host"
                .into(),
        });
    }
    let (alias, probe) =
        linux_probe(saved.config.lan.clone(), saved.config.tailscale.clone()).await?;
    let binary = AgentBinary::bundled(&app, &probe)?;
    agent_install::update(&alias, &probe, &binary.bytes, &binary.sha256).await?;
    if let Some(source) = state.remotes.sources.lock_ok().get(&id) {
        source.retry();
    }
    Ok(())
}

/// Removes a saved Linux host's agent (with `purge`, its history and identity too).
/// Bounded and never fails (D-105): a problem is the warning returned.
pub(in crate::remote) async fn uninstall_saved(saved: &SavedHost, purge: bool) -> Option<String> {
    let attempt = async {
        let (alias, _) =
            probe_routes(saved.config.lan.clone(), saved.config.tailscale.clone()).await?;
        Ok::<_, CommandError>(agent_install::uninstall(&alias, purge).await)
    };
    let warning = |detail: String| {
        format!(
            "The host was removed, but Kelvo could not uninstall its agent ({detail}). On the host, stop the kelvo-agent service and run `kelvo-agent uninstall`."
        )
    };
    match tokio::time::timeout(std::time::Duration::from_secs(60), attempt).await {
        Err(_) => Some(warning("the host did not answer".into())),
        Ok(Err(error)) => Some(warning(error.to_string())),
        Ok(Ok(report)) if !report.is_clean() => Some(warning(failed_steps(&report))),
        Ok(Ok(_)) => None,
    }
}

#[cfg(test)]
#[path = "linux_tests.rs"]
mod tests;

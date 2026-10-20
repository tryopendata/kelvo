//! Setup transaction: read-only cancellation boundaries, configure, register and recover.
//! The probe decides the path: [`mac`] installs Kelvo.app, [`linux`] the headless agent.
use super::*;
use kelvo_remote::agent_install::HostProbe;
pub(super) mod linux;
mod mac;
use mac::Installation;
/// Whether a probe is for an install, which validates a Linux host for the agent's
/// history budget, or for an agent already there (update, remove), which does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProbeFor {
    Install,
    Existing,
}

/// Probes each configured route (LAN first, Tailscale 500 ms later) and returns the
/// first that answers, with what it found.
pub(super) async fn probe_routes(
    lan: Option<String>,
    tailscale: Option<String>,
    purpose: ProbeFor,
) -> Result<(String, HostProbe), CommandError> {
    let budget = (purpose == ProbeFor::Install).then_some(linux::AGENT_BUDGET_MB);
    let config = RemoteHostConfig {
        id: String::new(),
        name: "Host".into(),
        lan: lan.clone(),
        tailscale: tailscale.clone(),
        enabled: true,
        mirror_budget_mb: 100,
    };
    validate(&config)?;
    let mut attempts = tokio::task::JoinSet::new();
    for (alias, delay) in [(lan, 0), (tailscale, 500)] {
        if let Some(alias) = alias {
            attempts.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                let result = kelvo_remote::agent_install::probe_host(&alias, budget).await;
                result.map(|probe| (alias, probe))
            });
        }
    }
    let mut errors = Vec::new();
    while let Some(result) = attempts.join_next().await {
        match result {
            Ok(Ok(found)) => return Ok(found),
            Ok(Err(e)) => errors.push(CommandError::from(e)),
            Err(e) => errors.push(failure(e)),
        }
    }
    Err(most_actionable(errors).unwrap_or_else(|| failure("No SSH route is configured")))
}

/// When every route fails, report the error the user can act on (an unknown host key
/// before a refused connection), keeping the other routes' messages beside it.
pub(super) fn most_actionable(errors: Vec<CommandError>) -> Option<CommandError> {
    let rank = |e: &CommandError| match e {
        CommandError::HostKeyUnknown { .. } => 0,
        CommandError::RemoteAuth { .. } => 1,
        CommandError::RemoteIncompatible { .. }
        | CommandError::RemoteIdentity { .. }
        | CommandError::RemoteLowDisk { .. }
        | CommandError::RemoteAgentConflict { .. } => 2,
        _ => 3,
    };
    let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
    let mut best = errors.into_iter().min_by_key(rank)?;
    if messages.len() > 1 {
        let joined = messages.join("\n");
        match &mut best {
            CommandError::HostKeyUnknown { message }
            | CommandError::RemoteAuth { message }
            | CommandError::RemoteIncompatible { message }
            | CommandError::RemoteIdentity { message }
            | CommandError::RemoteUnreachable { message }
            | CommandError::RemoteLowDisk { message }
            | CommandError::RemoteAgentConflict { message }
            | CommandError::RemoteAgentNotRunning { message }
            | CommandError::Internal { message } => *message = joined,
            _ => {}
        }
    }
    Some(best)
}

async fn wait_for_control(alias: &str, helper: &str) -> Result<ControlResponse, CommandError> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        match kelvo_remote::install::status(alias, helper).await {
            Ok(response) if response.ok => return Ok(response),
            result => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(failure(format!(
                        "Kelvo could not start. Complete any macOS approval on the other Mac and retry. {}",
                        match result {
                            Ok(r) => r.message,
                            Err(e) => e.to_string(),
                        }
                    )));
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn prepare_remote_host(
    app: AppHandle,
    lan: Option<String>,
    tailscale: Option<String>,
) -> Result<RemoteHostPreparation, CommandError> {
    match probe_routes(lan, tailscale, ProbeFor::Install).await? {
        (alias, HostProbe::Mac(p)) => mac::prepare(&alias, p).await,
        (alias, HostProbe::Linux(p)) => {
            let saved = app.state::<AppState>().remotes.saved.lock_ok().clone();
            linux::preparation(&alias, &p, &saved)
        }
    }
}

/// Gives the Linux agent at these routes a new host id, for a server whose agent
/// reports the id of a host already in the list (a clone, or a reinstall that kept
/// the data directory). Returns the preparation with the new id.
#[tauri::command]
#[specta::specta]
pub async fn reset_remote_host_identity(
    app: AppHandle,
    lan: Option<String>,
    tailscale: Option<String>,
) -> Result<RemoteHostPreparation, CommandError> {
    let state = app.state::<AppState>();
    let _guard = state.remotes.operation.lock().await;
    let (alias, probe) = linux_probe(lan.clone(), tailscale.clone(), ProbeFor::Install).await?;
    kelvo_remote::agent_install::reset_identity(&alias, &probe).await?;
    let (alias, probe) = linux_probe(lan, tailscale, ProbeFor::Install).await?;
    let saved = state.remotes.saved.lock_ok().clone();
    linux::preparation(&alias, &probe, &saved)
}

/// Updates a saved Linux host's agent to the one this build bundles, keeping its
/// identity and history; a failed update restores the previous agent.
#[tauri::command]
#[specta::specta]
pub async fn update_remote_agent(app: AppHandle, id: String) -> Result<(), CommandError> {
    linux::update_saved(app, id).await
}

/// The probe of a host that must be Linux (an agent operation on a saved host).
pub(super) async fn linux_probe(
    lan: Option<String>,
    tailscale: Option<String>,
    purpose: ProbeFor,
) -> Result<(String, kelvo_remote::agent_install::LinuxProbe), CommandError> {
    match probe_routes(lan, tailscale, purpose).await? {
        (alias, HostProbe::Linux(p)) => Ok((alias, *p)),
        (_, HostProbe::Mac(_)) => Err(CommandError::RemoteIdentity {
            message: "A Mac answered where a Linux host was expected. Check the SSH alias".into(),
        }),
    }
}

/// Registers a host that just answered setup, saves it, and returns its row. A failed
/// save takes the registration back out.
pub(super) async fn register_new(
    app: &AppHandle,
    config: RemoteHostConfig,
    record: HostRecord,
    command: kelvo_remote::RemoteCommand,
) -> Result<RemoteHostRecord, CommandError> {
    let state = app.state::<AppState>();
    let id = record.id;
    let os = record.info.os;
    register(
        app,
        SavedHost {
            config: config.clone(),
            record,
            command,
        },
    )?;
    if let Err(error) = save(app) {
        discard_host_async(app, id).await?;
        return Err(error);
    }
    let status = state
        .remotes
        .statuses
        .lock_ok()
        .get(&id)
        .cloned()
        .unwrap_or_default();
    Ok(RemoteHostRecord { config, os, status })
}

struct SetupGuard {
    app: AppHandle,
    id: String,
}
impl Drop for SetupGuard {
    fn drop(&mut self) {
        self.app
            .state::<AppState>()
            .remotes
            .setups
            .lock_ok()
            .remove(&self.id);
    }
}
fn check_setup_cancel(cancel: &tokio::sync::watch::Receiver<bool>) -> Result<(), CommandError> {
    if *cancel.borrow() {
        Err(failure(
            "Setup cancelled. Any completed installation is retained; retry to resume.",
        ))
    } else {
        Ok(())
    }
}
/// Only read-only steps may be dropped. File swaps and SettingsOwner writes must
/// settle before observing cancellation, so recovery owns their complete result.
pub(super) async fn setup_step<T>(
    cancel: &mut tokio::sync::watch::Receiver<bool>,
    step: impl std::future::Future<Output = Result<T, CommandError>>,
) -> Result<T, CommandError> {
    if *cancel.borrow() {
        return Err(failure(
            "Setup cancelled. Any completed installation is retained; retry to resume.",
        ));
    }
    tokio::select! {biased;
        _=cancel.changed()=>Err(failure("Setup cancelled. Any completed installation is retained; retry to resume.")),
        result=step=>result,
    }
}
#[tauri::command]
#[specta::specta]
pub fn cancel_remote_setup(
    state: State<'_, AppState>,
    setup_id: String,
) -> Result<(), CommandError> {
    if let Some(cancel) = state.remotes.setups.lock_ok().get(&setup_id) {
        let _ = cancel.send(true);
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn add_remote_host(
    app: AppHandle,
    config: RemoteHostConfig,
    install: bool,
    show_menu_bar: bool,
    setup_id: String,
) -> Result<RemoteHostRecord, CommandError> {
    validate(&config)?;
    let state = app.state::<AppState>();
    Uuid::parse_str(&setup_id).map_err(failure)?;
    let (tx, mut cancel) = tokio::sync::watch::channel(false);
    {
        let mut setups = state.remotes.setups.lock_ok();
        if setups.contains_key(&setup_id) {
            return Err(failure("This setup is already running"));
        }
        setups.insert(setup_id.clone(), tx);
    }
    let _setup = SetupGuard {
        app: app.clone(),
        id: setup_id,
    };
    let _guard = setup_step(&mut cancel, async {
        Ok(state.remotes.operation.lock().await)
    })
    .await?;
    let (alias, probe) = setup_step(
        &mut cancel,
        probe_routes(
            config.lan.clone(),
            config.tailscale.clone(),
            ProbeFor::Install,
        ),
    )
    .await?;
    let probe = match probe {
        HostProbe::Linux(probe) => {
            return linux::add(&app, &alias, *probe, config, install, &mut cancel).await;
        }
        HostProbe::Mac(probe) => probe,
    };
    let installation = Installation::prepare(&alias, probe, install).await?;
    let mut before = None;
    let result = installation
        .connect_and_register(&app, config, show_menu_bar, &mut before, &mut cancel)
        .await;
    match result {
        Ok(record) => {
            installation.finish().await;
            Ok(record)
        }
        Err(error) => Err(installation.recover(before, error).await),
    }
}

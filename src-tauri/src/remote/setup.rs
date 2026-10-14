//! Setup transaction: read-only cancellation boundaries, configure, register and recover.
use super::*;
mod installation;
use installation::Installation;
pub(super) async fn probe_routes(
    lan: Option<String>,
    tailscale: Option<String>,
) -> Result<(String, kelvo_remote::install::InstallProbe), CommandError> {
    let config = RemoteHostConfig {
        id: String::new(),
        name: "Mac".into(),
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
                let result = kelvo_remote::install::probe(&alias).await;
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
        CommandError::RemoteIncompatible { .. } | CommandError::RemoteIdentity { .. } => 2,
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
    lan: Option<String>,
    tailscale: Option<String>,
) -> Result<RemoteHostPreparation, CommandError> {
    let (alias, p) = probe_routes(lan, tailscale).await?;
    let compatible = if let Some(helper) = p.helper_path.as_deref() {
        kelvo_remote::install::version(&alias, helper)
            .await
            .is_ok_and(|version| version == env!("CARGO_PKG_VERSION"))
    } else {
        false
    };
    Ok(RemoteHostPreparation {
        installed: p.app_path.is_some(),
        compatible,
        host_id: None,
        name: p.user.clone(),
        message: if !compatible {
            Some(if p.app_path.is_some(){"Quit Kelvo on the other Mac, then update it with this build. Your settings and history are kept."}else{"Install this Kelvo build to enable remote monitoring"}.into())
        } else {
            None
        },
    })
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
        probe_routes(config.lan.clone(), config.tailscale.clone()),
    )
    .await?;
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

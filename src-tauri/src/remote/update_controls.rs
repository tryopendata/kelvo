//! Update policy is read from its owning host, never cached in the controller's settings.
use super::*;
use kelvo_remote::agent_install::HostProbe;
use kelvo_remote::control::{self, RemoteUpdateStatus};

pub(super) fn local_status(app: &AppHandle) -> RemoteUpdateStatus {
    let settings = app.state::<AppState>().settings.settings();
    let status = app.state::<crate::updates::Updates>().status();
    // The shell's status enum remains the source of truth; SSH exposes a compact,
    // forward-compatible summary rather than the desktop event's entire payload.
    let value = serde_json::to_value(status).unwrap_or_default();
    let text = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let kind = text("kind").unwrap_or_else(|| "failed".into());
    RemoteUpdateStatus {
        supported: kind != "not_configured",
        check_updates: settings.general.check_updates,
        auto_install_updates: settings.general.auto_install_updates,
        status: kind,
        version: text("version"),
        message: text("message").or_else(|| text("reason")),
    }
}

fn saved_host(app: &AppHandle, id: &str) -> Result<SavedHost, CommandError> {
    let id = host_id(id)?;
    app.state::<AppState>()
        .remotes
        .saved
        .lock_ok()
        .get(&id)
        .cloned()
        .ok_or(CommandError::UnknownHost { host: id })
}

async fn read_or_write(
    saved: SavedHost,
    policy: Option<(bool, bool)>,
) -> Result<RemoteUpdateStatus, CommandError> {
    let (alias, probe) = probe_routes(
        saved.config.lan.clone(),
        saved.config.tailscale.clone(),
        setup::ProbeFor::Existing,
    )
    .await?;
    match probe {
        HostProbe::Linux(probe) => {
            let existing = probe
                .existing()
                .ok_or_else(|| failure("No Kelvo agent is installed on this host"))?;
            if existing.status.as_ref().and_then(|s| s.host_id) != Some(saved.record.id) {
                return Err(CommandError::RemoteIdentity {
                    message:
                        "The host identity changed. Re-pair it before changing update settings."
                            .into(),
                });
            }
            if !existing
                .version
                .as_ref()
                .is_some_and(|v| v.automatic_updates)
            {
                return Ok(RemoteUpdateStatus::unsupported());
            }
            let target = probe.target()?;
            match policy {
                Some((check, install)) => {
                    control::agent_update_policy(&alias, &target, check, install)
                        .await
                        .map_err(Into::into)
                }
                None => control::agent_update_status(&alias, &target)
                    .await
                    .map_err(Into::into),
            }
        }
        HostProbe::Mac(probe) => {
            let helper = probe
                .helper_path
                .ok_or_else(|| failure("The remote helper is missing"))?;
            let snapshot = kelvo_remote::install::status(&alias, &helper).await?;
            if snapshot.host.as_ref().map(|h| h.id) != Some(saved.record.id) {
                return Err(CommandError::RemoteIdentity {
                    message:
                        "The host identity changed. Re-pair it before changing update settings."
                            .into(),
                });
            }
            if snapshot.updates.is_none() {
                return Ok(RemoteUpdateStatus::unsupported());
            }
            let response = if let Some((check_updates, auto_install_updates)) = policy {
                kelvo_remote::install::configure(
                    &alias,
                    &helper,
                    &ControlRequest::UpdatePolicy {
                        check_updates,
                        auto_install_updates,
                    },
                )
                .await?
            } else {
                snapshot
            };
            if !response.ok {
                return Err(failure(response.message));
            }
            response
                .updates
                .ok_or_else(|| failure("The host returned no update status"))
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_remote_update_status(
    app: AppHandle,
    id: String,
) -> Result<RemoteUpdateStatus, CommandError> {
    let saved = saved_host(&app, &id)?;
    bounded(read_or_write(saved, None)).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_remote_update_policy(
    app: AppHandle,
    id: String,
    check_updates: bool,
    auto_install_updates: bool,
) -> Result<RemoteUpdateStatus, CommandError> {
    let _maintenance = crate::updates::maintenance(&app)?;
    let state = app.state::<AppState>();
    let _operation = state.remotes.operation.lock().await;
    let saved = saved_host(&app, &id)?;
    bounded(read_or_write(
        saved,
        Some((check_updates, auto_install_updates)),
    ))
    .await
}

async fn bounded<T>(
    operation: impl std::future::Future<Output = Result<T, CommandError>>,
) -> Result<T, CommandError> {
    tokio::time::timeout(std::time::Duration::from_secs(45), operation)
        .await
        .map_err(|_| CommandError::RemoteUnreachable {
            message: "The host did not answer the update settings request.".into(),
        })?
}

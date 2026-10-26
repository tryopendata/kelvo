//! Host-list and trust commands; Tauri state and event effects stay in the shell.
use super::*;
#[tauri::command]
#[specta::specta]
pub fn list_remote_hosts(state: State<'_, AppState>) -> Vec<RemoteHostRecord> {
    let statuses = state.remotes.statuses.lock_ok().clone();
    state
        .remotes
        .saved
        .lock_ok()
        .iter()
        .map(|(id, h)| {
            let mut status = statuses.get(id).cloned().unwrap_or_default();
            if let Ok(entry) = state.host(*id)
                && let Some(frame) = entry.latest_frame()
            {
                let value = |metric: &str| {
                    frame
                        .layout
                        .series
                        .iter()
                        .position(|key| key.metric.as_str() == metric && key.labels.is_empty())
                        .and_then(|i| frame.held.get(i))
                        .copied()
                        .filter(|v| v.is_finite())
                        .map(f64::from)
                };
                status.cpu_pct = value("cpu.total");
                if h.record.info.mem_total_bytes > 0 {
                    status.memory_pct = value("mem.used")
                        .map(|used| used / h.record.info.mem_total_bytes as f64 * 100.0);
                }
            }
            status.update_available = h.is_linux()
                && status
                    .agent_version
                    .as_deref()
                    .is_some_and(|v| older_version(v, env!("CARGO_PKG_VERSION")));
            RemoteHostRecord {
                config: h.config.clone(),
                os: h.record.info.os,
                status,
            }
        })
        .collect()
}

/// Whether dotted version `a` is older than `b`, numerically per component. A
/// component that is not a number compares as 0, so a malformed version never offers
/// a downgrade.
pub(super) fn older_version(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['.', '-', '+'])
            .take(3)
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parts(a) < parts(b)
}

#[tauri::command]
#[specta::specta]
pub async fn inspect_remote_ssh_host(
    app: AppHandle,
    alias: String,
) -> Result<RemoteSshHostTrust, CommandError> {
    let candidate = kelvo_remote::trust::inspect_host(&alias)
        .await
        .map_err(CommandError::from)?;
    let token = Uuid::new_v4().to_string();
    let prompt = RemoteSshHostTrust {
        token: token.clone(),
        alias: candidate.alias.clone(),
        hostname: candidate.hostname.clone(),
        port: candidate.port,
        fingerprints: candidate.fingerprints.clone(),
    };
    let state = app.state::<AppState>();
    let mut pending = state.remotes.trust.lock_ok();
    pending.retain(|_, (at, _)| at.elapsed() < std::time::Duration::from_secs(300));
    if pending.len() >= 16 {
        return Err(failure(
            "Too many pending SSH verifications. Retry in five minutes",
        ));
    }
    pending.insert(token, (std::time::Instant::now(), candidate));
    Ok(prompt)
}
#[tauri::command]
#[specta::specta]
pub async fn accept_remote_ssh_host(app: AppHandle, token: String) -> Result<(), CommandError> {
    let state = app.state::<AppState>();
    let (at, candidate) = state
        .remotes
        .trust
        .lock_ok()
        .remove(&token)
        .ok_or_else(|| failure("SSH verification expired. Check the host again"))?;
    if at.elapsed() >= std::time::Duration::from_secs(300) {
        return Err(failure("SSH verification expired. Check the host again"));
    }
    kelvo_remote::trust::accept_host(&candidate)
        .await
        .map_err(CommandError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn update_remote_host(
    app: AppHandle,
    config: RemoteHostConfig,
) -> Result<RemoteHostRecord, CommandError> {
    let _maintenance = crate::updates::maintenance(&app)?;
    validate(&config)?;
    let state = app.state::<AppState>();
    let _guard = state.remotes.operation.lock().await;
    let id = host_id(&config.id)?;
    let previous = state
        .remotes
        .saved
        .lock_ok()
        .get(&id)
        .cloned()
        .ok_or(CommandError::UnknownHost { host: id })?;
    let mut saved = previous.clone();
    saved.config = config.clone();
    let os = saved.record.info.os;
    saved.record.display_name = config.name.clone();
    discard_host_async(&app, id).await?;
    if let Err(error) = register(&app, saved).and_then(|()| save(&app)) {
        discard_host_async(&app, id).await?;
        if let Err(restore) = register(&app, previous) {
            return Err(failure(format!(
                "{error}; restoring the previous connection failed: {restore}"
            )));
        }
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
#[tauri::command]
#[specta::specta]
pub async fn retry_remote_host(app: AppHandle, id: String) -> Result<(), CommandError> {
    let state = app.state::<AppState>();
    let id = host_id(&id)?;
    let sources = state.remotes.sources.lock_ok();
    let source = sources
        .get(&id)
        .ok_or(CommandError::UnknownHost { host: id })?;
    source.retry();
    Ok(())
}
fn stop_sharing_request() -> ControlRequest {
    ControlRequest::Configure {
        allow_remote_monitoring: false,
        show_menu_bar_icon: None,
        launch_at_login: None,
    }
}

/// Bounded so removing an offline Mac doesn't hang on SSH.
async fn stop_remote_sharing(saved: &SavedHost) -> Result<(), CommandError> {
    let attempt = async {
        let (alias, _) = probe_routes(
            saved.config.lan.clone(),
            saved.config.tailscale.clone(),
            super::setup::ProbeFor::Existing,
        )
        .await?;
        let RemoteCommand::MacHelper(helper) = &saved.command else {
            return Ok(());
        };
        configure_stop_sharing(|request| async move {
            kelvo_remote::install::configure(&alias, helper, &request)
                .await
                .map_err(CommandError::from)
        })
        .await
    };
    tokio::time::timeout(std::time::Duration::from_secs(15), attempt)
        .await
        .map_err(|_| CommandError::RemoteUnreachable {
            message: "The other Mac did not answer.".into(),
        })?
}

pub(super) async fn configure_stop_sharing<
    F: std::future::Future<Output = Result<ControlResponse, CommandError>>,
>(
    configure: impl FnOnce(ControlRequest) -> F,
) -> Result<(), CommandError> {
    let response = configure(stop_sharing_request()).await?;
    if response.ok {
        Ok(())
    } else {
        Err(CommandError::RemoteUnreachable {
            message: response.message,
        })
    }
}

/// A remote settings failure must never prevent completing the local removal.
pub(super) async fn finish_removal(
    stop_sharing: bool,
    configure: impl std::future::Future<Output = Result<(), CommandError>>,
    remove: impl std::future::Future<Output = Result<(), CommandError>>,
) -> Result<Option<String>, CommandError> {
    let warning = if stop_sharing {
        configure.await.err().map(|error| {
            format!("The Mac was removed, but Kelvo could not stop sharing on it. Turn off Allow remote monitoring in Kelvo on that Mac. {error}")
        })
    } else {
        None
    };
    remove.await?;
    Ok(warning)
}

/// Removes a host. A Mac can also stop sharing; a Linux host can also have its agent
/// uninstalled (with `purge_agent`, its history and identity too). Remote failures
/// warn and never block removal (D-105).
#[tauri::command]
#[specta::specta]
pub async fn remove_remote_host(
    app: AppHandle,
    id: String,
    delete_mirror: bool,
    stop_sharing: bool,
    uninstall_agent: bool,
    purge_agent: bool,
) -> Result<Option<String>, CommandError> {
    let _maintenance = crate::updates::maintenance(&app)?;
    let state = app.state::<AppState>();
    let _guard = state.remotes.operation.lock().await;
    let id = host_id(&id)?;
    if id == state.local {
        return Err(failure("The local Mac cannot be removed"));
    }
    let previous = state
        .remotes
        .saved
        .lock_ok()
        .remove(&id)
        .ok_or(CommandError::UnknownHost { host: id })?;
    if let Err(error) = save(&app) {
        state.remotes.saved.lock_ok().insert(id, previous);
        return Err(error);
    }
    let warning = if previous.is_linux() {
        let warning = if uninstall_agent {
            // Stop the session first so the agent's uninstall isn't racing a reconnect.
            if let Ok(entry) = state.host(id) {
                let entry = entry.clone();
                let _ = tokio::task::spawn_blocking(move || entry.stop()).await;
            }
            super::setup::linux::uninstall_saved(&previous, purge_agent).await
        } else {
            None
        };
        discard_host_async(&app, id).await?;
        warning
    } else {
        finish_removal(
            stop_sharing,
            stop_remote_sharing(&previous),
            discard_host_async(&app, id),
        )
        .await?
    };
    state.emit_hosts_changed(&app);
    if delete_mirror {
        let dir = data_dir(&app)?.join("mirrors").join(id.to_string());
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(failure)?;
        }
    }
    Ok(warning)
}

/// Registers `next` in place of a registration just discarded; when that fails, puts
/// the previous one back (`restore`) and returns the error, so a failed step never
/// leaves the host missing from the list.
pub(super) fn register_or_restore(
    next: impl FnOnce() -> Result<(), CommandError>,
    restore: impl FnOnce() -> Result<(), CommandError>,
) -> Result<(), CommandError> {
    let Err(error) = next() else {
        return Ok(());
    };
    match restore() {
        Ok(()) => Err(error),
        Err(restore) => Err(failure(format!(
            "{error}; restoring the previous connection failed: {restore}"
        ))),
    }
}

/// Re-pairs a host that now answers with a different id (a reinstall, a reset
/// identity, a replaced machine). The answering host is added as a new host with the
/// same name and routes; the old one is kept, disabled, with its mirrored history
/// (D-116).
#[tauri::command]
#[specta::specta]
pub async fn repair_remote_host(
    app: AppHandle,
    id: String,
) -> Result<RemoteHostRecord, CommandError> {
    let _maintenance = crate::updates::maintenance(&app)?;
    let state = app.state::<AppState>();
    let _guard = state.remotes.operation.lock().await;
    let id = host_id(&id)?;
    let previous = state
        .remotes
        .saved
        .lock_ok()
        .get(&id)
        .cloned()
        .ok_or(CommandError::UnknownHost { host: id })?;
    let session = kelvo_remote::connect(&previous.endpoint(), state.local, None)
        .await
        .map_err(CommandError::from)?;
    let mut record = HostRecord::from_identity(session.hello.host.clone(), false);
    session.close().await;
    if record.id == id {
        if let Some(source) = state.remotes.sources.lock_ok().get(&id) {
            source.retry();
        }
        return Err(CommandError::InvalidArgument {
            message:
                "This host answers with its paired identity again; it reconnects without re-pairing"
                    .into(),
        });
    }
    if record.id == state.local {
        return Err(failure("This host reports this Mac's identity"));
    }
    if let Some(other) = state.remotes.saved.lock_ok().get(&record.id) {
        return Err(CommandError::RemoteIdentity {
            message: format!(
                "This host now reports the identity of {}, which is already in your host list",
                other.config.name
            ),
        });
    }
    let mut old = previous.clone();
    old.config.enabled = false;
    old.config.name = format!("{} (before re-pair)", previous.config.name);
    old.record.display_name = old.config.name.clone();
    discard_host_async(&app, id).await?;
    register_or_restore(|| register(&app, old), || register(&app, previous.clone()))?;
    let mut config = previous.config.clone();
    config.id = record.id.to_string();
    config.enabled = true;
    record.display_name = config.name.clone();
    super::setup::register_new(&app, config, record, previous.command.clone()).await
}

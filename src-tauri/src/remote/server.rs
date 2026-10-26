//! Tauri adapters for local metrics/control servers and their lifecycle.
use super::*;
use kelvo_engine::disk_space::SpaceService;
pub fn start(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let path = saved_path(app)?;
    if let Ok(bytes) = std::fs::read(&path) {
        let (saved, errors) = parse_saved(&bytes);
        if !errors.is_empty() {
            // The next save rewrites the file without these hosts; keep the original.
            let kept = path.with_extension("json.unreadable");
            tracing::warn!(
                "remote hosts could not be read ({}); the file is kept at {}",
                errors.join("; "),
                kept.display()
            );
            if let Err(e) = std::fs::copy(&path, &kept) {
                tracing::warn!("keeping the unreadable remote host file: {e}");
            }
        }
        *state.remotes.saved.lock_ok() = saved;
    }
    let (stop, mut rx) = tokio::sync::watch::channel(false);
    *state.remotes.stop.lock_ok() = Some(stop);
    let paths = SocketPaths::new(data_dir(app)?);
    let handle = app.clone();
    let control = Arc::new(move |request| control(&handle, request));
    tauri::async_runtime::spawn(async move {
        loop {
            if *rx.borrow() {
                break;
            }
            if let Err(e) =
                kelvo_remote::serve_control(paths.control.clone(), control.clone(), rx.clone())
                    .await
            {
                tracing::error!("remote control socket: {e}");
            } else {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {},
                _ = rx.changed() => break,
            }
        }
    });
    refresh_metrics(app, state.settings.settings().remote_access.enabled);
    let handle = app.clone();
    *state.remotes.monitor.lock_ok() = match crate::remote_network::start(
        app,
        Arc::new(move |event| {
            let state = handle.state::<AppState>();
            for source in state.remotes.sources.lock_ok().values() {
                match event {
                    crate::remote_network::Event::NetworkChanged => source.network_changed(),
                    crate::remote_network::Event::WillSleep => source.sleep(),
                    crate::remote_network::Event::DidWake => source.wake(),
                }
            }
        }),
    ) {
        Ok(monitor) => Some(monitor),
        Err(error) => {
            tracing::warn!("remote network observer unavailable: {error}");
            None
        }
    };
    let saved: Vec<_> = state.remotes.saved.lock_ok().values().cloned().collect();
    for h in saved {
        if let Err(e) = register(app, h) {
            tracing::warn!("restoring remote host: {e}");
        }
    }
    Ok(())
}

fn control_response(app: &AppHandle, ok: bool, message: String) -> ControlResponse {
    let state = app.state::<AppState>();
    let settings = state.settings.settings();
    ControlResponse {
        ok,
        message,
        host: state.host(state.local).ok().map(|h| h.record()),
        allow_remote_monitoring: Some(settings.remote_access.enabled),
        show_menu_bar_icon: Some(settings.general.show_menu_bar),
        launch_at_login: Some(settings.general.launch_at_login),
        updates: Some(super::update_controls::local_status(app)),
    }
}
fn control(app: &AppHandle, request: ControlRequest) -> ControlResponse {
    match request {
        ControlRequest::Status | ControlRequest::UpdateStatus => {
            control_response(app, true, "Kelvo is running".into())
        }
        ControlRequest::UpdatePolicy {
            check_updates,
            auto_install_updates,
        } => {
            let _maintenance = match crate::updates::maintenance(app) {
                Ok(guard) => guard,
                Err(error) => return control_response(app, false, error.to_string()),
            };
            let patch = crate::settings::SettingsPatch {
                general: Some(crate::settings::GeneralPatch {
                    check_updates: Some(check_updates),
                    auto_install_updates: Some(auto_install_updates),
                    ..Default::default()
                }),
                ..Default::default()
            };
            match app.state::<AppState>().update_settings(app, &patch) {
                Ok(_) => control_response(app, true, "Update policy saved".into()),
                Err(error) => control_response(app, false, error.to_string()),
            }
        }
        ControlRequest::Configure {
            allow_remote_monitoring,
            show_menu_bar_icon,
            launch_at_login,
        } => {
            let _maintenance = match crate::updates::maintenance(app) {
                Ok(guard) => guard,
                Err(error) => return control_response(app, false, error.to_string()),
            };
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let handle = app.clone();
            if let Err(e) = app.run_on_main_thread(move || {
                let state = handle.state::<AppState>();
                let patch = crate::settings::SettingsPatch {
                    remote_access: Some(crate::settings::RemoteAccessPatch {
                        enabled: Some(allow_remote_monitoring),
                    }),
                    general: Some(crate::settings::GeneralPatch {
                        show_menu_bar: show_menu_bar_icon,
                        launch_at_login,
                        ..Default::default()
                    }),
                    onboarding: Some(crate::settings::OnboardingPatch {
                        completed: Some(true),
                    }),
                    ..Default::default()
                };
                let _ = tx.send(state.update_settings(&handle, &patch));
            }) {
                return control_response(app, false, e.to_string());
            }
            match rx.recv_timeout(std::time::Duration::from_secs(15)) {
                Ok(Ok(_)) => control_response(app, true, "Remote monitoring configured".into()),
                Ok(Err(e)) => control_response(app, false, e.to_string()),
                Err(e) => control_response(app, false, e.to_string()),
            }
        }
    }
}

/// Starts or stops sharing over the metrics socket. `enabled` comes from the caller: see
/// [`crate::settings::SettingsEffects`] for why the settings-change path can't read it.
pub fn refresh_metrics(app: &AppHandle, enabled: bool) {
    let state = app.state::<AppState>();
    let mut setting = state.remotes.metrics_stop.lock_ok();
    if let Some(tx) = setting.as_ref() {
        let _ = tx.send(enabled);
        return;
    }
    let Ok(entry) = state.host(state.local) else {
        return;
    };
    let Ok(dir) = data_dir(app) else { return };
    // Remote sessions get their own space service, apart from the dashboard's (D-117),
    // in editions that scan at all.
    let mut server = HostServer::new(entry, state.history.store());
    if crate::disk_space::available() {
        server = server.with_space(Arc::new(SpaceService::new()));
    }
    let provider = Arc::new(server);
    let path = SocketPaths::new(dir).metrics;
    let (tx, mut desired) = tokio::sync::watch::channel(enabled);
    *setting = Some(tx);
    // One owner serializes bind/unlink, including rapid off/on settings changes.
    tauri::async_runtime::spawn(async move {
        loop {
            if !*desired.borrow_and_update() {
                if desired.changed().await.is_err() {
                    break;
                }
                continue;
            }
            let (stop, rx) = tokio::sync::watch::channel(false);
            let server = kelvo_remote::serve_metrics(path.clone(), provider.clone(), rx);
            tokio::pin!(server);
            tokio::select! {
                result=&mut server=>{
                    if let Err(e)=result{tracing::error!("remote metrics socket: {e}");}
                    tokio::select!{_ = tokio::time::sleep(std::time::Duration::from_secs(1))=>{}, changed=desired.changed()=>{if changed.is_err(){break;}}}
                }
                changed=desired.changed()=>{
                    let _=stop.send(true);
                    let _=server.await;
                    if changed.is_err(){break;}
                }
            }
        }
    });
}

pub fn shutdown(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.remotes.monitor.lock_ok().take();
    for (_, watcher) in std::mem::take(&mut *state.remotes.watchers.lock_ok()) {
        watcher.abort();
    }
    if let Some(tx) = state.remotes.stop.lock_ok().take() {
        let _ = tx.send(true);
    }
    state.remotes.metrics_stop.lock_ok().take();
}

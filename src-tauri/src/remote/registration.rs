//! Install/remove one registered source; generation guards reject old callbacks.
use super::*;
pub(super) fn register(app: &AppHandle, saved: SavedHost) -> Result<(), CommandError> {
    let state = app.state::<AppState>();
    let id = saved.record.id;
    if id == state.local {
        return Err(CommandError::InvalidArgument {
            message: "This is this Mac".into(),
        });
    }
    let dir = data_dir(app)?.join("mirrors").join(id.to_string());
    std::fs::create_dir_all(&dir).map_err(failure)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(failure)?;
    }
    let history = Arc::new(History::open(&dir));
    history.register_host(&saved.record);
    let mirror = Arc::new(Mirror::new(
        history.store(),
        id,
        saved.config.mirror_budget_mb,
    ));
    let generation = Uuid::new_v4();
    state.remotes.generations.lock_ok().insert(id, generation);
    let handle = app.clone();
    let cb = Arc::new(move |s: kelvo_remote::ConnectionStatus| {
        let state = handle.state::<AppState>();
        if state.remotes.generations.lock_ok().get(&id) != Some(&generation) {
            return;
        }
        state
            .remotes
            .statuses
            .lock_ok()
            .insert(id, connection_status(s));
    });
    let source = RemoteSource::new(
        saved.record.clone(),
        state.local,
        saved.endpoint(),
        cb,
        Some(mirror.clone()),
    );
    source.set_controller_on_battery(state.host(state.local)?.hub().status().on_battery);
    state.remotes.sources.lock_ok().insert(id, source.clone());
    let entry = state.hosts.insert(saved.record.clone(), source);
    state.remotes.mirrors.lock_ok().insert(
        id,
        RegisteredMirror {
            history,
            sink: mirror,
        },
    );
    state.remotes.saved.lock_ok().insert(id, saved.clone());
    state.remotes.statuses.lock_ok().insert(
        id,
        RemoteHostStatus {
            state: if saved.config.enabled {
                RemoteState::Connecting
            } else {
                RemoteState::Disabled
            },
            ..Default::default()
        },
    );
    let watcher = crate::state::spawn_host_watcher(app.clone(), entry.clone());
    if let Some(old) = state.remotes.watchers.lock_ok().insert(id, watcher) {
        old.abort();
    }
    if saved.config.enabled
        && let Err(error) = entry.start(None)
    {
        discard_host(&state, id);
        return Err(failure(error));
    }
    state.emit_hosts_changed(app);
    Ok(())
}

pub(super) async fn discard_host_async(app: &AppHandle, id: HostId) -> Result<(), CommandError> {
    let app = app.clone();
    tokio::task::spawn_blocking(move || discard_host(&app.state::<AppState>(), id))
        .await
        .map_err(failure)
}

fn discard_host(state: &AppState, id: HostId) {
    if let Some(watcher) = state.remotes.watchers.lock_ok().remove(&id) {
        watcher.abort();
    }
    state.remotes.generations.lock_ok().remove(&id);
    if let Ok(entry) = state.host(id) {
        entry.stop();
    }
    state.hosts.remove(id);
    state.remotes.sources.lock_ok().remove(&id);
    state.remotes.saved.lock_ok().remove(&id);
    state.remotes.statuses.lock_ok().remove(&id);
    if let Some(m) = state.remotes.mirrors.lock_ok().remove(&id) {
        m.history.close();
    }
}

fn connection_status(s: kelvo_remote::ConnectionStatus) -> RemoteHostStatus {
    RemoteHostStatus {
        state: if s.connected {
            RemoteState::Connected
        } else if s.retrying {
            RemoteState::Connecting
        } else if s.error.is_some() {
            RemoteState::Error
        } else {
            RemoteState::Offline
        },
        route: s.route.map(|r| match r {
            kelvo_remote::Route::Lan => RemoteRoute::Lan,
            kelvo_remote::Route::Tailscale => RemoteRoute::Tailscale,
        }),
        last_seen_ms: s.last_seen_ms.map(|m| m as f64),
        error: s.error.or(s.history_error.clone()),
        sync: if s.history_error.is_some() {
            RemoteSync::Unavailable
        } else if s.catching_up {
            RemoteSync::CatchingUp
        } else if s.connected {
            RemoteSync::Current
        } else {
            RemoteSync::Idle
        },
        clock_offset_ms: s.clock_offset_ms as f64,
        cpu_pct: None,
        memory_pct: None,
        agent_version: s.remote_version,
        update_available: false,
        identity_changed: s.identity_changed.map(|id| id.to_string()),
    }
}

//! Tauri adapters for local metrics/control servers and their lifecycle.
use super::*;
struct LocalServer {
    app: AppHandle,
    entry: Arc<HostEntry>,
}
impl ServerProvider for LocalServer {
    fn hello(&self) -> Hello {
        let state = self.app.state::<AppState>();
        Hello {
            proto_version: kelvo_proto::PROTO_VERSION,
            min_compatible: kelvo_proto::MIN_COMPATIBLE,
            app_version: env!("CARGO_PKG_VERSION").into(),
            host: self.entry.record().identity(),
            db_instance_uuid: state.history.read(|r| Ok(r.epoch())).unwrap_or(Uuid::nil()),
            capabilities: self.entry.capabilities(),
            features: kelvo_proto::local_features(),
        }
    }
    fn live(&self) -> LiveHub {
        self.entry.hub().clone()
    }
    fn status(&self) -> SourceStatus {
        let s = self.entry.hub().status();
        SourceStatus {
            interval_ms: s.interval_ms,
            paused: s.paused,
            asleep: s.asleep,
            on_battery: s.on_battery,
            low_power_mode: s.low_power_mode,
            backed_off: s.backed_off,
            backgrounded: s.backgrounded,
            display_idle: s.display_idle,
            history_held_until: s.history_held_until,
            performance: s.performance,
            power_source: s.power_source,
            primary_iface: s.primary_iface.as_deref().map(str::to_owned),
            history_available: self.app.state::<AppState>().history.is_available(),
        }
    }
    fn recent(&self, from: i64, to: i64) -> Result<Message, String> {
        let now = kelvo_engine::wall_ms();
        let state = self.app.state::<AppState>();
        let rows = self
            .entry
            .recent_rows(state.local, from.max(now - 300_000), to.min(now));
        let mut layouts: Vec<WireLayout> = Vec::new();
        let mut wire = Vec::new();
        for row in rows.into_iter().filter(|r| r.tier == Tier::S10) {
            let idx = layouts
                .iter()
                .position(|l| l.series.as_slice() == row.series.as_ref())
                .unwrap_or_else(|| {
                    let i = layouts.len();
                    layouts.push(WireLayout {
                        layout_no: i as u32,
                        series: row.series.to_vec(),
                    });
                    i
                });
            wire.push(WireBucket {
                seq: 0,
                bucket_ts: row.bucket_ts,
                layout_no: idx as u32,
                stats: row.stats,
            });
        }
        let message = Message::RecentRows {
            epoch: self.hello().db_instance_uuid,
            tier: Tier::S10,
            complete_to_ms: now.div_euclid(10_000) * 10_000,
            layouts,
            rows: wire,
        };
        if kelvo_proto::encode_message(&message)
            .map_err(|e| e.to_string())?
            .len()
            > 256 * 1024
        {
            return Err(
                "Recent history exceeds the page limit; durable history remains available".into(),
            );
        }
        Ok(message)
    }

    fn set_interest(&self, session: u64, interest: RemoteInterest) {
        self.entry.remote_interest(session, interest);
    }
    fn sync(&self, req: &SyncRequest, kinds: SyncKinds) -> Result<Message, String> {
        let state = self.app.state::<AppState>();
        let mut max = req.max_rows.clamp(1, 2000);
        let cap = req.max_bytes.unwrap_or(16 * 1024).clamp(1024, 256 * 1024) as usize;
        loop {
            let read = state
                .history
                .read(|r| {
                    r.read_after_since(state.local, req.tier, req.after, max, kinds, req.from_ms)
                })
                .map_err(|e| e.to_string())?;
            let message = kelvo_remote::page_to_wire(read);
            if kelvo_proto::encode_message(&message)
                .map_err(|e| e.to_string())?
                .len()
                <= cap
            {
                return Ok(message);
            }
            if max == 1 {
                if kelvo_proto::encode_message(&message)
                    .map_err(|e| e.to_string())?
                    .len()
                    <= 256 * 1024
                {
                    return Ok(message);
                }
                return Err("A history row exceeds the maximum page size".into());
            }
            max = (max / 2).max(1);
        }
    }
}

pub fn start(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    if let Ok(bytes) = std::fs::read(saved_path(app)?) {
        match serde_json::from_slice::<BTreeMap<HostId, SavedHost>>(&bytes) {
            Ok(saved) => *state.remotes.saved.lock_ok() = saved,
            Err(e) => tracing::warn!("remote host configuration could not be read: {e}"),
        }
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
    }
}
fn control(app: &AppHandle, request: ControlRequest) -> ControlResponse {
    match request {
        ControlRequest::Status => control_response(app, true, "Kelvo is running".into()),
        ControlRequest::Configure {
            allow_remote_monitoring,
            show_menu_bar_icon,
            launch_at_login,
        } => {
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
    let provider = Arc::new(LocalServer {
        app: app.clone(),
        entry,
    });
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

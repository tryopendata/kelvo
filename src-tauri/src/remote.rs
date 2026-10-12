//! Mac-to-Mac monitoring: shell adapters, saved hosts and host-specific history.
//! Transport and protocol state machines live in kelvo-remote (D-104).
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use kelvo_engine::LiveHub;
use kelvo_proto::{
    Hello, Message, RemoteInterest, SourceStatus, SyncPage, SyncRequest, WireBucket, WireLayout,
};
use kelvo_remote::{
    ControlRequest, ControlResponse, EndpointConfig, MirrorSink, RemoteSource, ServerProvider,
    SocketPaths,
};
use kelvo_schema::lock::LockExt;
use kelvo_schema::{HostId, HostRecord, SyncKinds, Tier};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::error::CommandError;
use crate::history::History;
use crate::hosts::HostEntry;
use crate::live::LiveFeed;
use crate::state::AppState;

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
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct RemoteHostRecord {
    pub config: RemoteHostConfig,
    pub status: RemoteHostStatus,
}
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct RemoteHostPreparation {
    pub installed: bool,
    pub compatible: bool,
    pub host_id: Option<String>,
    pub name: String,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
pub struct RemoteSshHostTrust {
    pub token: String,
    pub alias: String,
    pub hostname: String,
    pub port: u16,
    pub fingerprints: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedHost {
    config: RemoteHostConfig,
    record: HostRecord,
    helper_path: String,
}
struct Mirror {
    history: Arc<History>,
    host: HostId,
    budget: u32,
    recent: Mutex<Vec<kelvo_store::BucketRow>>,
    epoch: Mutex<MirrorEpoch>,
}

#[derive(Default)]
struct MirrorEpoch {
    id: Option<Uuid>,
    /// Only current-epoch keys in the recent-rollup window are needed to reject a
    /// delayed transient snapshot after a durable page has superseded it.
    committed: Vec<(Tier, i64, Vec<kelvo_schema::SeriesKey>)>,
    newest_ms: Option<i64>,
}
impl MirrorEpoch {
    fn trim(&mut self, now: i64) {
        self.newest_ms = Some(self.newest_ms.map_or(now, |old| old.max(now)));
        let floor = self
            .newest_ms
            .unwrap_or(now)
            .saturating_sub(kelvo_engine::RECENT_ROWS_MS);
        self.committed.retain(|(_, ts, _)| *ts >= floor);
    }
}

/// Local history and every mirror have independent lifecycle and write failures.
#[derive(Default)]
pub struct RemoteManager {
    saved: Mutex<BTreeMap<HostId, SavedHost>>,
    setups: Mutex<BTreeMap<String, tokio::sync::watch::Sender<bool>>>,
    trust: Mutex<BTreeMap<String, (std::time::Instant, kelvo_remote::trust::TrustCandidate)>>,
    statuses: Mutex<BTreeMap<HostId, RemoteHostStatus>>,
    mirrors: Mutex<BTreeMap<HostId, Arc<Mirror>>>,
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
            .map_or_else(Vec::new, |m| {
                m.recent
                    .lock_ok()
                    .iter()
                    .filter(|r| r.bucket_ts >= from - 60_000 && r.bucket_ts < to)
                    .cloned()
                    .collect()
            })
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
            RemoteError::Identity(_) => Self::RemoteIdentity { message },
            RemoteError::Transport(_)
            | RemoteError::Io(_)
            | RemoteError::Codec(_)
            | RemoteError::Protocol(_) => Self::RemoteUnreachable { message },
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

impl MirrorSink for Mirror {
    fn cursor(
        &self,
        tier: Tier,
        epoch: Uuid,
        kinds: SyncKinds,
    ) -> Result<Option<kelvo_schema::Cursor>, String> {
        let mut current = self.epoch.lock_ok();
        if current.id != Some(epoch) {
            self.recent.lock_ok().clear();
            *current = MirrorEpoch {
                id: Some(epoch),
                ..Default::default()
            };
        }
        self.history
            .read(|r| r.resume_cursor(self.host, tier, epoch, kinds))
            .map_err(|e| e.to_string())
    }

    fn ingest(&self, page: SyncPage, kinds: SyncKinds) -> Result<(), String> {
        let mut current = self.epoch.lock_ok();
        if current.id != Some(page.epoch) {
            return Err("Obsolete history epoch".into());
        }
        let page = kelvo_remote::page_from_wire(page, kinds).map_err(|e| e.to_string())?;
        let written: Vec<_> = page
            .rows
            .iter()
            .map(|row| {
                let layout = page
                    .layouts
                    .iter()
                    .find(|layout| layout.layout_no == row.layout_no)
                    .ok_or_else(|| "Unknown durable history layout".to_string())?;
                Ok((page.tier, row.bucket_ts, layout.series.clone()))
            })
            .collect::<Result<_, String>>()?;
        self.history
            .writer()
            .map_err(|e| e.to_string())?
            .ingest_page_durable(self.host, page)
            .map_err(|e| e.to_string())?;
        // Record coverage only AFTER its rows and cursor are durably committed.
        for key in &written {
            if !current.committed.contains(key) {
                current.committed.push(key.clone());
            }
        }
        if let Some(newest) = written.iter().map(|(_, ts, _)| *ts).max() {
            current.trim(newest);
        }
        self.recent.lock_ok().retain(|r| {
            !written.iter().any(|(tier, ts, series)| {
                *tier == r.tier && *ts == r.bucket_ts && series.as_slice() == r.series.as_ref()
            })
        });
        Ok(())
    }

    fn recent(
        &self,
        epoch: Uuid,
        tier: Tier,
        complete_to_ms: i64,
        layouts: Vec<WireLayout>,
        rows: Vec<WireBucket>,
    ) -> Result<(), String> {
        let mut current = self.epoch.lock_ok();
        if current.id != Some(epoch) {
            return Err("Obsolete recent-history epoch".into());
        }
        if tier != Tier::S10 || rows.len() > 2000 || layouts.len() > 2000 {
            return Err("Unsupported or oversized recent-history snapshot".into());
        }
        let width = tier.bucket_ms().ok_or("Invalid recent tier")?;
        let floor = complete_to_ms.saturating_sub(kelvo_engine::RECENT_ROWS_MS);
        let mut out = Vec::new();
        for row in rows {
            let layout = layouts
                .iter()
                .find(|l| l.layout_no == row.layout_no)
                .ok_or("Unknown recent layout")?;
            if row.stats.len() != layout.series.len().saturating_mul(3)
                || row.bucket_ts < floor
                || row.bucket_ts > complete_to_ms
                || row.bucket_ts.rem_euclid(width) != 0
            {
                return Err("Invalid recent row".into());
            }
            if current.committed.iter().any(|(t, ts, series)| {
                *t == tier && *ts == row.bucket_ts && *series == layout.series
            }) {
                continue;
            }
            out.push(kelvo_store::BucketRow {
                host: self.host,
                tier,
                bucket_ts: row.bucket_ts,
                series: layout.series.clone().into(),
                stats: row.stats,
            });
        }
        // The snapshot may include the bucket beginning at complete_to_ms: it is the
        // open bucket. Exact committed keys win over that transient value.
        current.trim(complete_to_ms);
        *self.recent.lock_ok() = out;
        Ok(())
    }

    fn truncated(&self, tier: Tier, epoch: Uuid, earliest_ms: i64) -> Result<(), String> {
        let current = self.epoch.lock_ok();
        if current.id != Some(epoch) {
            return Err("Obsolete truncation epoch".into());
        }
        let writer = self.history.writer().map_err(|e| e.to_string())?;
        writer
            .record_truncation(self.host, tier, earliest_ms)
            .map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())
    }

    fn enforce_budget(&self, now_ms: i64) -> Result<(), String> {
        if self.history.size_on_disk().map_err(|e| e.to_string())?
            > u64::from(self.budget) * 1_000_000
        {
            self.prune(now_ms)?;
        }
        Ok(())
    }
    fn prune(&self, now_ms: i64) -> Result<(), String> {
        let _current = self.epoch.lock_ok();
        let retention = kelvo_store::Retention {
            max_bytes: u64::from(self.budget) * 1_000_000,
            ..kelvo_store::Retention::mirror_default()
        };
        let report = self
            .history
            .writer()
            .map_err(|e| e.to_string())?
            .prune_mirror(now_ms, retention)
            .map_err(|e| e.to_string())?;
        self.history
            .record_mirror_prune(&report, now_ms, &retention);
        if report.cap_trim.is_some_and(|trim| !trim.cap_met) {
            return Err("The mirror's fixed database overhead exceeds its storage budget".into());
        }
        Ok(())
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
    let (stop, rx) = tokio::sync::watch::channel(false);
    *state.remotes.stop.lock_ok() = Some(stop);
    let paths = SocketPaths::new(data_dir(app)?);
    let handle = app.clone();
    let control = Arc::new(move |request| control(&handle, request));
    tauri::async_runtime::spawn(async move {
        if let Err(e) = kelvo_remote::serve_control(paths.control, control, rx).await {
            tracing::error!("remote control socket: {e}");
        }
    });
    refresh_metrics(app);
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

pub fn refresh_metrics(app: &AppHandle) {
    let state = app.state::<AppState>();
    let enabled = state.settings.settings().remote_access.enabled;
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

fn register(app: &AppHandle, saved: SavedHost) -> Result<(), CommandError> {
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
    let mirror = Arc::new(Mirror {
        history: history.clone(),
        host: id,
        budget: saved.config.mirror_budget_mb,
        recent: Mutex::default(),
        epoch: Mutex::default(),
    });
    let generation = Uuid::new_v4();
    state.remotes.generations.lock_ok().insert(id, generation);
    let handle = app.clone();
    let cb = Arc::new(move |s: kelvo_remote::ConnectionStatus| {
        let state = handle.state::<AppState>();
        if state.remotes.generations.lock_ok().get(&id) != Some(&generation) {
            return;
        }
        state.remotes.statuses.lock_ok().insert(
            id,
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
            },
        );
    });
    let source = RemoteSource::new(
        saved.record.clone(),
        state.local,
        EndpointConfig {
            lan: saved.config.lan.clone(),
            tailscale: saved.config.tailscale.clone(),
            helper_path: saved.helper_path.clone(),
        },
        cb,
        Some(mirror.clone()),
    );
    source.set_controller_on_battery(state.host(state.local)?.hub().status().on_battery);
    state.remotes.sources.lock_ok().insert(id, source.clone());
    let entry = state.hosts.insert(saved.record.clone(), source);
    state.remotes.mirrors.lock_ok().insert(id, mirror);
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
            RemoteHostRecord {
                config: h.config.clone(),
                status,
            }
        })
        .collect()
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

async fn probe_routes(
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
fn most_actionable(errors: Vec<CommandError>) -> Option<CommandError> {
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
async fn setup_step<T>(
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
    mut config: RemoteHostConfig,
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
    let (alias, mut probe) = setup_step(
        &mut cancel,
        probe_routes(config.lan.clone(), config.tailscale.clone()),
    )
    .await?;
    let mut update = None;
    if install {
        let exe = std::env::current_exe().map_err(failure)?;
        let bundle = exe
            .ancestors()
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .ok_or_else(|| failure("Use a packaged Kelvo app to install another Mac"))?;
        if let Some(existing) = probe.app_path.as_deref() {
            let transaction = kelvo_remote::install::update_app(&alias, bundle, existing)
                .await
                .map_err(CommandError::from)?;
            probe = transaction.probe.clone();
            update = Some(transaction);
        } else {
            probe = kelvo_remote::install::install_app(&alias, bundle)
                .await
                .map_err(CommandError::from)?;
        }
    }
    let app_path = probe
        .app_path
        .ok_or_else(|| failure("Kelvo app was not found"))?;
    let helper = probe
        .helper_path
        .ok_or_else(|| failure("Install a Kelvo build with remote monitoring support"))?;
    let mut before = None;
    let result: Result<RemoteHostRecord, CommandError> = async {
        check_setup_cancel(&cancel)?;
        kelvo_remote::install::launch(&alias, &app_path)
            .await
            .map_err(CommandError::from)?;
        let snapshot = setup_step(&mut cancel, wait_for_control(&alias, &helper)).await?;
        let host = snapshot
            .host
            .as_ref()
            .ok_or_else(|| failure("Kelvo returned no machine identity"))?;
        if host.id == state.local {
            return Err(failure("This is this Mac"));
        }
        if state.remotes.saved.lock_ok().contains_key(&host.id) {
            return Err(failure("This Mac is already in your host list"));
        }
        let expected = host.id;
        before = Some(snapshot);
        let response = kelvo_remote::install::configure(
            &alias,
            &helper,
            &ControlRequest::Configure {
                allow_remote_monitoring: true,
                show_menu_bar_icon: Some(show_menu_bar),
                launch_at_login: Some(true),
            },
        )
        .await
        .map_err(CommandError::from)?;
        if !response.ok {
            return Err(failure(response.message));
        }
        check_setup_cancel(&cancel)?;
        let session = setup_step(&mut cancel, async {
            kelvo_remote::connect(
                &EndpointConfig {
                    lan: config.lan.clone(),
                    tailscale: config.tailscale.clone(),
                    helper_path: helper.clone(),
                },
                state.local,
                Some(expected),
            )
            .await
            .map_err(CommandError::from)
        })
        .await?;
        let mut record = HostRecord::from_identity(session.hello.host.clone(), false);
        session.close().await;
        check_setup_cancel(&cancel)?;
        config.id = record.id.to_string();
        record.display_name = config.name.clone();
        register(
            &app,
            SavedHost {
                config: config.clone(),
                record,
                helper_path: helper.clone(),
            },
        )?;
        if let Err(error) = save(&app) {
            let id = host_id(&config.id)?;
            discard_host_async(&app, id).await?;
            return Err(error);
        }
        let id = host_id(&config.id)?;
        let status = state
            .remotes
            .statuses
            .lock_ok()
            .get(&id)
            .cloned()
            .unwrap_or_default();
        Ok(RemoteHostRecord {
            config: config.clone(),
            status,
        })
    }
    .await;
    match result {
        Ok(record) => {
            if let Some(update) = &update
                && let Err(error) = kelvo_remote::install::finish_update(&alias, update).await
            {
                tracing::warn!(
                    "remote update succeeded; backup retained at {}: {error}",
                    update.backup_path
                );
            }
            Ok(record)
        }
        Err(error) => {
            let mut recovery = Vec::new();
            if let Some(snapshot) = before
                && let Some(enabled) = snapshot.allow_remote_monitoring
            {
                match kelvo_remote::install::configure(
                    &alias,
                    &helper,
                    &ControlRequest::Configure {
                        allow_remote_monitoring: enabled,
                        show_menu_bar_icon: snapshot.show_menu_bar_icon,
                        launch_at_login: snapshot.launch_at_login,
                    },
                )
                .await
                {
                    Ok(response) if response.ok => {}
                    other => {
                        tracing::warn!("remote settings restoration failed: {other:?}");
                        recovery.push("Kelvo could not restore the previous settings on the other Mac. Check its settings and try again.".into())
                    }
                }
            }
            if let Some(update) = &update
                && let Err(rollback) = kelvo_remote::install::rollback_update(&alias, update).await
            {
                recovery.push(format!("The previous app and its data remain backed up at {} and {}. The new app/data are retained. Quit Kelvo on the other Mac before restoring the matched app/data backup: {rollback}",update.backup_path,update.data_backup_path));
            }
            if recovery.is_empty() {
                Err(error)
            } else {
                Err(failure(format!("{error}. {}", recovery.join(" "))))
            }
        }
    }
}

async fn discard_host_async(app: &AppHandle, id: HostId) -> Result<(), CommandError> {
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

#[tauri::command]
#[specta::specta]
pub async fn update_remote_host(
    app: AppHandle,
    config: RemoteHostConfig,
) -> Result<RemoteHostRecord, CommandError> {
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
    Ok(RemoteHostRecord { config, status })
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
        let (alias, _) =
            probe_routes(saved.config.lan.clone(), saved.config.tailscale.clone()).await?;
        configure_stop_sharing(|request| async move {
            kelvo_remote::install::configure(&alias, &saved.helper_path, &request)
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

async fn configure_stop_sharing<
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
async fn finish_removal(
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

#[tauri::command]
#[specta::specta]
pub async fn remove_remote_host(
    app: AppHandle,
    id: String,
    delete_mirror: bool,
    stop_sharing: bool,
) -> Result<Option<String>, CommandError> {
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
    let warning = finish_removal(
        stop_sharing,
        stop_remote_sharing(&previous),
        discard_host_async(&app, id),
    )
    .await?;
    state.emit_hosts_changed(&app);
    if delete_mirror {
        let dir = data_dir(&app)?.join("mirrors").join(id.to_string());
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(failure)?;
        }
    }
    Ok(warning)
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;

#[cfg(test)]
mod command_regressions {
    use super::*;

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

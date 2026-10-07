//! Every v1.0 command in v1-local-monitor.md 6.3. Per-host commands take `host: HostId`
//! even though v1 has one host (architecture.md infra 2).
//!
//! Commands that touch SQLite are `async` and run the query on a blocking thread, so they
//! never hold the main thread or a runtime worker.
//!
//! Not here: `query_events` is v1.2.

use std::sync::Arc;

use kelvo_schema::{Capabilities, Event, HostId, HostRecord, Module, SeriesSelector};
use tauri::{AppHandle, Manager, State};

use crate::error::CommandError;
use crate::ipc::{
    BatteryHour, ByteCount, EnergyByApp, ExportOutcome, ExportRequest, HeatmapDay, HeatmapRequest,
    HistoryGrowth, HistoryHealth, HistoryPage, HistoryRequest, LiveMsg, Millis, NetworkAddresses,
    NetworkByApp, NetworkTotals, ProcessView, ProcessesAt, SensorDump, SensorReading,
    SettingsSnapshot, SubscriptionInfo, UpdateStatus, WindowAppearance,
};
use crate::live::{DEFAULT_BACKFILL_MS, LiveFeed, LiveRequest, LiveSink};
use crate::process_signal::{
    Micros, ProcessSignalError, SignalKind, SignalRefusal, SystemProcessOs,
};
use crate::settings::SettingsPatch;
use crate::state::AppState;

/// Runs `f` on a blocking thread with the app state.
async fn blocking<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&AppState) -> Result<T, CommandError> + Send + 'static,
) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>()))
        .await
        .map_err(|e| CommandError::Internal {
            message: format!("command task failed: {e}"),
        })?
}

/// Every host, the local one first.
#[tauri::command]
#[specta::specta]
pub fn list_hosts(state: State<'_, AppState>) -> Vec<HostRecord> {
    state.hosts.all().iter().map(|h| h.record()).collect()
}

#[tauri::command]
#[specta::specta]
pub fn get_host(state: State<'_, AppState>, host: HostId) -> Result<HostRecord, CommandError> {
    Ok(state.host(host)?.record())
}

#[tauri::command]
#[specta::specta]
pub fn get_capabilities(
    state: State<'_, AppState>,
    host: HostId,
) -> Result<Capabilities, CommandError> {
    Ok(state.host(host)?.capabilities())
}

/// Starts this window's live channel for `host`, replacing an earlier one (a page
/// reload). `backfill_ms` is how much ring history to send (default 60 s, at most one
/// hour): the last two minutes before this returns, older rows afterwards as
/// `backfill_earlier` chunks. `series` limits the channel to the matching series (default
/// all); `min_period_ms` sends at most one frame per period (default every tick). See
/// `LiveMsg` for the message order. Async, so the backfill is serialized on a runtime
/// worker rather than the main thread.
#[tauri::command]
#[specta::specta]
pub async fn subscribe_live(
    window: tauri::Window,
    state: State<'_, AppState>,
    host: HostId,
    channel: tauri::ipc::Channel<LiveMsg>,
    backfill_ms: Option<u32>,
    series: Option<Vec<SeriesSelector>>,
    min_period_ms: Option<u32>,
) -> Result<SubscriptionInfo, CommandError> {
    let entry = state.host(host)?;
    let feed: Arc<dyn LiveFeed> = entry;
    let sink: Arc<dyn LiveSink> = Arc::new(channel);
    let req = LiveRequest {
        backfill_ms: backfill_ms.map_or(DEFAULT_BACKFILL_MS, i64::from),
        series,
        min_period_ms: min_period_ms.unwrap_or(0),
    };
    Ok(state.live.subscribe(window.label(), feed, sink, req))
}

/// Whether this window shows process data for `host`, and which rows (`view`; default
/// every readable process at every sample). `stream` is `SubscriptionInfo.stream` from
/// this page's `subscribe_live`: the interest then ends when the page reloads. It counts
/// only while the window is visible, and is dropped when it closes. A new call replaces
/// the window's view, so a window with several process consumers sends their union.
#[tauri::command]
#[specta::specta]
pub fn set_process_interest(
    window: tauri::Window,
    state: State<'_, AppState>,
    host: HostId,
    interested: bool,
    view: Option<ProcessView>,
    stream: Option<u32>,
) -> Result<(), CommandError> {
    let feed: Arc<dyn LiveFeed> = state.host(host)?;
    let view = interested.then(|| view.unwrap_or_default());
    state
        .live
        .set_process_interest(window.label(), feed, view, stream);
    Ok(())
}

/// Per-series points from one tier, through now: the store's rows and the engine's
/// rows it has not committed yet, its open buckets included (D-092). Each series says
/// how far apart its points can be and still be one line (`hold_ms`). With history
/// unavailable, the engine's rows of the last 15 minutes alone (`HISTORY_RECENT_MS`).
#[tauri::command]
#[specta::specta]
pub async fn query_history(
    app: AppHandle,
    request: HistoryRequest,
) -> Result<HistoryPage, CommandError> {
    blocking(app, move |state| {
        let entry = state.host(request.host)?;
        let (host, from_ms, to_ms) = (request.host, recent_from(request.from_ms), request.to_ms);
        state.history.history(
            &request,
            || entry.recent_rows(host, from_ms, to_ms),
            |key, bucket_ms| entry.history_hold_ms(key, bucket_ms, request.from_ms),
        )
    })
    .await
}

/// The hourly battery bars: for each local hour between consecutive `hour_starts_ms`
/// (UTC ms, DST applied, as `query_heatmap` takes them), the charge in its last minute
/// and whether it charged, through now. `invalid_argument` when there are fewer than 2
/// boundaries, more than one per hour of 92 days, or they go backwards.
#[tauri::command]
#[specta::specta]
pub async fn battery_hours(
    app: AppHandle,
    host: HostId,
    hour_starts_ms: Vec<Millis>,
) -> Result<Vec<BatteryHour>, CommandError> {
    blocking(app, move |state| {
        let entry = state.host(host)?;
        let starts: Vec<i64> = hour_starts_ms.iter().map(|m| m.0).collect();
        let from_ms = recent_from(starts.first().copied().unwrap_or(0));
        let to_ms = starts.last().copied().unwrap_or(0);
        state
            .history
            .battery_hours(host, &starts, || entry.recent_rows(host, from_ms, to_ms))
    })
    .await
}

/// Where the engine's recent rows a read starting at `from_ms` needs begin: the read
/// widens to the bucket holding `from_ms`, up to a 15-minute one.
fn recent_from(from_ms: i64) -> i64 {
    from_ms.saturating_sub(kelvo_schema::Tier::M15.bucket_ms().unwrap_or(0))
}

/// Events (detector findings and fired alerts) with `from_ms <= ts < to_ms`, oldest
/// first. New ones also arrive as the `event-recorded` event.
#[tauri::command]
#[specta::specta]
pub async fn query_events(
    app: AppHandle,
    host: HostId,
    from_ms: Millis,
    to_ms: Millis,
) -> Result<Vec<Event>, CommandError> {
    blocking(app, move |state| {
        state.host(host)?;
        state.history.read(|r| r.events(host, from_ms.0, to_ms.0))
    })
    .await
}

/// The stored processes nearest `t_ms`: the snapshot (10 s apart, 30 s in Performance
/// mode) within the last 72 hours, the per-minute top 5 before that. `null` when nothing
/// was stored near it.
#[tauri::command]
#[specta::specta]
pub async fn query_processes_at(
    app: AppHandle,
    host: HostId,
    t_ms: Millis,
) -> Result<Option<ProcessesAt>, CommandError> {
    blocking(app, move |state| {
        state.host(host)?;
        Ok(state
            .history
            .read(|r| r.processes_at(host, t_ms.0))?
            .map(Into::into))
    })
    .await
}

/// Which apps moved the interface's bytes over `[from_ms, to_ms)` (D-089), widened to
/// whole buckets: the store's tiers for older ranges, the engine's in-memory buckets for
/// the last hour (the store commits every 5 minutes, and the open bucket is never
/// stored); the ring alone when the store is unavailable. `complete_to_ms` says where the
/// engine's buckets stop being final. `remote_host` for a host other than this Mac, since
/// per-app bytes are not synced; `invalid_argument` when `to_ms` is before `from_ms` or
/// the range is longer than the longest history retention.
#[tauri::command]
#[specta::specta]
pub async fn query_network_by_app(
    app: AppHandle,
    host: HostId,
    from_ms: Millis,
    to_ms: Millis,
) -> Result<NetworkByApp, CommandError> {
    blocking(app, move |state| {
        let entry = state.host(host)?;
        if !entry.record().is_local {
            return Err(CommandError::RemoteHost { host });
        }
        state.history.network_by_app(host, from_ms.0, to_ms.0, || {
            entry.recent_net(from_ms.0, to_ms.0)
        })
    })
    .await
}

/// The bytes the reported interfaces moved over `[from_ms, to_ms)`, widened to whole
/// buckets and cut at now, from the `net.rx_total` and `net.tx_total` rollups through now.
/// Needs neither per-app network history nor NetworkStatistics, so it answers in every
/// edition; with history unavailable, the engine's last 15 minutes alone.
/// `invalid_argument` when `to_ms` is before `from_ms` or the range is longer than the
/// longest history retention.
#[tauri::command]
#[specta::specta]
pub async fn query_network_totals(
    app: AppHandle,
    host: HostId,
    from_ms: Millis,
    to_ms: Millis,
) -> Result<NetworkTotals, CommandError> {
    blocking(app, move |state| {
        let entry = state.host(host)?;
        let now_ms = kelvo_engine::wall_ms();
        let recent_start = recent_from(from_ms.0);
        state
            .history
            .network_totals(host, from_ms.0, to_ms.0, now_ms, || {
                entry.recent_rows(host, recent_start, to_ms.0)
            })
    })
    .await
}

/// Which apps used energy over `[from_ms, to_ms)` (D-093), widened to whole 10 s
/// buckets, from the last hour of process samples the host's hub keeps in memory: a
/// range reaching further back is answered for the part inside that hour, and
/// `since_ms` says where counting started. `remote_host` for a host other than this Mac;
/// `invalid_argument` when `to_ms` is before `from_ms`.
#[tauri::command]
#[specta::specta]
pub async fn query_energy_by_app(
    app: AppHandle,
    host: HostId,
    from_ms: Millis,
    to_ms: Millis,
) -> Result<EnergyByApp, CommandError> {
    if to_ms.0 < from_ms.0 {
        return Err(CommandError::InvalidArgument {
            message: format!("to_ms {} is before from_ms {}", to_ms.0, from_ms.0),
        });
    }
    blocking(app, move |state| {
        let entry = state.host(host)?;
        if !entry.record().is_local {
            return Err(CommandError::RemoteHost { host });
        }
        let me = kelvo_engine::process_control::ProcessOs::self_pid(&SystemProcessOs);
        let own = kelvo_engine::process_control::own_processes();
        Ok(crate::energy::energy_by_app(
            entry.energy_by_app(from_ms.0, to_ms.0),
            |pid, start, name| SignalRefusal::of(pid, name, me, own.contains(pid, start)),
        ))
    })
    .await
}

/// The addresses of the interface carrying the default route. Read from the system on
/// each call; no network request. `remote_host` for a host other than this Mac.
#[tauri::command]
#[specta::specta]
pub fn get_network_addresses(
    state: State<'_, AppState>,
    host: HostId,
) -> Result<NetworkAddresses, CommandError> {
    let entry = state.host(host)?;
    if !entry.record().is_local {
        return Err(CommandError::RemoteHost { host });
    }
    Ok(crate::addresses::network_addresses(
        entry.primary_iface().as_deref(),
    ))
}

/// This Mac's public address, from an outside service over HTTPS (D-093). The one
/// request Kelvo makes on its own; only the Network page asks, and only while shown.
#[tauri::command]
#[specta::specta]
pub async fn get_public_ip() -> Result<String, CommandError> {
    tauri::async_runtime::spawn_blocking(crate::addresses::public_ip)
        .await
        .map_err(|e| CommandError::Internal {
            message: format!("command task failed: {e}"),
        })?
        .map(|ip| ip.to_string())
        .map_err(|message| CommandError::PublicIp { message })
}

/// Hourly averages of `cpu.total` or `thermal.hottest` for the requested local days,
/// from the 15-minute and 1-minute tiers. The frontend sends each day's local-hour
/// boundaries in UTC (DST applied), so Rust needs no time zone. `invalid_argument` when a
/// day does not have 25 non-decreasing boundaries or the days overlap.
#[tauri::command]
#[specta::specta]
pub async fn query_heatmap(
    app: AppHandle,
    request: HeatmapRequest,
) -> Result<Vec<HeatmapDay>, CommandError> {
    blocking(app, move |state| {
        state.host(request.host)?;
        let cells = request.cells()?;
        let series = request.metric.series();
        let hours = state
            .history
            .read(|r| r.heatmap(request.host, &series, &cells))?;
        Ok(request.days(&hours))
    })
    .await
}

/// Asks where to save with a save dialog (a sheet on the calling window), then streams
/// the range there as CSV: one row per bucket of the chosen tier, gaps as rows of their
/// own. `cancelled` when the dialog is dismissed; `export` when the file cannot be
/// written, with no partial file left.
#[tauri::command]
#[specta::specta]
pub async fn export_csv(
    app: AppHandle,
    window: tauri::Window,
    request: ExportRequest,
) -> Result<ExportOutcome, CommandError> {
    use tauri_plugin_dialog::DialogExt;

    let dialogs = app.clone();
    blocking(app, move |state| {
        state.host(request.host)?;
        let query = request.to_query()?;
        // No dialog when there is nothing to export from.
        state.history.writer()?;
        let picked = dialogs
            .dialog()
            .file()
            .set_parent(&window)
            .set_file_name(crate::export::file_name(request.file_name.as_deref()))
            .add_filter("CSV", &["csv"])
            .blocking_save_file();
        let Some(picked) = picked else {
            return Ok(ExportOutcome::Cancelled);
        };
        let path = picked.into_path().map_err(|e| CommandError::Export {
            message: format!("the chosen location is not a file path: {e}"),
        })?;
        crate::export::write_csv(&state.history, &query, &path)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub fn get_settings(state: State<'_, AppState>) -> SettingsSnapshot {
    state.settings.get()
}

/// Applies `patch`, validates, saves and emits `settings-changed`. Engine-affecting
/// changes (interval, battery slowdown, module switches) reach the engine before the
/// event. On error nothing changed.
#[tauri::command]
#[specta::specta]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> Result<SettingsSnapshot, CommandError> {
    state.update_settings(&app, &patch)
}

/// Bytes the history database takes on disk, including its WAL. The store holds every
/// host, so this is the file's size whichever host is asked about.
#[tauri::command]
#[specta::specta]
pub async fn history_size(app: AppHandle, host: HostId) -> Result<ByteCount, CommandError> {
    blocking(app, move |state| {
        state.host(host)?;
        Ok(ByteCount(state.history.size_on_disk()?))
    })
    .await
}

/// What history costs per day on this machine, measured from the database, or `None`
/// until an hour has been recorded in each retention window. The store holds every host,
/// so like `history_size` this is the whole file whichever host is asked about. Reads every page of the file, so the Settings page
/// asks once when it opens.
#[tauri::command]
#[specta::specta]
pub async fn history_growth(
    app: AppHandle,
    host: HostId,
) -> Result<Option<HistoryGrowth>, CommandError> {
    blocking(app, move |state| {
        state.host(host)?;
        let now = kelvo_engine::wall_ms();
        let growth = state
            .history
            .read(|r| r.history_growth(now, &kelvo_store::Retention::default()))?;
        Ok(growth.map(HistoryGrowth::from))
    })
    .await
}

/// Deletes the host's history and vacuums. Returns the size on disk afterwards.
#[tauri::command]
#[specta::specta]
pub async fn clear_history(app: AppHandle, host: HostId) -> Result<ByteCount, CommandError> {
    let app_handle = app.clone();
    blocking(app, move |state| {
        let entry = state.host(host)?;
        let writer = state.history.writer()?;
        // The engine's rows not committed yet and its open buckets go too: before the
        // store clears the host, so a bucket closing meanwhile holds only what came after
        // (the clear deletes it either way), and again after, for what it emitted then.
        entry.forget_recent_rows();
        writer.clear_host(host, kelvo_engine::wall_ms())?;
        entry.forget_recent_rows();
        tracing::info!(%host, "history cleared");
        if let Some(health) = state.history.clear_trim() {
            state.emit_history_health(&app_handle, health);
        }
        Ok(ByteCount(state.history.size_on_disk()?))
    })
    .await
}

/// The low-disk pause and the size-limit trim (D-057, D-059). The store is shared, so
/// this is the same for every host; `history_unavailable` when there is no store.
/// `history-health-changed` carries later changes.
#[tauri::command]
#[specta::specta]
pub async fn history_health(app: AppHandle, host: HostId) -> Result<HistoryHealth, CommandError> {
    // Async like the other store commands: the availability check takes the store lock,
    // which `reset_history` holds while it closes, moves and reopens the file. A sync
    // command would wait for it on the main thread, freezing the tray and every window.
    blocking(app, move |state| {
        state.host(host)?;
        state.history.writer()?;
        Ok(state.history.health())
    })
    .await
}

/// Moves the history database aside (it stays next to the new one as
/// `history-reset-<ms>.sqlite`, for diagnosis) and starts an empty one: the way out when
/// history is unavailable because the file is corrupt or from a newer Kelvo. Sampling
/// continues throughout. `store_busy` when another Kelvo process has the database open.
/// Returns the new size on disk; `history-health-changed` follows.
#[tauri::command]
#[specta::specta]
pub async fn reset_history(app: AppHandle) -> Result<ByteCount, CommandError> {
    let app_handle = app.clone();
    blocking(app, move |state| {
        state.reset_history(&app_handle)?;
        Ok(ByteCount(state.history.size_on_disk()?))
    })
    .await
}

/// Pauses or resumes sampling on this Mac. Pause writes a `paused` gap.
#[tauri::command]
#[specta::specta]
pub fn set_paused(state: State<'_, AppState>, paused: bool) {
    // The local host is registered for the app's lifetime; this cannot miss.
    if let Err(e) = state.set_paused(state.local, paused) {
        tracing::warn!("pausing: {e}");
    }
}

/// Creates or focuses the dashboard window.
#[tauri::command]
#[specta::specta]
pub fn open_dashboard(app: AppHandle, route: Option<String>) -> Result<(), CommandError> {
    crate::windows::open_dashboard(&app, route.as_deref())
}

/// Open-latency probe: the popover page reports its first painted frame after a show.
/// Rust injects the call itself (`popover::toggle`); pages never need to call it.
#[tauri::command]
#[specta::specta]
pub fn report_popover_paint(app: AppHandle, token: u32) {
    if let Some(p) = app.try_state::<crate::popover::Popover>() {
        p.painted(token);
    }
}

/// What the unknown-chip notice shares: the chip and OS, capabilities, and every Power
/// and Sensors series with its latest value. No serials, user, host or network names.
#[tauri::command]
#[specta::specta]
pub fn sensor_dump(state: State<'_, AppState>, host: HostId) -> Result<SensorDump, CommandError> {
    let entry = state.host(host)?;
    let record = entry.record();
    let catalog = kelvo_schema::Catalog::builtin();
    let sensors = entry
        .latest_frame()
        .map(|f| {
            f.layout
                .series
                .iter()
                .zip(f.held.iter())
                .filter(|(k, _)| {
                    catalog
                        .get(k.metric.as_str())
                        .is_some_and(|d| matches!(d.module, Module::Power | Module::Sensors))
                })
                .map(|(k, &v)| SensorReading {
                    key: k.clone(),
                    value: v.is_finite().then_some(v),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(SensorDump {
        model: record.info.model,
        chip: record.info.chip,
        os_version: record.info.os_version,
        chip_known: record.info.chip_known,
        capabilities: entry.capabilities(),
        sensors,
    })
}

/// Quit or Force Quit one local process (D-029). `start_time_us` is the start time of the
/// row the user picked; a different process on that PID now answers `pid_reused` and is
/// not signalled. The guardrails are in `process_signal::signal_process`.
#[tauri::command]
#[specta::specta]
pub fn process_signal(
    state: State<'_, AppState>,
    host: HostId,
    pid: i32,
    start_time_us: Micros,
    kind: SignalKind,
) -> Result<(), ProcessSignalError> {
    if state.host(host).is_err() {
        return Err(ProcessSignalError::UnknownHost { host });
    }
    if host != state.local {
        return Err(ProcessSignalError::RemoteHost { host });
    }
    let result =
        crate::process_signal::signal_process(&SystemProcessOs, pid, start_time_us.0, kind);
    match &result {
        Ok(()) => tracing::info!(pid, ?kind, "process signalled"),
        Err(e) => tracing::info!(pid, ?kind, error = %e, "process not signalled"),
    }
    result
}

/// What this build can do (D-065). Fixed for the process; the UI reads it once.
#[tauri::command]
#[specta::specta]
pub fn get_edition() -> crate::edition::Edition {
    crate::edition::Edition::current()
}

/// The updater arrives in phase 6. Until then this reports the setting, and never makes
/// a network request.
#[tauri::command]
#[specta::specta]
pub fn check_for_updates(state: State<'_, AppState>) -> UpdateStatus {
    if state.settings.settings().general.check_updates {
        UpdateStatus::NotConfigured
    } else {
        UpdateStatus::Disabled
    }
}

/// What the calling window's root reflects: Performance mode, Reduce Transparency, theme.
/// `window-appearance-changed` carries later changes.
#[tauri::command]
#[specta::specta]
pub fn get_window_appearance(state: State<'_, AppState>) -> WindowAppearance {
    state.appearance()
}

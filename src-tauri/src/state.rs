//! `AppState`: everything the shell owns, created once at launch (architecture.md, App
//! shell, Startup) and managed by Tauri.
//!
//! Startup order: create the data directory and open the history store (or go live-only:
//! neither stops the launch), read or create the host id (kept in memory if it cannot be
//! saved), build the local `HostRecord`, load settings, set the activation policy, register the
//! local host with its `LocalSource`, write the host into the store (a failure there
//! makes history unavailable, never stops the launch), start watching its bus, start the
//! source, schedule pruning.
//!
//! # For the tray and window code
//!
//! - Live data: `state.hosts.get(state.local)?.bus().subscribe()` gives every layout,
//!   frame, capabilities change, process batch and status. Build displays from
//!   `LiveFrame::held` (or `LiveFrame::snapshot`), never from `values`.
//! - Window lifecycle: call `state.live.window_visible(label, bool)` on show, hide,
//!   minimize, unminimize and occlusion changes. A window counts as hidden until it is
//!   reported visible, so report `true` on every show. `WindowEvent::Destroyed` already
//!   calls `window_closed`.
//! - Pause: `state.set_paused(host, bool)` (one host; the tray and the `set_paused`
//!   command pass `state.local`); current status from `HostEntry::status()` through the
//!   `LiveFeed` trait (the hub's latest status).
//! - Engine settings (interval, battery slowdown, module switches, what the menu bar
//!   shows) apply to the local
//!   host only: they describe this Mac's sampler. Remote hosts (v4) get their own.
//! - Settings: `state.settings.get()`; listen for `SettingsChanged` like any window.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Context;
use kelvo_engine::{BusMsg, EngineParts, HousekeepingHandle, LocalSource, identity, retention_for};
use kelvo_schema::settings::Appearance;
use kelvo_schema::{
    HostId, HostRecord, Labels, Module, ModuleCap, PerformanceReason, SeriesSelector, Settings,
    Tier, UnsupportedReason,
};
use kelvo_store::{HistoryQuery, TierChoice};
use tauri::{AppHandle, Manager, Theme};
use tauri_specta::Event;

use crate::calibration::FileScaleStore;
use crate::error::CommandError;
use crate::history::History;
use crate::hosts::{HostEntry, HostRegistry};
use crate::ipc::{
    CapabilitiesChanged, EventRecorded, HistoryHealth, HistoryHealthChanged, HostsChanged,
    SettingsChanged, SettingsSnapshot, WindowAppearance, WindowAppearanceChanged,
};
use crate::live::LiveRegistry;
use crate::notify;
use crate::platform;
use crate::settings::{
    PluginStoreFile, SettingsEffects, SettingsOwner, alerts_switched_on, engine_affecting,
    network_history_affecting, prune_affecting,
};

pub struct AppState {
    /// The local host's id.
    pub local: HostId,
    pub hosts: HostRegistry,
    pub live: LiveRegistry,
    pub settings: SettingsOwner,
    pub history: History,
    appearance: Mutex<WindowAppearance>,
    /// History housekeeping; woken early when the retention or size limit changes.
    housekeeping: HousekeepingHandle,
}

impl AppState {
    /// Builds the state and starts the local source. Runs in Tauri's `setup`, on the main
    /// thread.
    pub fn start(app: &AppHandle) -> anyhow::Result<()> {
        let data_dir = app
            .path()
            .app_data_dir()
            .context("resolving the app data directory")?;
        let (history, id) = open_data_dir(&data_dir, identity::machine_binding);
        let local = id.id;
        let record = platform::local_host_record(local);
        tracing::info!(
            host = %local,
            id_source = ?id.source,
            id_persisted = id.persisted,
            name = %record.display_name,
            chip = ?record.info.chip,
            os = %record.info.os_version,
            "local host"
        );

        let settings = SettingsOwner::load(crate::bench::settings_file(Box::new(
            PluginStoreFile::open(app)?,
        )));
        // The benchmark's overrides (D-088) reach the engine and the first appearance
        // only, never the owner, so no later settings write can save them.
        let mut current = settings.settings();
        if let Some(on) = crate::bench::performance_override() {
            current.sampling.performance_mode = on;
        }
        let force_battery = crate::bench::force_battery();
        if force_battery {
            current.sampling.slow_on_battery = true;
        }
        apply_activation_policy(app, current.general.show_in_dock);
        apply_theme(app, current.general.appearance);
        if current.onboarding.completed {
            reconcile_login_item(current.general.launch_at_login);
        }

        let mut parts = EngineParts::platform(Arc::new(FileScaleStore::open(&data_dir)));
        if force_battery {
            parts.power = Box::new(crate::bench::ForceBattery(parts.power));
        }
        let source = Arc::new(LocalSource::new(record.clone(), parts, current.clone()));
        let handle = app.clone();
        let emitter = app.clone();
        let housekeeping = history
            .spawn_pruner(
                move || {
                    handle.try_state::<AppState>().map_or_else(
                        || retention_for(&Settings::default().history),
                        |s| retention_for(&s.settings.settings().history),
                    )
                },
                move |health| {
                    if let Some(s) = emitter.try_state::<AppState>() {
                        s.emit_history_health(&emitter, health);
                    }
                },
            )
            .context("starting history housekeeping")?;
        let state = AppState {
            local,
            hosts: HostRegistry::new(),
            live: LiveRegistry::new(),
            settings,
            history,
            appearance: Mutex::new(WindowAppearance {
                // Seeded from the saved setting, so windows open without motion when the
                // user turned the mode on (D-088). Low Power Mode arrives with the
                // engine's first status, which the host watcher below applies.
                performance: PerformanceReason::resolve(current.sampling.performance_mode, false),
                reduce_transparency: platform::reduce_transparency(),
                theme: current.general.appearance,
            }),
            housekeeping,
        };
        let entry = state.hosts.insert(record, source.clone());
        // Subscribe before starting, so the watcher sees the first capabilities.
        spawn_host_watcher(app.clone(), Arc::clone(&entry));
        // A store that cannot take the host goes unavailable here and the source runs
        // live-only; the banner explains (D-064).
        let writer = state.history.register_host(&entry.record());
        source.seed_alert_history(recent_alerts(
            &state.history,
            local,
            kelvo_engine::wall_ms(),
        ));
        // Charts opened right after launch draw the hour before it from the store.
        warm_live_ring(&state.history, &entry, local, kelvo_engine::wall_ms());
        // Managed before the source starts, so the watcher always finds the state.
        app.manage(state);
        entry.start(writer).context("starting the local source")?;

        let handle = app.clone();
        platform::observe_reduce_transparency(move |reduce| {
            if let Some(state) = handle.try_state::<AppState>() {
                state.update_appearance(&handle, |a| a.reduce_transparency = reduce);
            }
        });
        Ok(())
    }

    /// Stops every source (flushing buckets, closing gaps), then closes the store.
    pub fn shutdown(&self) {
        self.hosts.stop_all();
        self.history.close();
        tracing::info!("shut down");
    }

    pub fn host(&self, host: HostId) -> Result<Arc<HostEntry>, CommandError> {
        self.hosts.get(host)
    }

    /// Pauses or resumes sampling on one host. Pause writes a `paused` gap.
    pub fn set_paused(&self, host: HostId, paused: bool) -> Result<(), CommandError> {
        self.host(host)?.set_paused(paused);
        tracing::info!(%host, paused, "sampling paused state");
        Ok(())
    }

    /// Moves the history database aside and starts a fresh one (`reset_history`). Every
    /// source is detached first (flushing and closing its gaps in the old file) and
    /// reattached to the new store with its host registered. Blocking.
    pub fn reset_history(&self, app: &AppHandle) -> Result<(), CommandError> {
        let hosts = self.hosts.all();
        for h in &hosts {
            h.set_store(None);
        }
        let health = self
            .history
            .reset_and_attach(kelvo_engine::wall_ms(), |history| {
                for h in &hosts {
                    h.set_store(history.register_host(&h.record()));
                }
            })?;
        // Only on success: the event carries health, and over a failed reset it would
        // tell windows history is back.
        self.emit_history_health(app, health);
        tracing::warn!("history reset");
        Ok(())
    }

    /// Emits `hosts-changed` with every host, local first.
    pub fn emit_hosts_changed(&self, app: &AppHandle) {
        let hosts: Vec<HostRecord> = self.hosts.all().iter().map(|h| h.record()).collect();
        if let Err(e) = (HostsChanged { hosts }).emit(app) {
            tracing::warn!("emitting hosts-changed: {e}");
        }
    }

    fn appearance_lock(&self) -> MutexGuard<'_, WindowAppearance> {
        self.appearance.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn appearance(&self) -> WindowAppearance {
        *self.appearance_lock()
    }

    /// Changes the appearance and emits `window-appearance-changed` if it changed.
    pub fn update_appearance(&self, app: &AppHandle, f: impl FnOnce(&mut WindowAppearance)) {
        let mut a = self.appearance_lock();
        let before = *a;
        f(&mut a);
        if *a != before {
            let appearance = *a;
            drop(a);
            if let Err(e) = (WindowAppearanceChanged { appearance }).emit(app) {
                tracing::warn!("emitting window-appearance-changed: {e}");
            }
        }
    }

    /// Emits `history-health-changed`.
    pub fn emit_history_health(&self, app: &AppHandle, health: HistoryHealth) {
        if let Err(e) = (HistoryHealthChanged { health }).emit(app) {
            tracing::warn!("emitting history-health-changed: {e}");
        }
    }

    /// Applies a settings patch with the shell's side effects.
    pub fn update_settings(
        &self,
        app: &AppHandle,
        patch: &crate::settings::SettingsPatch,
    ) -> Result<SettingsSnapshot, CommandError> {
        self.settings
            .update(patch, &ShellEffects { app, state: self })
    }
}

struct ShellEffects<'a> {
    app: &'a AppHandle,
    state: &'a AppState,
}

impl SettingsEffects for ShellEffects<'_> {
    fn prepare(&self, old: &Settings, new: &Settings) -> Result<(), CommandError> {
        if old.general.launch_at_login != new.general.launch_at_login {
            platform::set_launch_at_login(new.general.launch_at_login)
                .map_err(|message| CommandError::LaunchAtLogin { message })?;
        }
        Ok(())
    }

    fn committed(&self, old: &Settings, now: &SettingsSnapshot) {
        let new = &now.settings;
        if engine_affecting(old, new) {
            // Queued on the local engine's inbox ahead of anything sent later; the engine
            // applies it before its next tick. The sampling settings are this Mac's;
            // remote hosts (v4) are configured per host.
            for h in self.state.hosts.all() {
                if h.record().is_local {
                    h.apply_settings(new);
                }
            }
        }
        if network_history_affecting(old, new) {
            // Per-app network bytes are recorded by this Mac's engine only (D-089).
            let on = kelvo_engine::network_history_enabled(new);
            for h in self.state.hosts.all() {
                if h.record().is_local {
                    h.set_network_history(on);
                }
            }
        }
        if old.general.show_in_dock != new.general.show_in_dock {
            apply_activation_policy(self.app, new.general.show_in_dock);
        }
        if old.general.appearance != new.general.appearance {
            apply_theme(self.app, new.general.appearance);
            self.state
                .update_appearance(self.app, |a| a.theme = new.general.appearance);
        }
        if prune_affecting(old, new) {
            self.state.housekeeping.prune_now();
        }
        let switched_on = alerts_switched_on(old, new);
        if !switched_on.is_empty() {
            notify::request_permission(self.app);
            notify::alerts_on(self.app, &switched_on);
        }
        tracing::info!(revision = now.revision, "settings changed");
        let event = SettingsChanged {
            revision: now.revision,
            settings: new.clone(),
        };
        if let Err(e) = event.emit(self.app) {
            tracing::warn!("emitting settings-changed: {e}");
        }
    }
}

/// The alert events of the last [`kelvo_engine::detect::alert_history_ms`], which the
/// engine seeds its alert cooldowns from so a restart does not re-fire a rule. Empty
/// when history is unavailable or the read fails: the rules then start with no
/// cooldown. One small indexed query at launch.
fn recent_alerts(history: &History, host: HostId, now: i64) -> Vec<kelvo_schema::Event> {
    if !history.is_available() {
        return Vec::new();
    }
    let from = now - kelvo_engine::detect::alert_history_ms();
    match history.read(|r| r.events(host, from, now.saturating_add(1))) {
        Ok(events) => events
            .into_iter()
            .filter(|e| matches!(e.detail, kelvo_schema::EventDetail::Alert { .. }))
            .collect(),
        Err(e) => {
            tracing::warn!("reading recent alerts, alert cooldowns start fresh: {e:?}");
            Vec::new()
        }
    }
}

/// Fills the host's live ring with the last hour of 10 s history, so a live chart opened
/// after a restart shows what the previous run recorded. A read failure leaves the ring
/// empty, as before: live charts then start at launch.
fn warm_live_ring(history: &History, entry: &HostEntry, host: HostId, now: i64) {
    if !history.is_available() {
        return;
    }
    let query = HistoryQuery {
        host,
        selectors: kelvo_schema::CATALOG
            .iter()
            .filter(|d| d.persisted)
            .map(|d| SeriesSelector {
                metric: d.id.clone(),
                labels: Labels::default(),
            })
            .collect(),
        from_ms: now - kelvo_engine::RING_SPAN_MS,
        to_ms: now,
        tier: TierChoice::Fixed(Tier::S10),
        // One more than the hour holds: `now` is off the 10 s grid, so the range touches
        // a partial bucket at each end, and one slot short merges pairs into 20 s points.
        max_points: u32::try_from(kelvo_engine::RING_SPAN_MS / 10_000 + 1).unwrap_or(u32::MAX),
    };
    match history.read(|r| r.history(&query)) {
        Ok(result) => {
            let rows = entry.warm_ring(&result);
            tracing::info!(rows, "live ring warmed from history");
        }
        Err(e) => tracing::warn!("reading history for the live ring, it starts empty: {e:?}"),
    }
}

/// The data directory's part of startup: create it, open the history store, read or
/// create the host id. Never fails: a directory that cannot be created or written
/// (root-owned after a `sudo` run, a full disk) leaves history unavailable with the reason
/// and the host id in memory, and the app runs live-only.
fn open_data_dir(
    data_dir: &Path,
    binding: impl Fn(HostId) -> Option<String>,
) -> (History, identity::LocalId) {
    // Owner-only (D-074): it holds usage history.
    if let Err(e) = kelvo_store::create_private_dir(data_dir) {
        tracing::error!(dir = %data_dir.display(), "cannot create the data directory, running live-only: {e}");
    }
    // The plugin writes settings.json; one from before D-074 is tightened here. The store,
    // the host id and the calibration file see to their own.
    let settings = data_dir.join(crate::settings::SETTINGS_FILE);
    if let Err(e) = kelvo_store::restrict(&settings) {
        tracing::warn!("restricting {}: {e}", settings.display());
    }
    let history = History::open(data_dir);
    let stored_local = if history.is_available() {
        history.read(|r| r.local_host()).unwrap_or_else(|e| {
            tracing::warn!("reading the stored local host: {e}");
            None
        })
    } else {
        None
    };
    let id = identity::load_or_create(data_dir, stored_local.as_ref(), binding);
    (history, id)
}

/// Accessory (menu bar only) by default; Regular when "Show in Dock" is on.
fn apply_activation_policy(app: &AppHandle, show_in_dock: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if show_in_dock {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        if let Err(e) = app.set_activation_policy(policy) {
            tracing::warn!("setting the activation policy: {e}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, show_in_dock);
}

/// The native theme for every window, so vibrancy and `prefers-color-scheme` follow the
/// Appearance setting.
fn apply_theme(app: &AppHandle, appearance: Appearance) {
    app.set_theme(match appearance {
        Appearance::System => None,
        Appearance::Light => Some(Theme::Light),
        Appearance::Dark => Some(Theme::Dark),
    });
}

/// After onboarding, the login item follows the setting even if it was changed outside
/// Kelvo (System Settings > Login Items) or the app moved.
fn reconcile_login_item(wanted: bool) {
    if let Some(registered) = platform::launch_at_login_registered()
        && registered != wanted
        && let Err(e) = platform::set_launch_at_login(wanted)
    {
        tracing::warn!(wanted, "reconciling the login item: {e}");
    }
}

/// Follows one host's bus for the app's own needs: the `capabilities-changed` and
/// `event-recorded` events, alert notifications, `chip_known`, and Performance mode for window
/// appearance. The latest frame is the hub's.
fn spawn_host_watcher(app: AppHandle, entry: Arc<HostEntry>) {
    let mut sub = entry.bus().subscribe();
    tauri::async_runtime::spawn(async move {
        let mut performance = None;
        while let Some(msg) = sub.recv().await {
            match msg {
                BusMsg::Caps(caps) => {
                    let host = entry.record().id;
                    let unknown_chip = matches!(
                        caps.modules.get(&Module::Sensors),
                        Some(ModuleCap::Unsupported(UnsupportedReason::UnknownChip))
                    );
                    if unknown_chip && entry.update_record(|r| r.info.chip_known = false) {
                        tracing::info!(%host, "chip not mapped for sensors");
                        if let Some(state) = app.try_state::<AppState>() {
                            if let Ok(w) = state.history.writer()
                                && let Err(e) = w.upsert_host(entry.record())
                            {
                                tracing::warn!("updating the host record: {e}");
                            }
                            state.emit_hosts_changed(&app);
                        }
                    }
                    let event = CapabilitiesChanged {
                        host,
                        capabilities: (*caps).clone(),
                    };
                    if let Err(e) = event.emit(&app) {
                        tracing::warn!("emitting capabilities-changed: {e}");
                    }
                }
                BusMsg::Event(e) => {
                    if entry.record().is_local {
                        notify::alert(&app, &e);
                    }
                    let event = EventRecorded {
                        host: entry.record().id,
                        event: (*e).clone(),
                    };
                    if let Err(err) = event.emit(&app) {
                        tracing::warn!("emitting event-recorded: {err}");
                    }
                }
                BusMsg::Status(s) if entry.record().is_local => {
                    if performance != Some(s.performance)
                        && let Some(state) = app.try_state::<AppState>()
                    {
                        performance = Some(s.performance);
                        state.update_appearance(&app, |a| a.performance = s.performance);
                    }
                }
                _ => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HistoryUnavailableReason;
    use kelvo_engine::detect::Detectors;
    use kelvo_schema::{
        AlertCause, AlertRule, AlertSettings, DetectorThresholds, Event, EventDetail, SeriesKey,
        ThermalState,
    };

    /// The launch read that seeds alert cooldowns, against a real store: an alert from
    /// 5 minutes ago is read and holds the rule off; one from 31 minutes ago and
    /// non-alert events are left out.
    #[test]
    fn recent_alerts_seed_the_cooldown_from_the_store() {
        const NOW: i64 = 1_800_000_000_000;
        const MIN: i64 = 60_000;
        let data_dir = std::env::temp_dir().join(format!(
            "kelvo-shell-recent-alerts-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let (history, id) = open_data_dir(&data_dir, |_| None);
        let writer = history
            .register_host(&platform::local_host_record(id.id))
            .unwrap();
        let alert = |ago: i64| Event {
            ts_ms: NOW - ago,
            start_ms: NOW - ago,
            processes: vec![],
            detail: EventDetail::Alert {
                rule_id: AlertRule::thermal_serious().id,
                rule_name: "Thermal state Serious or worse".into(),
                cause: AlertCause::ThermalState {
                    state: ThermalState::Serious,
                },
            },
        };
        let change = Event {
            ts_ms: NOW - 2 * MIN,
            start_ms: NOW - 2 * MIN,
            processes: vec![],
            detail: EventDetail::ThermalState {
                from: None,
                to: ThermalState::Serious,
            },
        };
        for e in [alert(31 * MIN), alert(5 * MIN), change] {
            writer.record_event(id.id, &e).unwrap();
        }
        writer.flush().unwrap();

        let recent = recent_alerts(&history, id.id, NOW);
        assert_eq!(recent, [alert(5 * MIN)]);

        // Seeded, the rule stays quiet on Serious; unseeded it fires at once.
        let fires = |seed: &[Event]| -> usize {
            let mut d = Detectors::new(
                DetectorThresholds::DEFAULT,
                AlertSettings {
                    hot_process: false,
                    thermal_serious: true,
                },
            );
            d.bind(&[SeriesKey::parse("thermal.state").unwrap()]);
            d.seed_alert_history(seed);
            let mut out = Vec::new();
            for t in 0..60 {
                d.on_tick(NOW + t * 1000, &[2.0], &[1000], None, &mut out);
            }
            out.len()
        };
        assert_eq!(fires(&recent), 0);
        assert_eq!(fires(&[]), 1);
        history.close();
    }

    /// A source that is never started: `warm_live_ring` only needs the entry's hub.
    struct Unstarted(HostRecord);

    impl kelvo_engine::Source for Unstarted {
        fn host(&self) -> HostRecord {
            self.0.clone()
        }
        fn capabilities(&self) -> kelvo_schema::Capabilities {
            kelvo_schema::Capabilities::default()
        }
        fn start(
            self: Arc<Self>,
            _: kelvo_engine::SourceSink,
        ) -> Result<kelvo_engine::SourceHandle, kelvo_engine::SourceError> {
            Err(kelvo_engine::SourceError::AlreadyStarted)
        }
    }

    /// The launch read that warms the live ring (D-097), against a real store: the
    /// previous run's 10 s buckets come back at 10 s, per-core series included, cut to
    /// the ring's hour, with the time since the quit left empty.
    #[test]
    fn the_live_ring_warms_from_the_last_hour_of_10s_history() {
        const S10: i64 = 10_000;
        const MIN: i64 = 60_000;
        // Launch lands between ticks, as it always does in the field.
        const LAUNCH: i64 = 1_800_000_000_000 + 4_321;
        let data_dir = std::env::temp_dir().join(format!(
            "kelvo-shell-warm-ring-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let (history, id) = open_data_dir(&data_dir, |_| None);
        let record = platform::local_host_record(id.id);
        let writer = history.register_host(&record).unwrap();

        // The previous run: 90 minutes of buckets, quit 20 minutes before this launch.
        let series: Arc<[SeriesKey]> = ["cpu.total", "cpu.load{core=0}", "cpu.load{core=1}"]
            .map(|k| SeriesKey::parse(k).unwrap())
            .into();
        let quit = LAUNCH - LAUNCH.rem_euclid(S10) - 20 * MIN;
        let mut ts = quit - 90 * MIN;
        while ts < quit {
            let v = (ts / S10 % 100) as f32;
            writer
                .write_bucket(kelvo_store::BucketRow {
                    host: id.id,
                    tier: Tier::S10,
                    bucket_ts: ts,
                    series: Arc::clone(&series),
                    stats: [
                        v,
                        v,
                        v,
                        v + 1.0,
                        v + 1.0,
                        v + 1.0,
                        v + 2.0,
                        v + 2.0,
                        v + 2.0,
                    ]
                    .to_vec(),
                })
                .unwrap();
            ts += S10;
        }
        writer.flush().unwrap();

        let registry = HostRegistry::new();
        let entry = registry.insert(record.clone(), Arc::new(Unstarted(record)));
        warm_live_ring(&history, &entry, id.id, LAUNCH);

        let segs = crate::live::LiveFeed::hub(&*entry).backfill(i64::MIN);
        assert_eq!(segs.len(), 1, "one contiguous run of buckets");
        let seg = &segs[0];
        assert_eq!(seg.layout.layout_no, kelvo_engine::WARM_LAYOUT_NO);
        assert_eq!(seg.interval_ms, 10_000, "the 10 s tier, not merged slots");
        let keys: Vec<String> = seg.layout.series.iter().map(|k| k.to_string()).collect();
        for k in ["cpu.total", "cpu.load{core=0}", "cpu.load{core=1}"] {
            assert!(keys.iter().any(|s| s == k), "{k} missing from {keys:?}");
        }
        // The newest row is the last bucket's end; nothing is drawn after the quit.
        let newest = seg.start_ms + (seg.rows.len() as i64 - 1) * S10;
        assert_eq!(newest, quit);
        // The oldest row is inside the ring's hour before launch.
        assert!(
            seg.start_ms >= LAUNCH - kelvo_engine::RING_SPAN_MS,
            "{}",
            seg.start_ms
        );
        // The value at each row is that bucket's average.
        let total = keys.iter().position(|s| s == "cpu.total").unwrap();
        let last_bucket = quit - S10;
        assert_eq!(
            seg.rows.last().unwrap()[total],
            (last_bucket / S10 % 100) as f32
        );
        history.close();
    }

    /// No history yet (first launch) leaves the ring empty, so the first frame starts it.
    #[test]
    fn the_live_ring_stays_empty_without_history() {
        let data_dir = std::env::temp_dir().join(format!(
            "kelvo-shell-warm-empty-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let (history, id) = open_data_dir(&data_dir, |_| None);
        let record = platform::local_host_record(id.id);
        history.register_host(&record).unwrap();
        let registry = HostRegistry::new();
        let entry = registry.insert(record.clone(), Arc::new(Unstarted(record)));
        warm_live_ring(&history, &entry, id.id, 1_800_000_000_000);
        assert!(
            crate::live::LiveFeed::hub(&*entry)
                .backfill(i64::MIN)
                .is_empty()
        );
        history.close();
    }

    /// A data directory that cannot be created (here a path under a regular file; in the
    /// field root-owned after a `sudo` run, or a full disk) does not stop the launch:
    /// history is unavailable with the reason and the host id lives in memory.
    #[test]
    fn an_unusable_data_dir_runs_live_only_with_an_in_memory_id() {
        let parent = std::env::temp_dir().join(format!(
            "kelvo-shell-data-dir-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::write(parent.join("file"), "").unwrap();
        let data_dir = parent.join("file").join("Kelvo");

        let (history, id) = open_data_dir(&data_dir, |_| None);
        assert!(!history.is_available());
        assert!(
            matches!(
                history.unavailable_reason(),
                Some(HistoryUnavailableReason::Failed { .. })
            ),
            "{:?}",
            history.unavailable_reason()
        );
        assert_eq!(id.source, identity::IdSource::Created);
        assert!(!id.persisted);
    }

    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    const PRIVATE_FILES: [&str; 3] = ["history.sqlite", "history.sqlite.lock", "host-id"];

    /// D-074: the data directory holds usage history, so it and what Kelvo writes in it
    /// are owner-only, and a directory from before is tightened on launch.
    #[test]
    fn the_data_dir_and_its_files_are_owner_only() {
        let parent = std::env::temp_dir().join(format!(
            "kelvo-shell-data-perms-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let data_dir = parent.join("Kelvo");
        let (history, id) = open_data_dir(&data_dir, |_| None);
        assert!(history.is_available() && id.persisted);
        assert_eq!(mode(&data_dir), 0o700);
        for f in PRIVATE_FILES {
            assert_eq!(mode(&data_dir.join(f)), 0o600, "{f}");
        }
        history.close();

        // As a launch before D-074 left them.
        set_mode(&data_dir, 0o755);
        for f in PRIVATE_FILES {
            set_mode(&data_dir.join(f), 0o644);
        }
        std::fs::write(data_dir.join(crate::settings::SETTINGS_FILE), "{}").unwrap();
        set_mode(&data_dir.join(crate::settings::SETTINGS_FILE), 0o644);
        let (history, _) = open_data_dir(&data_dir, |_| None);
        assert_eq!(mode(&data_dir), 0o700);
        for f in PRIVATE_FILES
            .into_iter()
            .chain([crate::settings::SETTINGS_FILE])
        {
            assert_eq!(mode(&data_dir.join(f)), 0o600, "{f}");
        }
        history.close();
    }
}

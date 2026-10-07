//! The settings owner (architecture.md infra 8). Rust is the only writer of the settings
//! file; windows read with `get_settings`, change with `update_settings(patch)`, and
//! mirror the `settings-changed` event.
//!
//! An update runs under one lock, in this order: apply the patch, validate, run the
//! fallible side effects (login item), save the file, commit the new revision, apply the
//! engine-affecting changes, emit the event. A failure before the commit leaves
//! everything as it was. Holding the lock through the emit keeps events in revision
//! order.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use kelvo_engine::EngineSettings;
use kelvo_schema::AlertRule;
use kelvo_schema::Module;
use kelvo_schema::Settings;
use kelvo_schema::settings::{
    Appearance, ChartWindow, MemoryUnit, MenuBarMode, NetworkUnit, TemperatureUnit,
};
use serde::Deserialize;

use crate::error::CommandError;
use crate::ipc::SettingsSnapshot;

/// A partial change. Every field is optional; absent fields keep their value.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, specta::Type)]
#[serde(default)]
pub struct SettingsPatch {
    #[specta(optional)]
    pub modules: Option<BTreeMap<Module, ModulePatch>>,
    #[specta(optional)]
    pub sampling: Option<SamplingPatch>,
    #[specta(optional)]
    pub history: Option<HistoryPatch>,
    #[specta(optional)]
    pub units: Option<UnitsPatch>,
    #[specta(optional)]
    pub general: Option<GeneralPatch>,
    #[specta(optional)]
    pub onboarding: Option<OnboardingPatch>,
    #[specta(optional)]
    pub alerts: Option<AlertsPatch>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, specta::Type)]
#[serde(default)]
pub struct ModulePatch {
    #[specta(optional)]
    pub enabled: Option<bool>,
    #[specta(optional)]
    pub menu_bar: Option<MenuBarMode>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, specta::Type)]
#[serde(default)]
pub struct SamplingPatch {
    #[specta(optional)]
    pub interval_ms: Option<u32>,
    #[specta(optional)]
    pub slow_on_battery: Option<bool>,
    #[specta(optional)]
    pub performance_mode: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, specta::Type)]
#[serde(default)]
pub struct HistoryPatch {
    #[specta(optional)]
    pub retention_days: Option<u16>,
    #[specta(optional)]
    pub size_limit_mb: Option<u32>,
    #[specta(optional)]
    pub network_history: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, specta::Type)]
#[serde(default)]
pub struct UnitsPatch {
    #[specta(optional)]
    pub temperature: Option<TemperatureUnit>,
    #[specta(optional)]
    pub network: Option<NetworkUnit>,
    #[specta(optional)]
    pub memory: Option<MemoryUnit>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, specta::Type)]
#[serde(default)]
pub struct GeneralPatch {
    #[specta(optional)]
    pub launch_at_login: Option<bool>,
    #[specta(optional)]
    pub show_in_dock: Option<bool>,
    #[specta(optional)]
    pub appearance: Option<Appearance>,
    #[specta(optional)]
    pub check_updates: Option<bool>,
    #[specta(optional)]
    pub chart_window: Option<ChartWindow>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, specta::Type)]
#[serde(default)]
pub struct OnboardingPatch {
    #[specta(optional)]
    pub completed: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, specta::Type)]
#[serde(default)]
pub struct AlertsPatch {
    #[specta(optional)]
    pub hot_process: Option<bool>,
    #[specta(optional)]
    pub thermal_serious: Option<bool>,
}

fn set<T>(slot: &mut T, v: Option<T>) {
    if let Some(v) = v {
        *slot = v;
    }
}

impl SettingsPatch {
    /// `base` with this patch applied. A module the settings have no entry for (Sensors,
    /// Unknown) is rejected here rather than silently dropped.
    pub fn apply(&self, base: &Settings) -> Result<Settings, CommandError> {
        let mut s = base.clone();
        if let Some(modules) = &self.modules {
            for (module, p) in modules {
                let entry =
                    s.modules
                        .get_mut(module)
                        .ok_or_else(|| CommandError::InvalidSettings {
                            message: format!(
                                "{} has no settings entry of its own",
                                module.as_str()
                            ),
                        })?;
                set(&mut entry.enabled, p.enabled);
                set(&mut entry.menu_bar, p.menu_bar);
            }
        }
        if let Some(p) = self.sampling {
            set(&mut s.sampling.interval_ms, p.interval_ms);
            set(&mut s.sampling.slow_on_battery, p.slow_on_battery);
            set(&mut s.sampling.performance_mode, p.performance_mode);
        }
        if let Some(p) = self.history {
            set(&mut s.history.retention_days, p.retention_days);
            set(&mut s.history.size_limit_mb, p.size_limit_mb);
            set(&mut s.history.network_history, p.network_history);
        }
        if let Some(p) = self.units {
            set(&mut s.units.temperature, p.temperature);
            set(&mut s.units.network, p.network);
            set(&mut s.units.memory, p.memory);
        }
        if let Some(p) = self.general {
            set(&mut s.general.launch_at_login, p.launch_at_login);
            set(&mut s.general.show_in_dock, p.show_in_dock);
            set(&mut s.general.appearance, p.appearance);
            set(&mut s.general.check_updates, p.check_updates);
            set(&mut s.general.chart_window, p.chart_window);
        }
        if let Some(p) = self.onboarding {
            set(&mut s.onboarding.completed, p.completed);
        }
        if let Some(p) = self.alerts {
            set(&mut s.alerts.hot_process, p.hot_process);
            set(&mut s.alerts.thermal_serious, p.thermal_serious);
        }
        s.validate().map_err(|e| CommandError::InvalidSettings {
            message: e.to_string(),
        })?;
        Ok(s)
    }
}

/// Whether the engine has to be reconfigured to go from `old` to `new`: whatever it
/// derives from settings changed (interval, battery slowdown, a module switched on or
/// off, a module shown in or hidden from the menu bar). Comparing the engine's own view
/// keeps this from drifting when `EngineSettings` grows a field.
pub fn engine_affecting(old: &Settings, new: &Settings) -> bool {
    EngineSettings::from_settings(old) != EngineSettings::from_settings(new)
}

/// Whether the pruner should run now rather than at its next hour: the retention or the
/// size limit changed. A lower limit trims the file within seconds instead of an hour
/// later; a higher one costs one cheap prune.
pub fn prune_affecting(old: &Settings, new: &Settings) -> bool {
    old.history.retention_days != new.history.retention_days
        || old.history.size_limit_mb != new.history.size_limit_mb
}

/// Whether the "Network history" setting (D-089) changed, so the local engine has to
/// start or stop recording per-app bytes.
pub fn network_history_affecting(old: &Settings, new: &Settings) -> bool {
    old.history.network_history != new.history.network_history
}

/// The alert rules that went from off to on: the moment to ask for notification
/// permission and post the confirmation that makes macOS show its prompt (D-084).
pub fn alerts_switched_on(old: &Settings, new: &Settings) -> Vec<AlertRule> {
    old.alerts
        .rules()
        .into_iter()
        .zip(new.alerts.rules())
        .filter(|(was, now)| !was.enabled && now.enabled)
        .map(|(_, now)| now)
        .collect()
}

/// Where the settings live on disk. Production is `tauri-plugin-store`'s
/// `settings.json`; tests use memory.
pub trait SettingsFile: Send + Sync {
    /// The stored settings value, `None` when there is none (first run).
    fn load(&self) -> Option<serde_json::Value>;
    fn save(&self, value: serde_json::Value) -> Result<(), String>;
}

/// What an update does besides storing the settings. Implemented by the app shell; tests
/// record the calls.
pub trait SettingsEffects {
    /// Runs before the change is saved. An error aborts the update with nothing changed
    /// (registering the login item can fail).
    fn prepare(&self, old: &Settings, new: &Settings) -> Result<(), CommandError>;
    /// Runs after the change is committed, still under the settings lock: reconfigure the
    /// engine, the Dock and the theme, then emit `settings-changed`.
    fn committed(&self, old: &Settings, now: &SettingsSnapshot);
}

/// The key the settings object is stored under in the store file.
pub const SETTINGS_KEY: &str = "settings";

struct Current {
    revision: u64,
    settings: Settings,
}

pub struct SettingsOwner {
    file: Box<dyn SettingsFile>,
    current: Mutex<Current>,
    /// The file was missing: this is the first run.
    first_run: bool,
}

impl SettingsOwner {
    /// Loads the settings from `file`. A missing value starts from defaults (first run); a
    /// value that fails to decode or validate (a file from a newer build with a settings
    /// enum value this build does not know, D-040, or a hand-edited file) also starts from
    /// defaults, with a warning, and is overwritten on the next change.
    pub fn load(file: Box<dyn SettingsFile>) -> Self {
        let stored = file.load();
        let first_run = stored.is_none();
        let settings = match stored {
            None => Settings::default(),
            Some(value) => match serde_json::from_value::<Settings>(value) {
                Ok(s) => match s.validate() {
                    Ok(()) => s,
                    Err(e) => {
                        tracing::warn!("stored settings are invalid ({e}); using defaults");
                        Settings::default()
                    }
                },
                Err(e) => {
                    tracing::warn!("stored settings do not decode ({e}); using defaults");
                    Settings::default()
                }
            },
        };
        Self {
            file,
            current: Mutex::new(Current {
                revision: 1,
                settings,
            }),
            first_run,
        }
    }

    pub fn first_run(&self) -> bool {
        self.first_run
    }

    fn lock(&self) -> MutexGuard<'_, Current> {
        self.current.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self) -> SettingsSnapshot {
        let c = self.lock();
        SettingsSnapshot {
            revision: c.revision,
            settings: c.settings.clone(),
        }
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    /// Applies `patch`. A patch that changes nothing returns the current snapshot without
    /// saving, a new revision or an event.
    pub fn update(
        &self,
        patch: &SettingsPatch,
        effects: &dyn SettingsEffects,
    ) -> Result<SettingsSnapshot, CommandError> {
        let mut c = self.lock();
        let new = patch.apply(&c.settings)?;
        if new == c.settings {
            return Ok(SettingsSnapshot {
                revision: c.revision,
                settings: new,
            });
        }
        effects.prepare(&c.settings, &new)?;
        let value = serde_json::to_value(&new).map_err(|e| CommandError::SettingsNotSaved {
            message: e.to_string(),
        })?;
        self.file
            .save(value)
            .map_err(|message| CommandError::SettingsNotSaved { message })?;
        let old = std::mem::replace(&mut c.settings, new);
        c.revision += 1;
        let now = SettingsSnapshot {
            revision: c.revision,
            settings: c.settings.clone(),
        };
        effects.committed(&old, &now);
        Ok(now)
    }
}

/// The settings file's name in the app data directory.
pub const SETTINGS_FILE: &str = "settings.json";

/// The settings file through `tauri-plugin-store` (`settings.json` in the app data
/// directory). Auto-save is off: the owner saves explicitly, so a failed save is reported
/// to the caller.
pub struct PluginStoreFile<R: tauri::Runtime> {
    store: std::sync::Arc<tauri_plugin_store::Store<R>>,
    /// Where the plugin keeps it, to make it owner-only after it writes (D-074).
    path: std::path::PathBuf,
}

impl<R: tauri::Runtime> PluginStoreFile<R> {
    pub fn open(app: &tauri::AppHandle<R>) -> anyhow::Result<Self> {
        use anyhow::Context;
        use tauri::Manager;
        let path = app
            .path()
            .app_data_dir()
            .context("resolving the app data directory")?
            .join(SETTINGS_FILE);
        let store = tauri_plugin_store::StoreBuilder::new(app, SETTINGS_FILE)
            .disable_auto_save()
            .build()
            .context("opening settings.json")?;
        Ok(Self { store, path })
    }
}

impl<R: tauri::Runtime> SettingsFile for PluginStoreFile<R> {
    fn load(&self) -> Option<serde_json::Value> {
        self.store.get(SETTINGS_KEY)
    }

    fn save(&self, value: serde_json::Value) -> Result<(), String> {
        self.store.set(SETTINGS_KEY, value);
        self.store.save().map_err(|e| e.to_string())?;
        // The plugin creates the file with the default mode and rewrites it in place,
        // which keeps the mode, so this does work once per file.
        if let Err(e) = kelvo_store::restrict(&self.path) {
            tracing::warn!("restricting {}: {e}", self.path.display());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::sync::Arc;

    use kelvo_schema::settings::ModuleSettings;

    use super::*;

    /// A settings file in memory that outlives the owner, so a test can "restart".
    #[derive(Clone, Default)]
    struct MemFile {
        value: Arc<Mutex<Option<serde_json::Value>>>,
        fail: Arc<Mutex<bool>>,
        saves: Arc<Mutex<u32>>,
    }

    impl SettingsFile for MemFile {
        fn load(&self) -> Option<serde_json::Value> {
            self.value.lock().unwrap().clone()
        }
        fn save(&self, value: serde_json::Value) -> Result<(), String> {
            if *self.fail.lock().unwrap() {
                return Err("disk full".into());
            }
            *self.saves.lock().unwrap() += 1;
            *self.value.lock().unwrap() = Some(value);
            Ok(())
        }
    }

    #[derive(Default)]
    struct Recorder {
        calls: RefCell<Vec<String>>,
        refuse: bool,
    }

    impl SettingsEffects for Recorder {
        fn prepare(&self, _old: &Settings, _new: &Settings) -> Result<(), CommandError> {
            self.calls.borrow_mut().push("prepare".into());
            if self.refuse {
                return Err(CommandError::LaunchAtLogin {
                    message: "denied".into(),
                });
            }
            Ok(())
        }
        fn committed(&self, old: &Settings, now: &SettingsSnapshot) {
            if engine_affecting(old, &now.settings) {
                self.calls.borrow_mut().push("engine".into());
            }
            self.calls
                .borrow_mut()
                .push(format!("event r{}", now.revision));
        }
    }

    fn interval(ms: u32) -> SettingsPatch {
        SettingsPatch {
            sampling: Some(SamplingPatch {
                interval_ms: Some(ms),
                ..SamplingPatch::default()
            }),
            ..SettingsPatch::default()
        }
    }

    #[test]
    fn first_run_starts_from_defaults() {
        let owner = SettingsOwner::load(Box::new(MemFile::default()));
        assert!(owner.first_run());
        assert_eq!(owner.get().settings, Settings::default());
        assert_eq!(owner.get().revision, 1);
    }

    #[test]
    fn update_persists_bumps_revision_and_survives_restart() {
        let file = MemFile::default();
        let owner = SettingsOwner::load(Box::new(file.clone()));
        let fx = Recorder::default();
        let snap = owner.update(&interval(2000), &fx).unwrap();
        assert_eq!(snap.revision, 2);
        assert_eq!(snap.settings.sampling.interval_ms, 2000);
        let snap = owner
            .update(
                &SettingsPatch {
                    units: Some(UnitsPatch {
                        temperature: Some(TemperatureUnit::Celsius),
                        ..UnitsPatch::default()
                    }),
                    ..SettingsPatch::default()
                },
                &fx,
            )
            .unwrap();
        assert_eq!(snap.revision, 3);

        let restarted = SettingsOwner::load(Box::new(file.clone()));
        assert!(!restarted.first_run());
        let s = restarted.settings();
        assert_eq!(s.sampling.interval_ms, 2000);
        assert_eq!(s.units.temperature, TemperatureUnit::Celsius);
        // Untouched fields keep their value.
        assert_eq!(s.history.retention_days, 30);
    }

    #[test]
    fn engine_is_reconfigured_before_the_event() {
        let owner = SettingsOwner::load(Box::new(MemFile::default()));
        let fx = Recorder::default();
        owner.update(&interval(500), &fx).unwrap();
        // A display-only change does not touch the engine.
        owner
            .update(
                &SettingsPatch {
                    general: Some(GeneralPatch {
                        appearance: Some(Appearance::Dark),
                        ..GeneralPatch::default()
                    }),
                    ..SettingsPatch::default()
                },
                &fx,
            )
            .unwrap();
        // Switching a module off does.
        let mut modules = BTreeMap::new();
        modules.insert(
            Module::Gpu,
            ModulePatch {
                enabled: Some(false),
                menu_bar: None,
            },
        );
        owner
            .update(
                &SettingsPatch {
                    modules: Some(modules),
                    ..SettingsPatch::default()
                },
                &fx,
            )
            .unwrap();
        assert_eq!(
            *fx.calls.borrow(),
            vec![
                "prepare", "engine", "event r2", "prepare", "event r3", "prepare", "engine",
                "event r4"
            ]
        );
    }

    fn menu_bar(module: Module, mode: MenuBarMode) -> SettingsPatch {
        SettingsPatch {
            modules: Some(BTreeMap::from([(
                module,
                ModulePatch {
                    enabled: None,
                    menu_bar: Some(mode),
                },
            )])),
            ..SettingsPatch::default()
        }
    }

    /// The engine runs a menu-bar module's live collectors with no window open (D-067),
    /// so showing or hiding a module in the menu bar has to reach it.
    #[test]
    fn menu_bar_mode_reaches_the_engine_when_it_changes_what_is_shown() {
        let owner = SettingsOwner::load(Box::new(MemFile::default()));
        let fx = Recorder::default();
        assert!(!owner.settings().menu_bar_shows(Module::Network));
        owner
            .update(&menu_bar(Module::Network, MenuBarMode::ValueLabel), &fx)
            .unwrap();
        // CPU stays shown, only drawn differently: nothing for the engine.
        owner
            .update(&menu_bar(Module::Cpu, MenuBarMode::ValueLabel), &fx)
            .unwrap();
        owner
            .update(&menu_bar(Module::Network, MenuBarMode::Hidden), &fx)
            .unwrap();
        assert_eq!(
            *fx.calls.borrow(),
            vec![
                "prepare", "engine", "event r2", "prepare", "event r3", "prepare", "engine",
                "event r4"
            ]
        );
    }

    #[test]
    fn invalid_patch_changes_nothing() {
        let file = MemFile::default();
        let owner = SettingsOwner::load(Box::new(file.clone()));
        let fx = Recorder::default();
        let bad = [
            interval(1500),
            SettingsPatch {
                history: Some(HistoryPatch {
                    retention_days: Some(31),
                    ..HistoryPatch::default()
                }),
                ..SettingsPatch::default()
            },
            SettingsPatch {
                history: Some(HistoryPatch {
                    size_limit_mb: Some(200),
                    ..HistoryPatch::default()
                }),
                ..SettingsPatch::default()
            },
            SettingsPatch {
                modules: Some(BTreeMap::from([(
                    Module::Cpu,
                    ModulePatch {
                        enabled: None,
                        menu_bar: Some(MenuBarMode::WattsValue),
                    },
                )])),
                ..SettingsPatch::default()
            },
            SettingsPatch {
                modules: Some(BTreeMap::from([(Module::Sensors, ModulePatch::default())])),
                ..SettingsPatch::default()
            },
        ];
        for patch in bad {
            let err = owner.update(&patch, &fx).unwrap_err();
            assert!(
                matches!(err, CommandError::InvalidSettings { .. }),
                "{patch:?}: {err:?}"
            );
        }
        assert_eq!(owner.get().revision, 1);
        assert_eq!(owner.settings(), Settings::default());
        assert!(fx.calls.borrow().is_empty(), "no effects, no event");
        assert_eq!(*file.saves.lock().unwrap(), 0);
    }

    #[test]
    fn failed_prepare_or_save_changes_nothing() {
        let file = MemFile::default();
        let owner = SettingsOwner::load(Box::new(file.clone()));
        let refusing = Recorder {
            refuse: true,
            ..Recorder::default()
        };
        let err = owner.update(&interval(2000), &refusing).unwrap_err();
        assert!(matches!(err, CommandError::LaunchAtLogin { .. }));
        assert_eq!(*refusing.calls.borrow(), vec!["prepare"]);

        *file.fail.lock().unwrap() = true;
        let fx = Recorder::default();
        let err = owner.update(&interval(2000), &fx).unwrap_err();
        assert!(matches!(err, CommandError::SettingsNotSaved { .. }));
        assert_eq!(
            *fx.calls.borrow(),
            vec!["prepare"],
            "no event after a failed save"
        );

        assert_eq!(owner.get().revision, 1);
        assert_eq!(owner.settings().sampling.interval_ms, 1000);
        assert!(file.value.lock().unwrap().is_none());
    }

    #[test]
    fn size_limit_is_saved_and_wakes_the_pruner() {
        let file = MemFile::default();
        let owner = SettingsOwner::load(Box::new(file.clone()));
        let fx = Recorder::default();
        let before = owner.settings();
        let patch: SettingsPatch =
            serde_json::from_str(r#"{"history":{"size_limit_mb":500}}"#).unwrap();
        let snap = owner.update(&patch, &fx).unwrap();
        assert_eq!(snap.settings.history.size_limit_mb, 500);
        assert_eq!(snap.settings.history.retention_days, 30, "untouched");
        assert!(prune_affecting(&before, &snap.settings));
        // A size-limit change does not reconfigure the engine.
        assert_eq!(*fx.calls.borrow(), vec!["prepare", "event r2"]);
        let restarted = SettingsOwner::load(Box::new(file));
        assert_eq!(restarted.settings().history.size_limit_mb, 500);

        // Lowering it, or changing retention, prunes too; other settings do not.
        let mut lower = snap.settings.clone();
        lower.history.size_limit_mb = 150;
        assert!(prune_affecting(&snap.settings, &lower));
        let mut days = snap.settings.clone();
        days.history.retention_days = 90;
        assert!(prune_affecting(&snap.settings, &days));
        let mut other = snap.settings.clone();
        other.sampling.interval_ms = 30_000;
        assert!(!prune_affecting(&snap.settings, &other));
    }

    /// Network history (D-089) is saved and reaches the engine through its own switch,
    /// not the engine settings or the pruner.
    #[test]
    fn network_history_patch_applies_without_reconfiguring_or_pruning() {
        let patch: SettingsPatch =
            serde_json::from_str(r#"{"history":{"network_history":false}}"#).unwrap();
        let old = Settings::default();
        let new = patch.apply(&old).unwrap();
        assert!(old.history.network_history && !new.history.network_history);
        assert_eq!(new.history.retention_days, old.history.retention_days);
        assert!(network_history_affecting(&old, &new));
        assert!(!engine_affecting(&old, &new));
        assert!(!prune_affecting(&old, &new));
        assert!(!network_history_affecting(&old, &old));
    }

    /// The chart window (D-091) is display-only: saved, never an engine reconfigure.
    #[test]
    fn chart_window_patch_applies_without_reconfiguring() {
        let patch: SettingsPatch =
            serde_json::from_str(r#"{"general":{"chart_window":"30m"}}"#).unwrap();
        let old = Settings::default();
        let new = patch.apply(&old).unwrap();
        assert_eq!(new.general.chart_window, ChartWindow::M30);
        assert_eq!(new.general.appearance, old.general.appearance);
        assert!(!engine_affecting(&old, &new));
        assert!(!prune_affecting(&old, &new));
        let unknown = r#"{"general":{"chart_window":"2h"}}"#;
        assert!(serde_json::from_str::<SettingsPatch>(unknown).is_err());
    }

    #[test]
    fn no_op_patch_does_not_save_or_emit() {
        let file = MemFile::default();
        let owner = SettingsOwner::load(Box::new(file.clone()));
        let fx = Recorder::default();
        let snap = owner.update(&interval(1000), &fx).unwrap();
        assert_eq!(snap.revision, 1);
        assert!(fx.calls.borrow().is_empty());
        assert_eq!(*file.saves.lock().unwrap(), 0);
    }

    #[test]
    fn unreadable_or_invalid_file_falls_back_to_defaults() {
        // A settings enum value from a newer build (D-040).
        let mut newer = serde_json::to_value(Settings::default()).unwrap();
        newer["general"]["appearance"] = "sepia".into();
        // A value that decodes but fails validation.
        let mut invalid = serde_json::to_value(Settings::default()).unwrap();
        invalid["sampling"]["interval_ms"] = 750.into();
        for value in [newer, invalid, serde_json::json!("garbage")] {
            let file = MemFile::default();
            *file.value.lock().unwrap() = Some(value);
            let owner = SettingsOwner::load(Box::new(file));
            assert!(!owner.first_run());
            assert_eq!(owner.settings(), Settings::default());
        }
    }

    #[test]
    fn patch_json_from_the_frontend_decodes_with_absent_fields() {
        let patch: SettingsPatch = serde_json::from_str(
            r#"{"modules":{"disk":{"enabled":true}},"general":{"show_in_dock":true}}"#,
        )
        .unwrap();
        let s = patch.apply(&Settings::default()).unwrap();
        assert_eq!(
            s.module(Module::Disk),
            Some(&ModuleSettings {
                enabled: true,
                menu_bar: MenuBarMode::Hidden
            })
        );
        assert!(s.general.show_in_dock);
    }

    /// Switching an alert on reaches the engine (its rules are engine settings).
    #[test]
    fn alerts_patch_applies_and_affects_the_engine() {
        let patch: SettingsPatch =
            serde_json::from_str(r#"{"alerts":{"thermal_serious":true}}"#).unwrap();
        let old = Settings::default();
        let new = patch.apply(&old).unwrap();
        assert!(new.alerts.thermal_serious && !new.alerts.hot_process);
        assert!(engine_affecting(&old, &new));
    }

    /// Only rules going from off to on post the confirmation (and its permission
    /// prompt); one staying on or going off does not.
    #[test]
    fn alerts_switched_on_lists_only_newly_enabled_rules() {
        let with = |hot_process, thermal_serious| Settings {
            alerts: kelvo_schema::AlertSettings {
                hot_process,
                thermal_serious,
            },
            ..Settings::default()
        };
        let ids = |old, new| -> Vec<uuid::Uuid> {
            alerts_switched_on(&old, &new)
                .iter()
                .map(|r| r.id)
                .collect()
        };
        assert_eq!(
            ids(with(false, false), with(true, false)),
            [AlertRule::hot_process().id]
        );
        assert_eq!(
            ids(with(true, false), with(true, true)),
            [AlertRule::thermal_serious().id]
        );
        assert!(ids(with(true, true), with(true, false)).is_empty());
        assert!(ids(with(true, true), with(true, true)).is_empty());
        assert_eq!(ids(with(false, false), with(true, true)).len(), 2);
    }
}

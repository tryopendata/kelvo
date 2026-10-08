//! User settings (v1-local-monitor.md 6.4 and 4.15). Rust owns them: windows read them
//! with `get_settings`, change them with `update_settings`, and mirror the
//! `settings-changed` event (architecture.md infra 8).
//!
//! Modules: the Settings page has one "Power & Sensors" row, so the settings map has an
//! entry for [`Module::Power`] and none for [`Module::Sensors`]; the Power entry governs
//! both. Use [`Settings::module`] to look a module up, which applies that mapping.
//!
//! Defaults do not know which modules the host has. "On for present modules" is applied
//! by the app shell on first run, from `Capabilities`; [`Settings::default`] turns every
//! module on except Disk.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::alert::AlertRule;
use crate::catalog::Module;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Settings {
    /// One entry per settings module: every [`Module`] except `Sensors`. An entry for a
    /// module this build does not know (a settings file from a newer build) is dropped on
    /// decode (D-040).
    #[serde(deserialize_with = "crate::catalog::deserialize_known_modules")]
    #[specta(type = BTreeMap<Module, ModuleSettings>)]
    pub modules: BTreeMap<Module, ModuleSettings>,
    /// Absent from a settings file written before D-102: the default menu bar.
    #[serde(default)]
    pub menu_bar: MenuBarSettings,
    pub sampling: SamplingSettings,
    pub history: HistorySettings,
    pub units: UnitSettings,
    pub general: GeneralSettings,
    pub onboarding: OnboardingSettings,
    /// Absent from a settings file written before v1.2: both alerts off.
    #[serde(default)]
    pub alerts: AlertSettings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ModuleSettings {
    pub enabled: bool,
}

/// What the menu bar shows (D-102): bars and values in the combined item, and the modules
/// with a status item of their own. The three are independent; a module that is switched
/// off shows in none of them, and keeps its choices for when it comes back.
///
/// A group or field missing from the file takes its default, so a settings file written
/// before a readout existed still loads; a field from a newer build is ignored. The
/// fields are filled in by hand-written `Deserialize` impls rather than `#[serde(default)]`,
/// which would make every field optional in the generated TypeScript.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, specta::Type)]
pub struct MenuBarSettings {
    pub bars: BarSettings,
    pub readouts: ReadoutSettings,
    pub items: ItemSettings,
}

/// Deserializes `$ty` from a mirror with every field optional, taking each missing one
/// from `$ty::default()`.
macro_rules! deserialize_with_defaults {
    ($ty:ident { $($field:ident: $fty:ty),* $(,)? }) => {
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                #[derive(Deserialize)]
                struct Wire {
                    $(#[serde(default)] $field: Option<$fty>,)*
                }
                let w = Wire::deserialize(d)?;
                let base = $ty::default();
                Ok($ty {
                    $($field: w.$field.unwrap_or(base.$field),)*
                })
            }
        }
    };
}

deserialize_with_defaults!(MenuBarSettings {
    bars: BarSettings,
    readouts: ReadoutSettings,
    items: ItemSettings,
});
deserialize_with_defaults!(BarSettings {
    cpu: bool,
    gpu: bool,
    memory: bool,
});
deserialize_with_defaults!(ReadoutSettings {
    cpu: bool,
    gpu: bool,
    memory: bool,
    temperature: bool,
    power: bool,
    network: bool,
    disk: bool,
    battery: bool,
});
deserialize_with_defaults!(ItemSettings {
    cpu: ItemMode,
    gpu: ItemMode,
    memory: ItemMode,
    power: ItemMode,
    network: ItemMode,
    disk: ItemMode,
    battery: ItemMode,
});

/// Which modules draw a bar in the combined item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct BarSettings {
    pub cpu: bool,
    pub gpu: bool,
    pub memory: bool,
}

impl Default for BarSettings {
    fn default() -> Self {
        Self {
            cpu: true,
            gpu: true,
            memory: true,
        }
    }
}

impl BarSettings {
    /// The modules that can be a bar, in drawing order.
    pub const MODULES: [Module; 3] = [Module::Cpu, Module::Gpu, Module::Memory];

    pub fn get(&self, module: Module) -> bool {
        match module {
            Module::Cpu => self.cpu,
            Module::Gpu => self.gpu,
            Module::Memory => self.memory,
            _ => false,
        }
    }
}

/// A value the combined item prints after the bars, with its marker (D-102).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Readout {
    /// `cpu.total`.
    Cpu,
    /// `gpu.util`.
    Gpu,
    /// `mem.pressure`.
    Memory,
    /// `thermal.hottest`.
    Temperature,
    /// `power.system`.
    Power,
    /// `net.tx_total` over `net.rx_total`.
    Network,
    /// % used of the boot volume (`disk.used` / `disk.total`).
    Disk,
    /// `battery.charge`.
    Battery,
}

impl Readout {
    /// Every readout, in the order the menu bar draws them.
    pub const ALL: [Readout; 8] = [
        Readout::Cpu,
        Readout::Gpu,
        Readout::Memory,
        Readout::Temperature,
        Readout::Power,
        Readout::Network,
        Readout::Disk,
        Readout::Battery,
    ];

    /// The module whose series it shows. Temperature is `Sensors`, which the Power &
    /// Sensors settings entry switches.
    pub fn module(self) -> Module {
        match self {
            Readout::Cpu => Module::Cpu,
            Readout::Gpu => Module::Gpu,
            Readout::Memory => Module::Memory,
            Readout::Temperature => Module::Sensors,
            Readout::Power => Module::Power,
            Readout::Network => Module::Network,
            Readout::Disk => Module::Disk,
            Readout::Battery => Module::Battery,
        }
    }
}

/// Which readouts are on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct ReadoutSettings {
    pub cpu: bool,
    pub gpu: bool,
    pub memory: bool,
    pub temperature: bool,
    pub power: bool,
    pub network: bool,
    pub disk: bool,
    pub battery: bool,
}

impl Default for ReadoutSettings {
    fn default() -> Self {
        Self {
            cpu: false,
            gpu: false,
            memory: false,
            temperature: true,
            power: false,
            network: false,
            disk: false,
            battery: false,
        }
    }
}

impl ReadoutSettings {
    pub fn get(&self, r: Readout) -> bool {
        match r {
            Readout::Cpu => self.cpu,
            Readout::Gpu => self.gpu,
            Readout::Memory => self.memory,
            Readout::Temperature => self.temperature,
            Readout::Power => self.power,
            Readout::Network => self.network,
            Readout::Disk => self.disk,
            Readout::Battery => self.battery,
        }
    }
}

/// A module's status item of its own (D-080). Not every mode is offered for every
/// module; see [`ItemMode::allowed_for`].
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum ItemMode {
    #[default]
    Off,
    /// The labelled value: percent, watts, or the read + write or total network rate.
    Value,
    /// The module's graph: CPU sparkline, GPU history bars, memory fill gauge, network
    /// up and down rates.
    Graph,
    /// CPU per-core load, P cores then E cores (CPU only).
    Cores,
}

impl ItemMode {
    /// The options the Settings select offers for `module`.
    pub fn allowed_for(module: Module) -> &'static [ItemMode] {
        use ItemMode::*;
        match module {
            Module::Cpu => &[Off, Value, Graph, Cores],
            Module::Gpu | Module::Memory | Module::Network => &[Off, Value, Graph],
            Module::Power | Module::Sensors | Module::Disk | Module::Battery => &[Off, Value],
            Module::Unknown => &[],
        }
    }
}

/// Each settings module's own item; `Off` by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, specta::Type)]
pub struct ItemSettings {
    pub cpu: ItemMode,
    pub gpu: ItemMode,
    pub memory: ItemMode,
    pub power: ItemMode,
    pub network: ItemMode,
    pub disk: ItemMode,
    pub battery: ItemMode,
}

impl ItemSettings {
    /// `module`'s item; `Sensors` reads the Power & Sensors entry.
    pub fn get(&self, module: Module) -> ItemMode {
        match module {
            Module::Cpu => self.cpu,
            Module::Gpu => self.gpu,
            Module::Memory => self.memory,
            Module::Power | Module::Sensors => self.power,
            Module::Network => self.network,
            Module::Disk => self.disk,
            Module::Battery => self.battery,
            Module::Unknown => ItemMode::Off,
        }
    }

    pub fn get_mut(&mut self, module: Module) -> Option<&mut ItemMode> {
        match module {
            Module::Cpu => Some(&mut self.cpu),
            Module::Gpu => Some(&mut self.gpu),
            Module::Memory => Some(&mut self.memory),
            Module::Power => Some(&mut self.power),
            Module::Network => Some(&mut self.network),
            Module::Disk => Some(&mut self.disk),
            Module::Battery => Some(&mut self.battery),
            Module::Sensors | Module::Unknown => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SamplingSettings {
    /// Base tick; one of [`SamplingSettings::INTERVALS_MS`] (0.5 s to 60 s).
    pub interval_ms: u32,
    /// On battery (or Low Power Mode) the base tick doubles, capped at 60 s.
    pub slow_on_battery: bool,
    /// Performance mode (D-088): the background and visible levers that trade update
    /// rate and motion for CPU. Low Power Mode turns it on too, without changing this
    /// value; [`PerformanceReason::resolve`] gives the effective state. Absent from a
    /// settings file written before it existed: off.
    #[serde(default)]
    pub performance_mode: bool,
}

/// Why Performance mode is in effect (D-088).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceReason {
    #[default]
    Off,
    /// The user turned it on in Settings.
    Setting,
    /// macOS Low Power Mode is on and the setting is off. It ends with Low Power Mode.
    LowPowerMode,
}

impl PerformanceReason {
    /// The setting wins when both hold: turning Low Power Mode off would not end it.
    pub fn resolve(setting: bool, low_power_mode: bool) -> Self {
        match (setting, low_power_mode) {
            (true, _) => Self::Setting,
            (false, true) => Self::LowPowerMode,
            (false, false) => Self::Off,
        }
    }

    pub fn is_on(self) -> bool {
        self != Self::Off
    }
}

/// What powers the Mac right now (D-092), from the same power-source notification the
/// battery back-off reacts to. A Mac without a battery is always on `Adapter`.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum PowerSource {
    /// Running on the battery.
    Battery,
    /// On external power, the battery not charging (full, held at a limit, or no battery).
    #[default]
    Adapter,
    /// On external power and the battery is charging.
    Charging,
}

impl PowerSource {
    /// `on_battery` wins: a battery that reports charging while it powers the Mac is
    /// still the source.
    pub fn resolve(on_battery: bool, charging: bool) -> Self {
        match (on_battery, charging) {
            (true, _) => Self::Battery,
            (false, true) => Self::Charging,
            (false, false) => Self::Adapter,
        }
    }
}

impl SamplingSettings {
    pub const INTERVALS_MS: [u32; 7] = [500, 1000, 2000, 5000, 10_000, 30_000, 60_000];
    /// The longest base tick, which is also the cap on the battery back-off.
    pub const MAX_INTERVAL_MS: u32 = 60_000;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct HistorySettings {
    /// Retention of `tier_1m`, gaps and events; one of [`HistorySettings::RETENTION_DAYS`].
    pub retention_days: u16,
    /// Cap on the history file, WAL included, in MB (10^6 bytes); one of
    /// [`HistorySettings::SIZE_LIMITS_MB`]. The oldest history is trimmed to stay under
    /// it, so it can shorten `retention_days` (D-057, D-059).
    pub size_limit_mb: u32,
    /// "Network history" (D-089): record which apps moved how many bytes, every 10 s
    /// even with no window open. On by default; off, per-process network rates are
    /// sampled only while a view shows them (D-082) and nothing new is recorded. The
    /// `appstore` build never records it. Absent from settings files written before it
    /// existed: on.
    #[serde(default = "network_history_default")]
    pub network_history: bool,
}

const fn network_history_default() -> bool {
    true
}

impl HistorySettings {
    pub const RETENTION_DAYS: [u16; 3] = [7, 30, 90];
    /// "History size limit" options: 150 MB (default), 300 MB, 500 MB, 1 GB.
    pub const SIZE_LIMITS_MB: [u32; 4] = [150, 300, 500, 1000];
    pub const DEFAULT_SIZE_LIMIT_MB: u32 = 150;

    /// The size limit in bytes.
    pub const fn size_limit_bytes(&self) -> u64 {
        self.size_limit_mb as u64 * 1_000_000
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct UnitSettings {
    pub temperature: TemperatureUnit,
    pub network: NetworkUnit,
    pub memory: MemoryUnit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
}

/// MB/s or Mb/s.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NetworkUnit {
    BytesPerSec,
    BitsPerSec,
}

/// GB (10^9) or GiB (2^30).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MemoryUnit {
    Decimal,
    Binary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct GeneralSettings {
    pub launch_at_login: bool,
    pub show_in_dock: bool,
    pub appearance: Appearance,
    /// The only network request Kelvo makes; off means none at all.
    pub check_updates: bool,
    /// The span of every windowed chart on the module pages (D-091). Absent from a
    /// settings file written before it existed: 15m.
    #[serde(default)]
    pub chart_window: ChartWindow,
}

/// The dashboard chart window (D-091). Every option fits the engine's 1-hour ring.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
pub enum ChartWindow {
    #[serde(rename = "5m")]
    M5,
    #[default]
    #[serde(rename = "15m")]
    M15,
    #[serde(rename = "30m")]
    M30,
    #[serde(rename = "1h")]
    H1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct OnboardingSettings {
    pub completed: bool,
}

/// The built-in alert rules (v1.2), each off until the user turns it on. Turning one on
/// posts a one-off "Alerts are on" notification: macOS asks for notification permission
/// on a first delivery, not when the app asks (D-084), so this brings the prompt up
/// while the user is in Settings rather than with the first real alert.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AlertSettings {
    /// [`AlertRule::hot_process`]: a process above 200% CPU for 5 minutes.
    pub hot_process: bool,
    /// [`AlertRule::thermal_serious`]: thermal state Serious or worse.
    pub thermal_serious: bool,
}

impl AlertSettings {
    /// The built-in rules, each with `enabled` from these settings.
    pub fn rules(&self) -> [AlertRule; 2] {
        [
            AlertRule {
                enabled: self.hot_process,
                ..AlertRule::hot_process()
            },
            AlertRule {
                enabled: self.thermal_serious,
                ..AlertRule::thermal_serious()
            },
        ]
    }
}

/// The modules that have their own settings entry (every module except `Sensors`).
pub const SETTINGS_MODULES: [Module; 7] = [
    Module::Cpu,
    Module::Gpu,
    Module::Memory,
    Module::Power,
    Module::Network,
    Module::Disk,
    Module::Battery,
];

impl Default for Settings {
    fn default() -> Self {
        let modules = SETTINGS_MODULES
            .into_iter()
            .map(|m| {
                let settings = ModuleSettings {
                    enabled: m != Module::Disk,
                };
                (m, settings)
            })
            .collect();
        Self {
            modules,
            menu_bar: MenuBarSettings::default(),
            sampling: SamplingSettings {
                interval_ms: 1000,
                slow_on_battery: true,
                performance_mode: false,
            },
            history: HistorySettings {
                retention_days: 30,
                size_limit_mb: HistorySettings::DEFAULT_SIZE_LIMIT_MB,
                network_history: true,
            },
            units: UnitSettings {
                temperature: TemperatureUnit::Fahrenheit,
                network: NetworkUnit::BytesPerSec,
                memory: MemoryUnit::Decimal,
            },
            general: GeneralSettings {
                launch_at_login: true,
                show_in_dock: false,
                appearance: Appearance::System,
                check_updates: true,
                chart_window: ChartWindow::default(),
            },
            onboarding: OnboardingSettings { completed: false },
            alerts: AlertSettings::default(),
        }
    }
}

impl Settings {
    /// Settings for `module`. `Sensors` reads the `Power` entry.
    pub fn module(&self, module: Module) -> Option<&ModuleSettings> {
        let key = if module == Module::Sensors {
            Module::Power
        } else {
            module
        };
        self.modules.get(&key)
    }

    /// Whether the menu bar shows a value from `module` (enabled, and a bar, a readout or
    /// an own item). The engine samples a module the menu bar shows at the base interval
    /// even with no window open (D-067). The Disk readout does not count: disk capacity
    /// is sampled every 60 s whatever is on screen, and counting it would put disk
    /// throughput on the background tick (D-102).
    pub fn menu_bar_shows(&self, module: Module) -> bool {
        if module == Module::Unknown || !self.module(module).is_some_and(|m| m.enabled) {
            return false;
        }
        let mb = &self.menu_bar;
        let readout = Readout::ALL
            .into_iter()
            .any(|r| r != Readout::Disk && r.module() == module && mb.readouts.get(r));
        // Sensors rides on the Power entry, but only the temperature readout draws it.
        let item = module != Module::Sensors && mb.items.get(module) != ItemMode::Off;
        mb.bars.get(module) || readout || item
    }

    /// Checks every value against the allowed sets in 6.4.
    pub fn validate(&self) -> Result<(), SettingsError> {
        if !SamplingSettings::INTERVALS_MS.contains(&self.sampling.interval_ms) {
            return Err(SettingsError::Interval(self.sampling.interval_ms));
        }
        if !HistorySettings::RETENTION_DAYS.contains(&self.history.retention_days) {
            return Err(SettingsError::Retention(self.history.retention_days));
        }
        if !HistorySettings::SIZE_LIMITS_MB.contains(&self.history.size_limit_mb) {
            return Err(SettingsError::SizeLimit(self.history.size_limit_mb));
        }
        if self.modules.contains_key(&Module::Sensors) {
            return Err(SettingsError::SensorsEntry);
        }
        if self.modules.contains_key(&Module::Unknown) {
            return Err(SettingsError::UnknownModule);
        }
        for m in SETTINGS_MODULES {
            if !self.modules.contains_key(&m) {
                return Err(SettingsError::MissingModule(m));
            }
            let mode = self.menu_bar.items.get(m);
            if !ItemMode::allowed_for(m).contains(&mode) {
                return Err(SettingsError::ItemMode { module: m, mode });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("sampling interval {0} ms is not one of 500, 1000, 2000, 5000, 10000, 30000, 60000")]
    Interval(u32),
    #[error("retention {0} days is not one of 7, 30, 90")]
    Retention(u16),
    #[error("history size limit {0} MB is not one of 150, 300, 500, 1000")]
    SizeLimit(u32),
    #[error("module {0:?} has no settings entry")]
    MissingModule(Module),
    #[error("sensors has no settings entry of its own; the power entry covers it")]
    SensorsEntry,
    #[error("settings cannot hold an entry for an unknown module")]
    UnknownModule,
    #[error("menu bar item {mode:?} is not offered for {module:?}")]
    ItemMode { module: Module, mode: ItemMode },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_valid_and_matches_plan() {
        let s = Settings::default();
        s.validate().unwrap();
        assert_eq!(s.sampling.interval_ms, 1000);
        assert_eq!(s.history.retention_days, 30);
        assert_eq!(s.history.size_limit_mb, 150);
        assert_eq!(s.history.size_limit_bytes(), 150_000_000);
        assert!(s.history.network_history);
        assert!(!s.module(Module::Disk).unwrap().enabled);
        assert!(s.module(Module::Cpu).unwrap().enabled);
        // Three bars and the temperature, no own items (D-102 keeps the v1.0 look).
        assert_eq!(s.menu_bar.bars, BarSettings::default());
        let on: Vec<Readout> = Readout::ALL
            .into_iter()
            .filter(|&r| s.menu_bar.readouts.get(r))
            .collect();
        assert_eq!(on, [Readout::Temperature]);
        assert_eq!(s.menu_bar.items, ItemSettings::default());
        assert!(s.general.launch_at_login && !s.general.show_in_dock && s.general.check_updates);
        assert!(!s.onboarding.completed);
        assert_eq!(s.alerts, AlertSettings::default());
        assert!(s.alerts.rules().iter().all(|r| !r.enabled));
    }

    /// A settings file from before v1.2 has no `alerts`: both stay off.
    #[test]
    fn settings_without_alerts_decode_with_alerts_off() {
        let mut json = serde_json::to_value(Settings::default()).unwrap();
        json.as_object_mut().unwrap().remove("alerts");
        let s: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(s.alerts, AlertSettings::default());
    }

    /// A settings file from before D-089 has no `history.network_history`: on.
    #[test]
    fn settings_without_network_history_decode_with_it_on() {
        let mut s = Settings::default();
        s.history.network_history = false;
        let mut json = serde_json::to_value(s).unwrap();
        json["history"]
            .as_object_mut()
            .unwrap()
            .remove("network_history");
        let s: Settings = serde_json::from_value(json).unwrap();
        assert!(s.history.network_history);
    }

    /// A settings file from before Performance mode has no `performance_mode`: off.
    #[test]
    fn settings_without_performance_mode_decode_with_it_off() {
        let mut json = serde_json::to_value(Settings::default()).unwrap();
        json["sampling"]
            .as_object_mut()
            .unwrap()
            .remove("performance_mode");
        let s: Settings = serde_json::from_value(json).unwrap();
        assert!(!s.sampling.performance_mode);
    }

    /// A settings file from before D-091 has no `general.chart_window`: 15m.
    #[test]
    fn settings_without_chart_window_decode_with_15m() {
        let mut s = Settings::default();
        s.general.chart_window = ChartWindow::H1;
        let mut json = serde_json::to_value(s).unwrap();
        json["general"]
            .as_object_mut()
            .unwrap()
            .remove("chart_window");
        let s: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(s.general.chart_window, ChartWindow::M15);
        assert_eq!(Settings::default().general.chart_window, ChartWindow::M15);
    }

    #[test]
    fn chart_window_round_trips_as_its_label() {
        for (w, label) in [
            (ChartWindow::M5, "5m"),
            (ChartWindow::M15, "15m"),
            (ChartWindow::M30, "30m"),
            (ChartWindow::H1, "1h"),
        ] {
            assert_eq!(serde_json::to_string(&w).unwrap(), format!("\"{label}\""));
            let mut s = Settings::default();
            s.general.chart_window = w;
            let json = serde_json::to_string(&s).unwrap();
            assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
        }
    }

    #[test]
    fn performance_reason_prefers_the_setting() {
        use PerformanceReason::*;
        assert_eq!(PerformanceReason::resolve(false, false), Off);
        assert_eq!(PerformanceReason::resolve(true, false), Setting);
        assert_eq!(PerformanceReason::resolve(false, true), LowPowerMode);
        assert_eq!(PerformanceReason::resolve(true, true), Setting);
        assert!(!Off.is_on() && Setting.is_on() && LowPowerMode.is_on());
    }

    #[test]
    fn power_source_from_battery_and_charging() {
        use PowerSource::*;
        assert_eq!(PowerSource::resolve(true, false), Battery);
        assert_eq!(PowerSource::resolve(true, true), Battery);
        assert_eq!(PowerSource::resolve(false, true), Charging);
        // A Mac without a battery: never on battery, never charging.
        assert_eq!(PowerSource::resolve(false, false), Adapter);
        assert_eq!(
            serde_json::to_string(&Charging).unwrap(),
            "\"charging\"",
            "snake_case on the wire"
        );
    }

    #[test]
    fn alert_settings_switch_the_builtin_rules() {
        let a = AlertSettings {
            hot_process: true,
            thermal_serious: false,
        };
        let [hot, thermal] = a.rules();
        assert!(hot.enabled && !thermal.enabled);
        assert_eq!(hot.id, AlertRule::hot_process().id);
        assert_ne!(hot.id, thermal.id);
    }

    #[test]
    fn menu_bar_shows_what_the_tray_draws() {
        let mut s = Settings::default();
        let shown = |s: &Settings| -> Vec<Module> {
            Module::ALL
                .into_iter()
                .filter(|&m| s.menu_bar_shows(m))
                .collect()
        };
        // The temperature draws Sensors; Power draws nothing until watts are on.
        assert_eq!(
            shown(&s),
            [Module::Cpu, Module::Gpu, Module::Memory, Module::Sensors]
        );

        // Power and Sensors share one entry but are drawn by different readouts.
        s.menu_bar.readouts.temperature = false;
        s.menu_bar.readouts.power = true;
        s.menu_bar.readouts.network = true;
        s.menu_bar.bars.cpu = false;
        assert_eq!(
            shown(&s),
            [Module::Gpu, Module::Memory, Module::Power, Module::Network]
        );

        // A readout or an own item shows a module without its bar.
        s.menu_bar.readouts.cpu = true;
        assert!(s.menu_bar_shows(Module::Cpu));
        s.menu_bar.readouts.cpu = false;
        s.menu_bar.items.cpu = ItemMode::Cores;
        assert!(s.menu_bar_shows(Module::Cpu));
        // Power's own item is watts: it does not draw the temperature.
        s.menu_bar.readouts.power = false;
        s.menu_bar.items.power = ItemMode::Value;
        assert!(s.menu_bar_shows(Module::Power));
        assert!(!s.menu_bar_shows(Module::Sensors));

        // The Disk readout reads capacity, sampled every 60 s anyway: not on the
        // background tick. Disk's own item (throughput) is.
        s.modules.get_mut(&Module::Disk).unwrap().enabled = true;
        s.menu_bar.readouts.disk = true;
        assert!(!s.menu_bar_shows(Module::Disk));
        s.menu_bar.items.disk = ItemMode::Value;
        assert!(s.menu_bar_shows(Module::Disk));

        // A disabled module is not sampled, so it draws nothing whatever is chosen.
        s.modules.get_mut(&Module::Network).unwrap().enabled = false;
        assert!(!s.menu_bar_shows(Module::Network));
        s.modules.get_mut(&Module::Power).unwrap().enabled = false;
        assert!(!s.menu_bar_shows(Module::Power));
        assert!(!s.menu_bar_shows(Module::Unknown));
    }

    #[test]
    fn validate_rejects_each_bad_value() {
        let ok = Settings::default();
        let mut s = ok.clone();
        s.sampling.interval_ms = 1500;
        assert_eq!(s.validate(), Err(SettingsError::Interval(1500)));
        s.sampling.interval_ms = 120_000;
        assert_eq!(s.validate(), Err(SettingsError::Interval(120_000)));

        let mut s = ok.clone();
        s.history.retention_days = 31;
        assert_eq!(s.validate(), Err(SettingsError::Retention(31)));

        let mut s = ok.clone();
        s.history.size_limit_mb = 200;
        assert_eq!(s.validate(), Err(SettingsError::SizeLimit(200)));

        let mut s = ok.clone();
        s.modules.remove(&Module::Battery);
        assert_eq!(
            s.validate(),
            Err(SettingsError::MissingModule(Module::Battery))
        );

        let mut s = ok.clone();
        s.modules
            .insert(Module::Sensors, ModuleSettings { enabled: true });
        assert_eq!(s.validate(), Err(SettingsError::SensorsEntry));

        let mut s = ok.clone();
        s.modules
            .insert(Module::Unknown, ModuleSettings { enabled: true });
        assert_eq!(s.validate(), Err(SettingsError::UnknownModule));

        // Cores is CPU only; graphs exist for CPU, GPU, Memory, Network.
        for (module, mode) in [
            (Module::Gpu, ItemMode::Cores),
            (Module::Network, ItemMode::Cores),
            (Module::Disk, ItemMode::Graph),
            (Module::Battery, ItemMode::Graph),
            (Module::Power, ItemMode::Graph),
        ] {
            let mut s = ok.clone();
            *s.menu_bar.items.get_mut(module).unwrap() = mode;
            assert_eq!(s.validate(), Err(SettingsError::ItemMode { module, mode }));
        }
    }

    #[test]
    fn every_allowed_value_validates() {
        for interval in SamplingSettings::INTERVALS_MS {
            for days in HistorySettings::RETENTION_DAYS {
                let mut s = Settings::default();
                s.sampling.interval_ms = interval;
                s.history.retention_days = days;
                s.validate().unwrap();
            }
        }
        for mb in HistorySettings::SIZE_LIMITS_MB {
            let mut s = Settings::default();
            s.history.size_limit_mb = mb;
            s.validate().unwrap();
        }
        for m in SETTINGS_MODULES {
            for &mode in ItemMode::allowed_for(m) {
                let mut s = Settings::default();
                *s.menu_bar.items.get_mut(m).unwrap() = mode;
                s.validate().unwrap();
            }
        }
    }

    #[test]
    fn item_modes_are_offered_per_module() {
        use ItemMode::*;
        assert_eq!(
            ItemMode::allowed_for(Module::Cpu),
            [Off, Value, Graph, Cores]
        );
        for m in [Module::Gpu, Module::Memory, Module::Network] {
            assert_eq!(ItemMode::allowed_for(m), [Off, Value, Graph]);
        }
        for m in [Module::Power, Module::Disk, Module::Battery] {
            assert_eq!(ItemMode::allowed_for(m), [Off, Value]);
        }
        assert_eq!(serde_json::to_string(&Cores).unwrap(), r#""cores""#);
    }

    #[test]
    fn readouts_map_to_their_modules_in_drawing_order() {
        let modules: Vec<Module> = Readout::ALL.into_iter().map(Readout::module).collect();
        assert_eq!(
            modules,
            [
                Module::Cpu,
                Module::Gpu,
                Module::Memory,
                Module::Sensors,
                Module::Power,
                Module::Network,
                Module::Disk,
                Module::Battery,
            ]
        );
        assert_eq!(
            serde_json::to_string(&Readout::Temperature).unwrap(),
            r#""temperature""#
        );
    }

    #[test]
    fn json_round_trip() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""cpu":{"enabled":true}"#), "{json}");
        assert!(
            json.contains(r#""bars":{"cpu":true,"gpu":true,"memory":true}"#),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
    }

    /// A settings file from before D-102: per-module `menu_bar` modes and no top-level
    /// `menu_bar`. The old modes are dropped, the default menu bar applies, and every
    /// other setting is kept.
    #[test]
    fn settings_from_before_d102_load_with_the_default_menu_bar() {
        let mut want = Settings::default();
        want.sampling.interval_ms = 2000;
        let mut json = serde_json::to_value(&want).unwrap();
        json.as_object_mut().unwrap().remove("menu_bar");
        for (_, m) in json["modules"].as_object_mut().unwrap() {
            m["menu_bar"] = "watts_value".into();
        }
        let s: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(s, want);
        s.validate().unwrap();
    }

    /// A file written before a readout (or bar, or item) existed takes that field's
    /// default; one from a newer build with a readout this build lacks drops it.
    #[test]
    fn menu_bar_fields_missing_or_unknown_decode() {
        let mut json = serde_json::to_value(Settings::default()).unwrap();
        let readouts = json["menu_bar"]["readouts"].as_object_mut().unwrap();
        readouts.remove("temperature");
        readouts.insert("fans".into(), true.into());
        json["menu_bar"]["items"]
            .as_object_mut()
            .unwrap()
            .remove("cpu");
        let s: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(s.menu_bar, MenuBarSettings::default());
    }

    #[test]
    fn settings_from_a_newer_build_drop_unknown_modules() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap().replacen(
            r#""modules":{"#,
            r#""modules":{"npu":{"enabled":true},"#,
            1,
        );
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        back.validate().unwrap();
    }
}

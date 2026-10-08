//! Scenario hook for the coalition benchmark (`scripts/bench-coalition.sh`, D-067).
//!
//! Only builds with the `bench` feature read `KELVO_BENCH_SCENARIO`; every other build
//! compiles [`scenario`] to `None`, so a shipped app has no way in. The benchmark builds
//! its own bundle with the feature and a separate identifier, so it never touches the
//! user's history or settings.
//!
//! - `tray`: nothing opens; the app rests with the tray only.
//! - `popover`: the popover is shown, as a tray click would.
//! - `dashboard:<route>`: the dashboard opens at `<route>` (`/dashboard/overview`).
//!
//! Onboarding is skipped for every scenario, without marking it completed (so the login
//! item is never reconciled either).
//!
//! Two more variables shape a run without touching the bench identity's settings file
//! (D-088; D-073 recorded a run spoiled by a setting left in it):
//!
//! - `KELVO_BENCH_PERFORMANCE=1|0`: Performance mode on or off for this launch, in memory.
//! - `KELVO_BENCH_BATTERY=1`: the engine sees the Mac on battery, with the battery
//!   slowdown on, so the backed-off case measures on AC.

use std::time::Duration;

use tauri::AppHandle;

/// How long after setup the scenario's window opens: the popover's webview has to
/// load first.
const OPEN_AFTER: Duration = Duration::from_secs(3);

// Without the feature nothing constructs a scenario; the type and parser stay compiled
// so the parser is tested in every build.
#[cfg_attr(not(feature = "bench"), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scenario {
    Tray,
    Popover,
    Dashboard(String),
}

#[cfg_attr(not(feature = "bench"), allow(dead_code))]
impl Scenario {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "tray" => Some(Self::Tray),
            "popover" => Some(Self::Popover),
            _ => s
                .strip_prefix("dashboard:")
                .map(|route| Self::Dashboard(route.to_owned())),
        }
    }
}

/// The scenario this launch was asked for.
#[cfg(feature = "bench")]
pub fn scenario() -> Option<Scenario> {
    let s = std::env::var("KELVO_BENCH_SCENARIO").ok()?;
    let parsed = Scenario::parse(&s);
    if parsed.is_none() {
        tracing::warn!(scenario = %s, "unknown KELVO_BENCH_SCENARIO");
    }
    parsed
}

#[cfg(not(feature = "bench"))]
pub fn scenario() -> Option<Scenario> {
    None
}

/// `KELVO_BENCH_PERFORMANCE`: Performance mode for this launch, never saved.
#[cfg(feature = "bench")]
pub fn performance_override() -> Option<bool> {
    match std::env::var("KELVO_BENCH_PERFORMANCE").ok()?.as_str() {
        "1" => Some(true),
        "0" => Some(false),
        other => {
            tracing::warn!(value = %other, "KELVO_BENCH_PERFORMANCE is 1 or 0");
            None
        }
    }
}

#[cfg(not(feature = "bench"))]
pub fn performance_override() -> Option<bool> {
    None
}

/// `KELVO_BENCH_BATTERY=1`: report the Mac on battery to the engine.
#[cfg(feature = "bench")]
pub fn force_battery() -> bool {
    std::env::var("KELVO_BENCH_BATTERY").is_ok_and(|v| v == "1")
}

#[cfg(not(feature = "bench"))]
pub fn force_battery() -> bool {
    false
}

/// `KELVO_BENCH_DEFAULTS=1`: this launch starts from shipped default settings, kept in
/// memory and never saved, so a setting left in the bench identity's file cannot spoil a
/// run (D-073, and again on 2026-10-07). `KELVO_BENCH_MENU_BAR=items.cpu=graph,bars.gpu=false`
/// sets menu bar choices (D-102) on top of the defaults. Without the variable, `file` is used.
#[cfg(feature = "bench")]
pub fn settings_file(
    file: Box<dyn crate::settings::SettingsFile>,
) -> Box<dyn crate::settings::SettingsFile> {
    if !std::env::var("KELVO_BENCH_DEFAULTS").is_ok_and(|v| v == "1") {
        return file;
    }
    let menu_bar = std::env::var("KELVO_BENCH_MENU_BAR").unwrap_or_default();
    Box::new(MemorySettings(std::sync::Mutex::new(Some(defaults_with(
        &menu_bar,
    )))))
}

#[cfg(not(feature = "bench"))]
pub fn settings_file(
    file: Box<dyn crate::settings::SettingsFile>,
) -> Box<dyn crate::settings::SettingsFile> {
    file
}

/// Default settings as stored JSON, with `group.field=value` menu bar pairs applied
/// (`items.cpu=graph`, `readouts.power=true`; serde names, comma separated). A pair that
/// does not parse is skipped with a warning; a mode a module does not offer fails the
/// owner's validation, which falls back to defaults.
#[cfg_attr(not(feature = "bench"), allow(dead_code))]
fn defaults_with(menu_bar: &str) -> serde_json::Value {
    let mut value = serde_json::to_value(kelvo_schema::settings::Settings::default())
        .unwrap_or(serde_json::Value::Null);
    for pair in menu_bar.split(',').filter(|p| !p.is_empty()) {
        let Some((path, v)) = pair.split_once('=') else {
            tracing::warn!(pair, "KELVO_BENCH_MENU_BAR pairs are group.field=value");
            continue;
        };
        let v = match v {
            "true" => serde_json::Value::Bool(true),
            "false" => serde_json::Value::Bool(false),
            mode => serde_json::Value::String(mode.to_owned()),
        };
        let pointer = format!("/menu_bar/{}", path.replace('.', "/"));
        match value.pointer_mut(&pointer) {
            Some(slot) => *slot = v,
            None => tracing::warn!(path, "KELVO_BENCH_MENU_BAR: no such field"),
        }
    }
    value
}

/// Settings held in memory for one launch.
#[cfg_attr(not(feature = "bench"), allow(dead_code))]
struct MemorySettings(std::sync::Mutex<Option<serde_json::Value>>);

impl crate::settings::SettingsFile for MemorySettings {
    fn load(&self) -> Option<serde_json::Value> {
        self.0.lock().ok()?.clone()
    }

    fn save(&self, value: serde_json::Value) -> Result<(), String> {
        *self.0.lock().map_err(|e| e.to_string())? = Some(value);
        Ok(())
    }
}

/// `KELVO_BENCH_TRAY_COUNTERS_MS`: how often the tray logs its pacing counters (debug
/// level, "tray frames"), so the bench can count drawn frames inside its measured window.
#[cfg(feature = "bench")]
pub fn tray_counters_every() -> Option<std::time::Duration> {
    let ms: u64 = std::env::var("KELVO_BENCH_TRAY_COUNTERS_MS")
        .ok()?
        .parse()
        .ok()?;
    Some(std::time::Duration::from_millis(ms.max(1000)))
}

#[cfg(not(feature = "bench"))]
pub fn tray_counters_every() -> Option<std::time::Duration> {
    None
}

/// Power signals that report the Mac on battery and pass everything else through.
#[cfg_attr(not(feature = "bench"), allow(dead_code))]
pub struct ForceBattery(pub Box<dyn kelvo_engine::PowerSignals>);

impl kelvo_engine::PowerSignals for ForceBattery {
    fn start(&mut self, inbox: kelvo_engine::Inbox) {
        self.0.start(inbox);
    }

    fn poll(&mut self) -> kelvo_engine::PowerState {
        kelvo_engine::PowerState {
            on_battery: true,
            ..self.0.poll()
        }
    }
}

/// Opens what `scenario` needs, after [`OPEN_AFTER`], on the main thread.
pub fn start(app: &AppHandle, scenario: Scenario) {
    tracing::info!(?scenario, "bench scenario");
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(OPEN_AFTER);
        let handle = app.clone();
        let r = app.run_on_main_thread(move || open(&handle, &scenario));
        if let Err(e) = r {
            tracing::warn!("bench scenario: {e}");
        }
    });
}

fn open(app: &AppHandle, scenario: &Scenario) {
    match scenario {
        Scenario::Tray => {}
        #[cfg(target_os = "macos")]
        Scenario::Popover => {
            crate::popover::toggle(app, std::time::Instant::now(), crate::tray::TRAY_ID);
        }
        #[cfg(not(target_os = "macos"))]
        Scenario::Popover => {}
        Scenario::Dashboard(route) => {
            if let Err(e) = crate::windows::open_dashboard(app, Some(route)) {
                tracing::warn!("bench scenario: opening {route}: {e:?}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_scenarios_the_script_passes() {
        assert_eq!(Scenario::parse("tray"), Some(Scenario::Tray));
        assert_eq!(Scenario::parse("popover"), Some(Scenario::Popover));
        assert_eq!(
            Scenario::parse("dashboard:/dashboard/processes"),
            Some(Scenario::Dashboard("/dashboard/processes".into()))
        );
        assert_eq!(Scenario::parse("window"), None);
    }

    #[test]
    fn menu_bar_pairs_apply_on_top_of_the_defaults() {
        use kelvo_schema::Module;
        use kelvo_schema::settings::{ItemMode, Settings};

        let value = defaults_with(
            "items.cpu=graph,items.gpu=graph,bars.gpu=false,readouts.power=true,bogus,items.nope=graph",
        );
        let s: Settings = serde_json::from_value(value).expect("decodes");
        let d = Settings::default();
        assert_eq!(s.menu_bar.items.get(Module::Cpu), ItemMode::Graph);
        assert_eq!(s.menu_bar.items.get(Module::Gpu), ItemMode::Graph);
        assert_eq!(s.menu_bar.items.memory, d.menu_bar.items.memory);
        assert!(!s.menu_bar.bars.gpu && s.menu_bar.bars.cpu);
        assert!(s.menu_bar.readouts.power && s.menu_bar.readouts.temperature);
        assert_eq!(s.sampling.interval_ms, d.sampling.interval_ms);
    }

    #[cfg(not(feature = "bench"))]
    #[test]
    fn shipped_builds_ignore_the_variable() {
        // SAFETY (test): no other thread of this test reads the variable.
        unsafe { std::env::set_var("KELVO_BENCH_SCENARIO", "tray") };
        assert_eq!(scenario(), None);
    }
}

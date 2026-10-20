//! Settings for the headless Linux agent (D-115), which has no menu bar, no windows and
//! no onboarding.

use super::{BarSettings, ItemSettings, MenuBarSettings, ReadoutSettings, Settings};

impl Settings {
    /// The agent's settings before the controller first runs `configure`:
    ///
    /// - An empty menu bar, so no module is pinned to `Interest::Live` and the per-tick
    ///   collectors run at their idle period until a viewer asks for live values (D-067).
    /// - Every module on, Disk included: the Mac leaves Disk for onboarding to switch on,
    ///   and the agent has no onboarding.
    /// - History as on the Mac: 30 days within 150 MB (the controller sets the budget the
    ///   user chose), and network history on, which records nothing until a collector
    ///   supplies per-process network bytes.
    pub fn agent_default() -> Self {
        let mut s = Self::default();
        for m in s.modules.values_mut() {
            m.enabled = true;
        }
        s.menu_bar = MenuBarSettings {
            bars: BarSettings {
                cpu: false,
                gpu: false,
                memory: false,
            },
            readouts: ReadoutSettings {
                temperature: false,
                ..ReadoutSettings::default()
            },
            items: ItemSettings::default(),
        };
        s.general.show_menu_bar = false;
        s
    }
}

#[cfg(test)]
mod tests {
    use crate::catalog::Module;
    use crate::settings::{HistorySettings, Settings};

    #[test]
    fn agent_default_shows_nothing_in_the_menu_bar_and_samples_every_module() {
        let s = Settings::agent_default();
        s.validate().unwrap();
        for m in Module::ALL {
            assert!(!s.menu_bar_shows(m), "{m:?} is pinned live by the menu bar");
        }
        for m in crate::settings::SETTINGS_MODULES {
            assert!(s.module(m).unwrap().enabled, "{m:?} is off");
        }
        assert!(!s.general.show_menu_bar);
        assert_eq!(s.history.retention_days, 30);
        assert_eq!(
            s.history.size_limit_mb,
            HistorySettings::DEFAULT_SIZE_LIMIT_MB
        );
        assert!(s.history.network_history);
        assert_eq!(s.sampling, Settings::default().sampling);
    }
}

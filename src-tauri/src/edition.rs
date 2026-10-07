//! What this build of the app can do (D-065), for actions the capabilities path does not
//! cover: capabilities describe hosts and their collectors, not the app. The sandboxed
//! App Store edition (`appstore` feature) reports collectors it dropped through
//! capabilities as usual; app-level actions it cannot take are listed here. No UI code
//! reads the build feature.

use serde::Serialize;

/// Returned by `get_edition`. Fixed for the life of the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
pub struct Edition {
    /// Quit and Force Quit on the Processes page. False in the App Store edition, where
    /// `process_signal` answers `unavailable`; the UI should hide the actions.
    pub process_signal: bool,
}

impl Edition {
    pub fn current() -> Self {
        Self {
            process_signal: crate::process_signal::signals_available(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_the_field_names_the_ui_reads() {
        assert_eq!(
            serde_json::to_value(Edition {
                process_signal: true
            })
            .unwrap(),
            serde_json::json!({ "process_signal": true })
        );
    }
}

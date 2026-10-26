//! Installation excludes maintenance and UI opening until the process restarts.
use tauri::{AppHandle, Manager};
use tokio::sync::OwnedRwLockReadGuard;

use super::Updates;
use crate::{AppState, CommandError};

/// Keep this guard for the entire operation, including blocking work and rollback.
/// The owner exists before UI or remote control starts accepting work.
pub fn maintenance(app: &AppHandle) -> Result<OwnedRwLockReadGuard<()>, CommandError> {
    app.state::<Updates>().begin_maintenance()
}

#[derive(Debug)]
pub(super) enum InstallBlocked {
    Maintenance,
    PolicyOrWindow,
}
impl InstallBlocked {
    pub(super) fn reason(&self) -> &'static str {
        match self {
            Self::Maintenance => "Waiting for maintenance to finish.",
            Self::PolicyOrWindow => {
                "Waiting until automatic installation is enabled and Kelvo windows are closed."
            }
        }
    }
}

impl Updates {
    /// Read the latest native/settings eligibility only after excluding mutators.
    pub(super) fn installation_guard(
        &self,
        automatic: bool,
        eligible: impl FnOnce() -> bool,
    ) -> Result<tokio::sync::OwnedRwLockWriteGuard<()>, InstallBlocked> {
        let guard = self
            .maintenance
            .clone()
            .try_write_owned()
            .map_err(|_| InstallBlocked::Maintenance)?;
        if automatic && !eligible() {
            return Err(InstallBlocked::PolicyOrWindow);
        }
        Ok(guard)
    }
    fn begin_maintenance(&self) -> Result<OwnedRwLockReadGuard<()>, CommandError> {
        self.maintenance
            .clone()
            .try_read_owned()
            .map_err(|_| CommandError::Update {
                message: "Kelvo is installing an update. Try again after it restarts.".into(),
            })
    }
}

pub(super) fn automatic_allowed(app: &AppHandle) -> bool {
    let settings = app.state::<AppState>().settings.settings();
    policy_allows(
        settings.general.check_updates,
        settings.general.auto_install_updates,
        settings.onboarding.completed,
    )
}

fn policy_allows(check: bool, install: bool, onboarding: bool) -> bool {
    check && install && onboarding
}

pub(super) fn interactive_window_open(app: &AppHandle) -> bool {
    // Boards do not exist in the current native shell. When added, their ordinary
    // windows must be excluded here and their edit sessions must hold maintenance.
    // Query native visibility: an occluded dashboard is still an interactive window.
    app.webview_windows()
        .values()
        .any(|window| window.is_visible().unwrap_or(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_opt_out_and_incomplete_onboarding_prevents_installation() {
        assert!(policy_allows(true, true, true));
        for policy in [
            (false, true, true),
            (true, false, true),
            (true, true, false),
        ] {
            assert!(!policy_allows(policy.0, policy.1, policy.2));
        }
    }

    #[tokio::test]
    async fn maintenance_and_installation_exclude_one_another() {
        let owner = Updates::default();
        let setup = owner.begin_maintenance().unwrap();
        assert!(owner.installation_guard(true, || true).is_err());
        drop(setup);
        let install = owner.installation_guard(true, || true).unwrap();
        assert!(owner.begin_maintenance().is_err());
        drop(install);
        assert!(owner.begin_maintenance().is_ok());
    }
}

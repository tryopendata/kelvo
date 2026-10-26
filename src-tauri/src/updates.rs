//! Rust owns release selection, verified downloads, and installation eligibility.
pub(crate) mod cache;
mod gate;
mod schedule;
mod workflow;

use std::sync::{Arc, Mutex};

use kelvo_schema::lock::LockExt;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::Update;
use tauri_specta::Event;

use crate::CommandError;

pub use crate::startup::take_background_restart;
pub use gate::maintenance;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateStatus {
    Idle,
    NotConfigured,
    Checking,
    UpToDate,
    Available {
        version: String,
        notes: String,
    },
    Downloading {
        version: String,
        percent: Option<u8>,
    },
    Downloaded {
        version: String,
    },
    Waiting {
        version: String,
        reason: String,
    },
    CompatibilityBlocked {
        version: String,
        message: String,
    },
    NeedsAuthorization {
        version: String,
    },
    Installing {
        version: String,
    },
    Ready {
        version: String,
    },
    Failed {
        message: String,
    },
}

impl UpdateStatus {
    fn has_download(&self) -> bool {
        matches!(
            self,
            Self::Downloaded { .. } | Self::Waiting { .. } | Self::NeedsAuthorization { .. }
        )
    }
}

#[derive(Clone, Serialize, specta::Type, tauri_specta::Event)]
pub struct UpdateStatusChanged {
    pub status: UpdateStatus,
}

#[derive(Clone)]
struct Candidate {
    update: Update,
    artifact: kelvo_update::Artifact,
    manifest: Vec<u8>,
    signature: String,
}

pub struct Updates {
    status: Mutex<UpdateStatus>,
    candidate: Mutex<Option<Candidate>>,
    operation: tokio::sync::Mutex<()>,
    maintenance: Arc<tokio::sync::RwLock<()>>,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            status: Mutex::new(if updates_enabled() {
                UpdateStatus::Idle
            } else {
                UpdateStatus::NotConfigured
            }),
            candidate: Mutex::new(None),
            operation: tokio::sync::Mutex::new(()),
            maintenance: Arc::new(tokio::sync::RwLock::new(())),
        }
    }
}

fn updates_enabled() -> bool {
    !cfg!(any(
        debug_assertions,
        feature = "appstore",
        feature = "bench"
    ))
}

impl Updates {
    async fn restoration_guard(&self) -> Option<tokio::sync::MutexGuard<'_, ()>> {
        let operation = self.operation.lock().await;
        matches!(self.status(), UpdateStatus::Idle).then_some(operation)
    }

    pub fn status(&self) -> UpdateStatus {
        self.status.lock_ok().clone()
    }

    fn set(&self, app: &AppHandle, status: UpdateStatus) -> UpdateStatus {
        {
            let mut current = self.status.lock_ok();
            if *current == status {
                return status;
            }
            *current = status.clone();
        }
        if let Err(error) = (UpdateStatusChanged {
            status: status.clone(),
        })
        .emit(app)
        {
            tracing::warn!(%error, "publishing update status");
        }
        status
    }

    fn failed(&self, app: &AppHandle, error: impl std::fmt::Display) -> CommandError {
        let message = error.to_string();
        self.set(
            app,
            UpdateStatus::Failed {
                message: message.clone(),
            },
        );
        CommandError::Update { message }
    }
}

fn busy() -> CommandError {
    CommandError::Update {
        message: "An update operation is already running.".into(),
    }
}

pub fn start(app: &AppHandle) {
    app.manage(Updates::default());
    if updates_enabled() {
        schedule::start(app.clone());
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_update_status(state: State<'_, Updates>) -> UpdateStatus {
    state.status()
}

/// Explicit checks work even when automatic checks are disabled.
#[tauri::command]
#[specta::specta]
pub async fn check_for_updates(
    app: AppHandle,
    state: State<'_, Updates>,
) -> Result<UpdateStatus, CommandError> {
    if !updates_enabled() {
        return Ok(UpdateStatus::NotConfigured);
    }
    let _operation = state.operation.try_lock().map_err(|_| busy())?;
    state.check(&app).await
}

/// The caller approves the displayed version, never a URL, signature or file path.
#[tauri::command]
#[specta::specta]
pub async fn install_update(
    app: AppHandle,
    state: State<'_, Updates>,
    version: String,
) -> Result<(), CommandError> {
    if !updates_enabled() {
        return Err(CommandError::Update {
            message: "Updates are unavailable in this build.".into(),
        });
    }
    let _operation = state.operation.try_lock().map_err(|_| busy())?;
    let candidate = state
        .candidate
        .lock_ok()
        .clone()
        .filter(|candidate| candidate.update.version == version)
        .ok_or_else(|| CommandError::Update {
            message: "Check for updates again before installing.".into(),
        })?;
    state.download(&app, &candidate).await?;
    state.install(&app, candidate, false).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn startup_cache_restoration_cannot_replace_manual_work() {
        let owner = Updates::default();
        *owner.status.lock_ok() = UpdateStatus::Idle;
        let manual = owner.operation.lock().await;
        let restore = owner.restoration_guard();
        tokio::pin!(restore);
        tokio::select! {
            biased;
            _ = &mut restore => panic!("restoration raced an in-progress manual operation"),
            () = std::future::ready(()) => {}
        }
        *owner.status.lock_ok() = UpdateStatus::Available {
            version: "9.0.0".into(),
            notes: String::new(),
        };
        drop(manual);
        assert!(
            restore.await.is_none(),
            "startup cache cannot replace a manual candidate"
        );
        *owner.status.lock_ok() = UpdateStatus::Ready {
            version: "9.0.0".into(),
        };
        assert!(
            owner.restoration_guard().await.is_none(),
            "never replace an installed app again"
        );
        *owner.status.lock_ok() = UpdateStatus::Idle;
        assert!(owner.restoration_guard().await.is_some());
    }
}

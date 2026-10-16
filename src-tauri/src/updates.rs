//! App-wide updater ownership. Only Rust chooses the source and holds the verified
//! release candidate; a webview can request a check or approve that exact version.
use std::{sync::Mutex, time::Duration};

use kelvo_schema::lock::LockExt;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use tauri_specta::Event;

use crate::{AppState, CommandError};

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

#[derive(Clone, Serialize, specta::Type, tauri_specta::Event)]
pub struct UpdateStatusChanged {
    pub status: UpdateStatus,
}

pub struct Updates {
    status: Mutex<UpdateStatus>,
    candidate: Mutex<Option<Update>>,
    operation: tokio::sync::Mutex<()>,
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
        }
    }
}

fn updates_enabled() -> bool {
    !cfg!(any(debug_assertions, feature = "appstore"))
}

impl Updates {
    fn set(&self, app: &AppHandle, status: UpdateStatus) -> UpdateStatus {
        *self.status.lock_ok() = status.clone();
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

    async fn check(&self, app: &AppHandle) -> Result<UpdateStatus, CommandError> {
        if !updates_enabled() {
            return Ok(UpdateStatus::NotConfigured);
        }
        let _operation = self.operation.try_lock().map_err(|_| busy())?;
        // Once installed, never replace the on-disk app again before restarting.
        if matches!(*self.status.lock_ok(), UpdateStatus::Ready { .. }) {
            return Ok(self.status.lock_ok().clone());
        }
        self.set(app, UpdateStatus::Checking);
        *self.candidate.lock_ok() = None;
        let updater = app
            .updater_builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| self.failed(app, e))?;
        let candidate = updater.check().await.map_err(|e| self.failed(app, e))?;
        let status = match &candidate {
            Some(update) => UpdateStatus::Available {
                version: update.version.clone(),
                notes: update.body.clone().unwrap_or_default(),
            },
            None => UpdateStatus::UpToDate,
        };
        *self.candidate.lock_ok() = candidate;
        Ok(self.set(app, status))
    }
}

fn busy() -> CommandError {
    CommandError::Update {
        message: "An update operation is already running.".into(),
    }
}

pub fn start(app: &AppHandle) {
    app.manage(Updates::default());
    if !updates_enabled() || crate::bench::scenario().is_some() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            let settings = app.state::<AppState>().settings.settings();
            if settings.general.check_updates
                && settings.onboarding.completed
                && let Err(error) = app.state::<Updates>().check(&app).await
            {
                tracing::info!(%error, "automatic update check");
            }
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}

#[tauri::command]
#[specta::specta]
pub fn get_update_status(state: State<'_, Updates>) -> UpdateStatus {
    state.status.lock_ok().clone()
}

/// Explicit user checks work even when automatic checks are disabled.
#[tauri::command]
#[specta::specta]
pub async fn check_for_updates(
    app: AppHandle,
    state: State<'_, Updates>,
) -> Result<UpdateStatus, CommandError> {
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
    let mut update = state
        .candidate
        .lock_ok()
        .clone()
        .filter(|update| update.version == version)
        .ok_or_else(|| CommandError::Update {
            message: "Check for updates again before installing.".into(),
        })?;
    update.timeout = Some(Duration::from_secs(10 * 60));
    let mut downloaded = 0_u64;
    let mut last_percent = None;
    state.set(
        &app,
        UpdateStatus::Downloading {
            version: version.clone(),
            percent: None,
        },
    );
    let bytes = update
        .download(
            |chunk, total| {
                downloaded = downloaded.saturating_add(chunk as u64);
                let percent = total
                    .filter(|total| *total > 0)
                    .map(|total| (downloaded.saturating_mul(100) / total).min(100) as u8);
                if percent != last_percent {
                    last_percent = percent;
                    state.set(
                        &app,
                        UpdateStatus::Downloading {
                            version: version.clone(),
                            percent,
                        },
                    );
                }
            },
            || {},
        )
        .await
        .map_err(|e| state.failed(&app, e))?;
    state.set(
        &app,
        UpdateStatus::Installing {
            version: version.clone(),
        },
    );
    // Extraction and native authorization can block. Keep them off the async executor.
    tauri::async_runtime::spawn_blocking(move || update.install(bytes))
        .await
        .map_err(|e| state.failed(&app, e))?
        .map_err(|e| state.failed(&app, e))?;
    *state.candidate.lock_ok() = None;
    state.set(&app, UpdateStatus::Ready { version });
    // Tauri delivers RunEvent::Exit, flushing history and stopping remote services.
    app.request_restart();
    Ok(())
}

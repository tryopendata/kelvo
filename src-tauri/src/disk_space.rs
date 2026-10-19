//! Disk space report commands (plan/disk-space-report.md). Local only: there is no host
//! parameter, and the UI offers the view only for this Mac. The service lives in
//! `kelvo-engine`; this module adapts it to commands and Reveal in Finder.
use kelvo_engine::disk_space::SpaceError;
use kelvo_schema::disk_space::{
    SpaceFile, SpaceReport, SpaceScanStatus, SpaceSubtree, SpaceTarget, SpaceVolume,
};
use tauri::State;

use crate::error::CommandError;
use crate::state::AppState;

impl From<SpaceError> for CommandError {
    fn from(e: SpaceError) -> Self {
        Self::DiskSpace {
            message: e.to_string(),
        }
    }
}

/// Whether this build can scan (false in the sandboxed App Store edition).
pub const fn available() -> bool {
    cfg!(not(feature = "appstore"))
}

fn guard() -> Result<(), CommandError> {
    if available() {
        Ok(())
    } else {
        Err(CommandError::DiskSpace {
            message: "Space reports are not available in this edition".into(),
        })
    }
}

#[tauri::command]
#[specta::specta]
pub async fn disk_space_volumes(
    state: State<'_, AppState>,
) -> Result<Vec<SpaceVolume>, CommandError> {
    guard()?;
    let service = state.disk_space.clone();
    tauri::async_runtime::spawn_blocking(move || service.volumes())
        .await
        .map_err(|e| CommandError::Internal {
            message: e.to_string(),
        })
}

#[tauri::command]
#[specta::specta]
pub async fn disk_space_start(
    state: State<'_, AppState>,
    volume: String,
    include: Vec<String>,
) -> Result<SpaceScanStatus, CommandError> {
    guard()?;
    let service = state.disk_space.clone();
    tauri::async_runtime::spawn_blocking(move || service.start(&volume, &include))
        .await
        .map_err(|e| CommandError::Internal {
            message: e.to_string(),
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
#[specta::specta]
pub fn disk_space_status(state: State<'_, AppState>) -> SpaceScanStatus {
    state.disk_space.status()
}

#[tauri::command]
#[specta::specta]
pub fn disk_space_cancel(state: State<'_, AppState>) {
    state.disk_space.cancel();
}

#[tauri::command]
#[specta::specta]
pub fn disk_space_report(state: State<'_, AppState>) -> Option<SpaceReport> {
    state.disk_space.report()
}

#[tauri::command]
#[specta::specta]
pub fn disk_space_subtree(
    state: State<'_, AppState>,
    report: u32,
    node: Option<u32>,
) -> Result<SpaceSubtree, CommandError> {
    Ok(state.disk_space.subtree(report, node)?)
}

#[tauri::command]
#[specta::specta]
pub fn disk_space_largest_files(
    state: State<'_, AppState>,
    report: u32,
) -> Result<Vec<SpaceFile>, CommandError> {
    Ok(state.disk_space.largest_files(report)?)
}

/// The path of an entry of a report, as Finder shows it, for Copy path.
#[tauri::command]
#[specta::specta]
pub fn disk_space_path(
    state: State<'_, AppState>,
    report: u32,
    target: SpaceTarget,
) -> Result<String, CommandError> {
    let path = state.disk_space.reveal_path(report, target)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Shows an entry of a report in Finder. Paths come from the report, never the webview.
#[tauri::command]
#[specta::specta]
pub fn disk_space_reveal(
    state: State<'_, AppState>,
    report: u32,
    target: SpaceTarget,
) -> Result<(), CommandError> {
    let path = state.disk_space.reveal_path(report, target)?;
    if !path.exists() {
        return Err(CommandError::DiskSpace {
            message: "This item no longer exists".into(),
        });
    }
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&path)
        .status()
        .map_err(|e| CommandError::DiskSpace {
            message: format!("Couldn't open Finder: {e}"),
        })
        .and_then(|status| {
            status
                .success()
                .then_some(())
                .ok_or(CommandError::DiskSpace {
                    message: "Finder couldn't show this item".into(),
                })
        })
}

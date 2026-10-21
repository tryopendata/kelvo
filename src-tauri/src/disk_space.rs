//! Disk space report commands (plan/disk-space-report.md). This Mac's scans run on
//! `AppState::disk_space`; a remote host's run on that host and are asked for over its
//! session (D-117). The service lives in `kelvo-engine`; this module adapts it to
//! commands and Reveal in Finder, which is this Mac only.
use kelvo_engine::disk_space::SpaceError;
use kelvo_remote::RemoteError;
use kelvo_schema::HostId;
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

/// A remote host's refusal is the same `disk_space` error a local scan gives.
fn remote_error(error: RemoteError) -> CommandError {
    match error {
        RemoteError::Refused(message) => CommandError::DiskSpace { message },
        other => other.into(),
    }
}

/// The dashboard closed: drops this Mac's report and asks each remote host to drop the
/// scan this Mac started there.
pub fn dashboard_closed(state: &AppState) {
    state.disk_space.clear();
    for source in state.remotes.sources() {
        tauri::async_runtime::spawn(async move {
            let _ = source.space_clear().await;
        });
    }
}

#[tauri::command]
#[specta::specta]
pub async fn disk_space_volumes(
    state: State<'_, AppState>,
    host: HostId,
) -> Result<Vec<SpaceVolume>, CommandError> {
    guard()?;
    if host != state.local {
        let source = state.remotes.source(host)?;
        return source.space_volumes().await.map_err(remote_error);
    }
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
    host: HostId,
    volume: String,
    include: Vec<String>,
) -> Result<SpaceScanStatus, CommandError> {
    guard()?;
    if host != state.local {
        // A remote scan never enters protected locations (D-117).
        let source = state.remotes.source(host)?;
        return source.space_start(volume).await.map_err(remote_error);
    }
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
pub async fn disk_space_status(
    state: State<'_, AppState>,
    host: HostId,
) -> Result<SpaceScanStatus, CommandError> {
    if host != state.local {
        let source = state.remotes.source(host)?;
        return source.space_status().await.map_err(remote_error);
    }
    Ok(state.disk_space.status())
}

#[tauri::command]
#[specta::specta]
pub async fn disk_space_cancel(
    state: State<'_, AppState>,
    host: HostId,
) -> Result<(), CommandError> {
    if host != state.local {
        let source = state.remotes.source(host)?;
        return source.space_cancel().await.map_err(remote_error);
    }
    state.disk_space.cancel();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn disk_space_report(
    state: State<'_, AppState>,
    host: HostId,
) -> Result<Option<SpaceReport>, CommandError> {
    if host != state.local {
        let source = state.remotes.source(host)?;
        return source.space_report().await.map_err(remote_error);
    }
    Ok(state.disk_space.report())
}

#[tauri::command]
#[specta::specta]
pub async fn disk_space_subtree(
    state: State<'_, AppState>,
    host: HostId,
    report: u32,
    node: Option<u32>,
) -> Result<SpaceSubtree, CommandError> {
    if host != state.local {
        let source = state.remotes.source(host)?;
        return source
            .space_subtree(report, node)
            .await
            .map_err(remote_error);
    }
    Ok(state.disk_space.subtree(report, node)?)
}

#[tauri::command]
#[specta::specta]
pub async fn disk_space_largest_files(
    state: State<'_, AppState>,
    host: HostId,
    report: u32,
) -> Result<Vec<SpaceFile>, CommandError> {
    if host != state.local {
        let source = state.remotes.source(host)?;
        return source
            .space_largest_files(report)
            .await
            .map_err(remote_error);
    }
    Ok(state.disk_space.largest_files(report)?)
}

/// The path of an entry of a report, as the host shows it, for Copy path.
#[tauri::command]
#[specta::specta]
pub async fn disk_space_path(
    state: State<'_, AppState>,
    host: HostId,
    report: u32,
    target: SpaceTarget,
) -> Result<String, CommandError> {
    if host != state.local {
        let source = state.remotes.source(host)?;
        return source
            .space_path(report, target)
            .await
            .map_err(remote_error);
    }
    let path = state.disk_space.reveal_path(report, target)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Shows an entry of this Mac's report in Finder. Paths come from the report, never the
/// webview.
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

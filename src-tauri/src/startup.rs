//! Startup UI policy and one-use update restart markers shared with native acceptance.
use crate::updates::cache;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::AppHandle;

/// Remote setup and explicit background launches do not open interactive windows.
pub fn deferred_ui_launch(args: impl IntoIterator<Item = impl AsRef<str>>) -> bool {
    args.into_iter()
        .any(|arg| matches!(arg.as_ref(), "--remote-setup" | "--background"))
}

pub fn show_initial_windows(
    background_restart: bool,
    deferred: bool,
    launched_at_login: bool,
) -> bool {
    !background_restart && !deferred && !launched_at_login
}

#[derive(Serialize, Deserialize)]
struct Restart {
    version: String,
    created: u64,
}

pub fn mark_background_restart(app: &AppHandle, version: &str) -> anyhow::Result<()> {
    let restart = Restart {
        version: version.into(),
        created: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    };
    cache::write_private(
        &cache::directory(app)?.join("background-restart.json"),
        &serde_json::to_vec(&restart)?,
    )
}

pub fn clear_background_restart(app: &AppHandle) {
    if let Ok(directory) = cache::directory(app) {
        let _ = fs::remove_file(directory.join("background-restart.json"));
    }
}

/// Consume once before deciding startup UI, including when the old process originally
/// launched normally and later installed an update while running in the menu bar.
pub fn take_background_restart(app: &AppHandle) -> bool {
    let Ok(directory) = cache::directory(app) else {
        return false;
    };
    let path = directory.join("background-restart.json");
    let bytes = cache::read_bounded(&path, 4096);
    let _ = fs::remove_file(&path);
    let Ok(restart) = bytes.and_then(|bytes| Ok(serde_json::from_slice::<Restart>(&bytes)?)) else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    restart.version == env!("CARGO_PKG_VERSION")
        && now.saturating_sub(restart.created) < 10 * 60
        && now >= restart.created
}

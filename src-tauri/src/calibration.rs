//! Learned CPU power scales across restarts (D-065).
//!
//! The SMC CPU power reading is scaled live to the PMP counters (D-054), but those
//! counters can go half an hour without moving, so a fresh session would show values about
//! 25% low until two of them do. The engine seeds each session from the scale an earlier
//! one learned for the same chip, read through [`kelvo_engine::ScaleStore`], which this
//! file implements. `kelvo-collect` never writes files.
//!
//! The file is `power-calibration.json` in the app data directory, separate from
//! `settings.json`: it is not a setting, no window reads it, and losing it only costs one
//! session's seed. Every save rewrites it through a temp file and a rename, so a crash
//! leaves the old file or the new one. A missing or unreadable file is an empty map.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use kelvo_engine::ScaleStore;
use kelvo_schema::lock::LockExt;
use serde::{Deserialize, Serialize};

/// The file name in the app data directory.
pub const FILE_NAME: &str = "power-calibration.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Contents {
    /// Smoothed SMC-to-PMP scale per chip brand string ("Apple M3 Max").
    #[serde(default)]
    cpu_power_scale: BTreeMap<String, f64>,
}

/// [`ScaleStore`] over a JSON file. Loaded once at start; each save updates the map and
/// rewrites the file.
pub struct FileScaleStore {
    path: PathBuf,
    contents: Mutex<Contents>,
}

impl FileScaleStore {
    /// Reads `dir/power-calibration.json`, or starts empty when it is missing or invalid.
    pub fn open(dir: &Path) -> Self {
        let path = dir.join(FILE_NAME);
        // Owner-only (D-074): new files are written 0600; one from before is tightened.
        if let Err(e) = kelvo_store::restrict(&path) {
            tracing::warn!("restricting {}: {e}", path.display());
        }
        let contents = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!("{} is invalid ({e}); starting uncalibrated", path.display());
                Contents::default()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Contents::default(),
            Err(e) => {
                tracing::warn!("reading {}: {e}; starting uncalibrated", path.display());
                Contents::default()
            }
        };
        Self {
            path,
            contents: Mutex::new(contents),
        }
    }

    fn write(&self, contents: &Contents) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(contents).map_err(std::io::Error::other)?;
        let tmp = self.path.with_extension("json.tmp");
        kelvo_store::write_private(&tmp, &json)?;
        std::fs::rename(&tmp, &self.path)
    }
}

impl ScaleStore for FileScaleStore {
    fn load(&self, chip: &str) -> Option<f64> {
        let contents = self.contents.lock_ok();
        contents.cpu_power_scale.get(chip).copied()
    }

    fn save(&self, chip: &str, scale: f64) {
        if !scale.is_finite() {
            return;
        }
        let mut contents = self.contents.lock_ok();
        contents.cpu_power_scale.insert(chip.to_owned(), scale);
        match self.write(&contents) {
            Ok(()) => tracing::debug!(chip, scale, "saved the CPU power calibration"),
            Err(e) => tracing::warn!("writing {}: {e}", self.path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kelvo-calib-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_saved_scale_is_there_after_a_restart() {
        let dir = temp_dir();
        let first = FileScaleStore::open(&dir);
        assert_eq!(first.load("Apple M3 Max"), None);
        first.save("Apple M3 Max", 1.31);
        first.save("Apple M4 Pro", 1.1);
        first.save("Apple M3 Max", 1.29);
        drop(first);

        let second = FileScaleStore::open(&dir);
        assert_eq!(second.load("Apple M3 Max"), Some(1.29));
        assert_eq!(second.load("Apple M4 Pro"), Some(1.1));
        assert!(!dir.join("power-calibration.json.tmp").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_or_corrupt_file_starts_empty_and_is_replaced_on_save() {
        let dir = temp_dir();
        std::fs::write(dir.join(FILE_NAME), b"{ not json").unwrap();
        let store = FileScaleStore::open(&dir);
        assert_eq!(store.load("Apple M3 Max"), None);
        store.save("Apple M3 Max", 1.3);
        assert_eq!(FileScaleStore::open(&dir).load("Apple M3 Max"), Some(1.3));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// D-074: owner-only when written, and an older looser file is tightened on open.
    #[test]
    fn the_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = temp_dir();
        let path = dir.join(FILE_NAME);
        FileScaleStore::open(&dir).save("Apple M3 Max", 1.3);
        assert_eq!(mode(&path), 0o600);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        drop(FileScaleStore::open(&dir));
        assert_eq!(mode(&path), 0o600);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn non_finite_scales_are_not_saved() {
        let dir = temp_dir();
        let store = FileScaleStore::open(&dir);
        store.save("Apple M3 Max", f64::NAN);
        assert_eq!(store.load("Apple M3 Max"), None);
        assert!(!dir.join(FILE_NAME).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

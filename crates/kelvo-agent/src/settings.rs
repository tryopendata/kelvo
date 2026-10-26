//! The agent's settings: [`Settings::agent_default`] with the data directory's
//! `settings.json` merged over it.
//!
//! The file holds only what `kelvo-agent configure` was given (merged across calls), not
//! a full copy, so a later build's defaults still apply to everything the controller
//! never set. The daemon reads it once at start; a change applies on restart.
use std::path::Path;

use anyhow::{Context, bail};
use kelvo_schema::Settings;
use serde_json::Value;

pub const SETTINGS_FILE: &str = "settings.json";

/// The settings `run` starts with. A missing file is the defaults; an unreadable or
/// invalid one is logged and also the defaults, so a bad file never stops the agent.
pub fn load(data_dir: &Path) -> Settings {
    let path = data_dir.join(SETTINGS_FILE);
    let overlay = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Settings::agent_default(),
        Err(e) => {
            tracing::warn!(path = %path.display(), "reading settings: {e}; using the defaults");
            return Settings::agent_default();
        }
    };
    match serde_json::from_slice(&overlay)
        .context("parsing")
        .and_then(|overlay| resolve(&overlay))
    {
        Ok(settings) => settings,
        Err(e) => {
            tracing::warn!(path = %path.display(), "invalid settings ({e:#}); using the defaults");
            Settings::agent_default()
        }
    }
}

/// Updaters fail closed on unreadable or invalid preferences. Only absence means
/// the default policy; daemon startup keeps its existing availability behavior.
pub fn load_strict(data_dir: &Path) -> anyhow::Result<Settings> {
    let path = data_dir.join(SETTINGS_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => resolve(&serde_json::from_slice(&bytes).context("parsing update settings")?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::agent_default()),
        Err(e) => Err(e).context("reading update settings"),
    }
}

/// The agent defaults with `overlay`, a JSON object holding any subset of [`Settings`],
/// merged over them, validated.
pub fn resolve(overlay: &Value) -> anyhow::Result<Settings> {
    if !overlay.is_object() {
        bail!("settings must be a JSON object");
    }
    let mut merged =
        serde_json::to_value(Settings::agent_default()).context("encoding the defaults")?;
    merge(&mut merged, overlay);
    let settings: Settings = serde_json::from_value(merged).context("decoding settings")?;
    settings.validate().context("validating settings")?;
    Ok(settings)
}

/// `configure '<json>'`: merges `patch` into the file's overlay, checks the result and
/// writes it back atomically. Returns the settings the daemon will start with.
pub fn configure(data_dir: &Path, patch: &str) -> anyhow::Result<Settings> {
    let patch: Value = serde_json::from_str(patch).context("parsing the settings JSON")?;
    if !patch.is_object() {
        bail!("settings must be a JSON object");
    }
    let path = data_dir.join(SETTINGS_FILE);
    let mut overlay = match std::fs::read(&path) {
        // A broken file is replaced rather than blocking the controller's repair.
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
            .ok()
            .filter(Value::is_object)
            .unwrap_or_else(|| Value::Object(serde_json::Map::new())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Object(serde_json::Map::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    merge(&mut overlay, &patch);
    let settings = resolve(&overlay)?;
    let bytes = serde_json::to_vec_pretty(&overlay).context("encoding settings")?;
    kelvo_store::create_private_dir(data_dir)
        .with_context(|| format!("creating {}", data_dir.display()))?;
    let tmp = path.with_extension("json.tmp");
    kelvo_store::write_private(&tmp, &bytes)
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("renaming to {}", path.display()))?;
    Ok(settings)
}

/// Recursively merges objects; any other value in `patch` replaces the one in `base`.
fn merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(base), Value::Object(patch)) => {
            for (key, value) in patch {
                match base.get_mut(key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        base.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base, patch) => *base = patch.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelvo_schema::Module;
    use serde_json::json;

    #[test]
    fn automatic_updates_fail_closed_and_re_read_policy() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            load_strict(dir.path())
                .unwrap()
                .general
                .auto_install_updates
        );
        configure(
            dir.path(),
            r#"{"general":{"check_updates":false,"auto_install_updates":false}}"#,
        )
        .unwrap();
        let disabled = load_strict(dir.path()).unwrap();
        assert!(!disabled.general.check_updates && !disabled.general.auto_install_updates);
        std::fs::write(dir.path().join(SETTINGS_FILE), "broken json").unwrap();
        assert!(load_strict(dir.path()).is_err());
        // The daemon still starts with defaults; the updater must never use that fallback.
        assert_eq!(load(dir.path()), Settings::agent_default());
    }

    #[test]
    fn missing_file_is_the_agent_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), Settings::agent_default());
    }

    #[test]
    fn partial_overlay_changes_only_its_fields() {
        let s = resolve(&json!({"history": {"retention_days": 7, "size_limit_mb": 300}})).unwrap();
        assert_eq!(s.history.retention_days, 7);
        assert_eq!(s.history.size_limit_mb, 300);
        let mut expected = Settings::agent_default();
        expected.history.retention_days = 7;
        expected.history.size_limit_mb = 300;
        assert_eq!(s, expected);
    }

    #[test]
    fn invalid_values_are_rejected() {
        assert!(resolve(&json!({"history": {"retention_days": 8}})).is_err());
        assert!(resolve(&json!({"history": {"size_limit_mb": 1}})).is_err());
        assert!(resolve(&json!({"sampling": {"interval_ms": "fast"}})).is_err());
        assert!(resolve(&json!([1, 2])).is_err());
    }

    #[test]
    fn configure_merges_across_calls_and_load_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        configure(&data, r#"{"history": {"retention_days": 90}}"#).unwrap();
        configure(&data, r#"{"history": {"size_limit_mb": 500}}"#).unwrap();
        let s = load(&data);
        assert_eq!(s.history.retention_days, 90);
        assert_eq!(s.history.size_limit_mb, 500);
        // Only what was configured is stored, so other defaults can still move.
        let stored: Value =
            serde_json::from_slice(&std::fs::read(data.join(SETTINGS_FILE)).unwrap()).unwrap();
        assert_eq!(
            stored,
            json!({"history": {"retention_days": 90, "size_limit_mb": 500}})
        );
        assert!(s.module(Module::Disk).unwrap().enabled);
    }

    #[test]
    fn rejected_configure_leaves_the_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        configure(dir.path(), r#"{"history": {"retention_days": 7}}"#).unwrap();
        let before = std::fs::read(dir.path().join(SETTINGS_FILE)).unwrap();
        assert!(configure(dir.path(), r#"{"history": {"retention_days": 9}}"#).is_err());
        assert!(configure(dir.path(), "not json").is_err());
        assert!(configure(dir.path(), "42").is_err());
        assert_eq!(
            std::fs::read(dir.path().join(SETTINGS_FILE)).unwrap(),
            before
        );
    }

    #[test]
    fn invalid_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE);
        std::fs::write(&path, r#"{"history": {"retention_days": 3}}"#).unwrap();
        assert_eq!(load(dir.path()), Settings::agent_default());
        std::fs::write(&path, "{").unwrap();
        assert_eq!(load(dir.path()), Settings::agent_default());
    }
}

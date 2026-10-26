use super::*;
use tauri_plugin_updater::UpdaterExt;

const PAYLOAD: &[u8] = include_bytes!("../../tests/fixtures/update-payload");
const SIGNATURE: &str = include_str!("../../tests/fixtures/update-payload.sig");

fn candidate() -> Candidate {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    context
        .config_mut()
        .plugins
        .0
        .insert("updater".into(), config["plugins"]["updater"].clone());
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .unwrap();
    let updater = app
        .updater_builder()
        .target("darwin-aarch64")
        .executable_path("runtime-path/app")
        .build()
        .unwrap();
    let update = updater.restore_update(serde_json::json!({
        "version": "99.0.0",
        "platforms": { "darwin-aarch64": { "url": "https://example.invalid/package", "signature": SIGNATURE.trim() } }
    })).unwrap().unwrap();
    Candidate {
        update,
        artifact: kelvo_update::Artifact {
            url: "https://example.invalid/package".into(),
            size: PAYLOAD.len() as u64,
            sha256: format!("{:x}", Sha256::digest(PAYLOAD)),
            signature: Some(SIGNATURE.into()),
        },
        manifest: vec![],
        signature: String::new(),
    }
}

#[test]
fn cached_package_is_reverified_before_installation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("package.tar.gz");
    let mut candidate = candidate();
    write_private(&path, PAYLOAD).unwrap();
    assert_eq!(verified_package(&path, &candidate).unwrap(), PAYLOAD);
    candidate.update.version = "9.0.0".into();
    assert!(
        verified_package(&path, &candidate).is_err(),
        "a valid package cannot be relabeled"
    );
    candidate.update.version = "99.0.0".into();
    write_private(&path, b"evil").unwrap();
    assert!(
        verified_package(&path, &candidate).is_err(),
        "modified content fails its signed digest"
    );
    candidate.artifact.sha256 = format!("{:x}", Sha256::digest(b"evil"));
    candidate.artifact.size = 4;
    assert!(
        verified_package(&path, &candidate).is_err(),
        "even replaced local digest cannot bypass archive signature"
    );
    write_private(&path, b"test extra").unwrap();
    assert!(verified_package(&path, &candidate).is_err());
}

#[test]
fn cache_is_private_and_rejects_oversized_or_redirected_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("updates");
    private_directory(&path).unwrap();
    let file = path.join("metadata");
    write_private(&file, b"verified").unwrap();
    assert_eq!(read_bounded(&file, 8).unwrap(), b"verified");
    assert!(read_bounded(&file, 7).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = path.join("link");
        symlink(&file, &link).unwrap();
        assert!(read_bounded(&link, 8).is_err());
    }
}

struct FileSettings(std::path::PathBuf);
impl crate::settings::SettingsFile for FileSettings {
    fn load(&self) -> Option<serde_json::Value> {
        Some(serde_json::from_slice(&std::fs::read(&self.0).unwrap()).unwrap())
    }
    fn save(&self, value: serde_json::Value) -> Result<(), String> {
        std::fs::write(&self.0, serde_json::to_vec(&value).unwrap()).map_err(|e| e.to_string())
    }
}
struct NativeEffects;
impl crate::settings::SettingsEffects for NativeEffects {
    fn prepare(
        &self,
        _: &kelvo_schema::Settings,
        _: &kelvo_schema::Settings,
    ) -> Result<(), crate::CommandError> {
        Ok(())
    }
    fn committed(&self, _: &kelvo_schema::Settings, _: &crate::ipc::SettingsSnapshot) {}
}

#[test]
fn changes_after_package_verification_prevent_automatic_installation() {
    use crate::{
        settings::{GeneralPatch, SettingsOwner, SettingsPatch},
        updates::Updates,
    };
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("package");
    write_private(&package, PAYLOAD).unwrap();
    let candidate = candidate();
    let settings_path = temp.path().join("settings.json");
    let mut initial = kelvo_schema::Settings::default();
    initial.onboarding.completed = true;
    std::fs::write(&settings_path, serde_json::to_vec(&initial).unwrap()).unwrap();
    let settings = SettingsOwner::load(Box::new(FileSettings(settings_path)));
    let owner = Updates::default();
    // Only native window visibility is faked; policy uses the actual settings owner.
    let visible = std::cell::Cell::new(false);
    let eligible = || {
        let current = settings.settings();
        current.general.check_updates
            && current.general.auto_install_updates
            && current.onboarding.completed
            && !visible.get()
    };
    assert!(eligible());
    verified_package(&package, &candidate).unwrap();
    let patch = |enabled| SettingsPatch {
        general: Some(GeneralPatch {
            auto_install_updates: Some(enabled),
            ..Default::default()
        }),
        ..Default::default()
    };
    settings.update(&patch(false), &NativeEffects).unwrap();
    assert!(
        owner.installation_guard(true, eligible).is_err(),
        "opt-out after download must win"
    );
    settings.update(&patch(true), &NativeEffects).unwrap();
    visible.set(true);
    assert!(
        owner.installation_guard(true, eligible).is_err(),
        "a newly opened window must win"
    );
    visible.set(false);
    let maintenance = owner.maintenance.clone().try_read_owned().unwrap();
    assert!(
        owner.installation_guard(true, eligible).is_err(),
        "maintenance after verification must finish first"
    );
    drop(maintenance);
    let committed = owner.installation_guard(true, eligible).unwrap();
    assert!(
        owner.maintenance.clone().try_read_owned().is_err(),
        "new mutation is excluded through restart"
    );
    drop(committed);
    settings.update(&patch(false), &NativeEffects).unwrap();
    visible.set(true);
    assert!(
        owner.installation_guard(false, eligible).is_ok(),
        "explicit installation works when automatic updates are disabled"
    );
}

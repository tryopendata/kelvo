#![allow(clippy::unwrap_used)] // Test fixture setup should fail immediately on invalid input.
//! Exercise the real updater with Kelvo's committed public key and an inert signed
//! fixture. No network, installed app, or user data is touched.
use tauri_plugin_updater::UpdaterExt;

fn updater() -> tauri_plugin_updater::Updater {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let key = config["plugins"]["updater"]["pubkey"].as_str().unwrap();
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    context
        .config_mut()
        .plugins
        .0
        .insert("updater".into(), config["plugins"]["updater"].clone());
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().pubkey(key).build())
        .build(context)
        .unwrap();
    app.updater_builder()
        .target("darwin-aarch64")
        .executable_path("/Applications/Kelvo.app/Contents/MacOS/kelvo")
        .build()
        .unwrap()
}

fn manifest(version: &str) -> serde_json::Value {
    serde_json::json!({
        "version": version,
        "platforms": { "darwin-aarch64": {
            "url": "https://example.invalid/Kelvo.app.tar.gz",
            "signature": include_str!("fixtures/update-payload.sig").trim(),
        } }
    })
}

#[test]
fn updater_accepts_authentic_bytes_and_rejects_tampering_before_install() {
    let updater = updater();
    let update = updater.restore_update(manifest("99.0.0")).unwrap().unwrap();
    update
        .verify(include_bytes!("fixtures/update-payload"))
        .unwrap();
    assert!(update.verify(b"modified download").is_err());
    // install verifies too, before interpreting the archive or touching disk.
    assert!(update.install(b"modified download").is_err());
    let relabelled = updater.restore_update(manifest("99.0.1")).unwrap().unwrap();
    assert!(
        relabelled
            .verify(include_bytes!("fixtures/update-payload"))
            .is_err()
    );
}

#[test]
fn release_config_requires_a_signed_version_and_https() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    assert_eq!(config["plugins"]["updater"]["requireSignedVersion"], true);
    assert_eq!(
        config["plugins"]["updater"]["endpoints"],
        serde_json::json!([
            "https://github.com/tryopendata/kelvo/releases/latest/download/latest.json"
        ])
    );
}

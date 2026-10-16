//! Verify and install the real updater archive into an isolated temporary bundle.
//! Never launches Kelvo or touches an installed app or its data.
use std::{path::PathBuf, process::Command};

use anyhow::{Context, bail, ensure};
use tauri_plugin_updater::UpdaterExt;

fn main() -> anyhow::Result<()> {
    let assets = PathBuf::from(
        std::env::args()
            .nth(1)
            .context("pass the release-assets directory")?,
    );
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))?;
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    context
        .config_mut()
        .plugins
        .0
        .insert("updater".into(), config["plugins"]["updater"].clone());
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)?;
    let temporary = tempfile::tempdir()?;
    let bundle = temporary.path().join("Kelvo.app");
    let executable = bundle.join("Contents/MacOS/kelvo");
    std::fs::create_dir_all(executable.parent().context("executable parent")?)?;
    std::fs::write(&executable, b"old version placeholder")?;
    let updater = app.updater_builder()
        .executable_path(&executable)
        .target("darwin-aarch64")
        // This validates the current release, not a newer one.
        .version_comparator(|_, _| true)
        .build()?;
    let metadata = serde_json::from_slice(&std::fs::read(assets.join("latest.json"))?)?;
    let update = updater
        .restore_update(metadata)?
        .context("missing release")?;
    let archive = std::fs::read(assets.join("Kelvo.app.tar.gz"))?;
    update
        .verify(&archive)
        .context("release signature or signed version mismatch")?;
    let mut tampered = archive.clone();
    if let Some(byte) = tampered.first_mut() {
        *byte ^= 1;
    } else {
        bail!("empty archive");
    }
    ensure!(
        update.install(tampered).is_err(),
        "tampered archive was accepted"
    );
    ensure!(
        std::fs::read(&executable)? == b"old version placeholder",
        "failed verification changed installed app"
    );
    update
        .install(archive)
        .context("installing into temporary bundle")?;
    ensure!(
        bundle.join("Contents/MacOS/kelvo-remote-helper").is_file(),
        "remote helper missing"
    );
    ensure!(
        Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&bundle)
            .status()?
            .success(),
        "installed bundle signature invalid"
    );
    let quarantine = Command::new("xattr")
        .args(["-p", "com.apple.quarantine"])
        .arg(&bundle)
        .output()?;
    println!(
        "Verified signed release {}, rejected tampering without changing the old bundle, installed app and helper, verified installed code signature. Quarantine attribute present: {}",
        update.version,
        quarantine.status.success()
    );
    Ok(())
}

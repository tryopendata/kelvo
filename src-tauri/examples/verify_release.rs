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
    let catalog = verify_catalog(&assets)?;
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
    let metadata = legacy_metadata(&assets, &catalog)?;
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
    verify_agents(&catalog, &bundle.join("Contents/Resources/agent"))?;
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

fn verify_catalog(assets: &std::path::Path) -> anyhow::Result<kelvo_update::VerifiedRelease> {
    let catalog = kelvo_update::verify_manifest(
        &std::fs::read(assets.join("release.json"))?,
        &std::fs::read_to_string(assets.join("release.json.sig"))?,
    )?;
    ensure!(
        catalog.release().version == env!("CARGO_PKG_VERSION"),
        "catalog version mismatch"
    );
    ensure!(
        catalog.release().protocol
            == kelvo_update::ProtocolRange {
                min: kelvo_proto::MIN_COMPATIBLE,
                max: kelvo_proto::PROTO_VERSION,
            },
        "catalog protocol mismatch"
    );
    ensure!(
        catalog.release().artifacts.len() == 3,
        "catalog must advertise exactly the three shipped targets"
    );
    for (target, filename) in [
        ("darwin-aarch64", "Kelvo.app.tar.gz"),
        ("linux-aarch64", "kelvo-agent-aarch64-unknown-linux-musl"),
        ("linux-x86_64", "kelvo-agent-x86_64-unknown-linux-musl"),
    ] {
        let artifact = catalog
            .release()
            .artifacts
            .get(target)
            .context("missing release target")?;
        kelvo_update::verify_artifact(artifact, &assets.join(filename))?;
    }
    Ok(catalog)
}

fn legacy_metadata(
    assets: &std::path::Path,
    catalog: &kelvo_update::VerifiedRelease,
) -> anyhow::Result<serde_json::Value> {
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(assets.join("latest.json"))?)?;
    let artifact = catalog
        .release()
        .artifacts
        .get("darwin-aarch64")
        .context("missing Mac target")?;
    let platform = &metadata["platforms"]["darwin-aarch64"];
    ensure!(
        metadata["version"].as_str() == Some(catalog.release().version.as_str())
            && platform["url"].as_str() == Some(artifact.url.as_str())
            && platform["signature"].as_str() == artifact.signature.as_deref(),
        "legacy updater metadata disagrees with the signed catalog"
    );
    Ok(metadata)
}

fn verify_agents(
    catalog: &kelvo_update::VerifiedRelease,
    directory: &std::path::Path,
) -> anyhow::Result<()> {
    for (target, filename) in [
        ("linux-aarch64", "kelvo-agent-aarch64-unknown-linux-musl"),
        ("linux-x86_64", "kelvo-agent-x86_64-unknown-linux-musl"),
    ] {
        let artifact = catalog
            .release()
            .artifacts
            .get(target)
            .context("missing agent target")?;
        kelvo_update::verify_artifact(artifact, &directory.join(filename))
            .context("bundled agent differs from the signed standalone release")?;
    }
    Ok(())
}

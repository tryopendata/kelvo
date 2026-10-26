//! A host-owned update, launched by systemd independently of the SSH connection.
mod health;
mod install;
mod stage;
pub(crate) mod transaction;
mod trial;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, ensure};
use kelvo_remote::control::RemoteUpdateStatus;
use kelvo_remote::{AgentLayout, AgentPaths};
use kelvo_update::Client;

use crate::{maintenance, settings};
use transaction::{Phase, directory, durable_write};

const SERVICE: &str = "kelvo-agent-update.service";
const COLLECTOR: &str = "kelvo-agent.service";
const TIMER: &str = "kelvo-agent-update.timer";

pub fn status(paths: &AgentPaths) -> anyhow::Result<RemoteUpdateStatus> {
    let settings = settings::load_strict(&paths.data_dir)?;
    let saved = std::fs::read(directory(paths).join("result.json"));
    let mut result = match saved {
        Ok(bytes) => serde_json::from_slice(&bytes).context("reading last update result")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => RemoteUpdateStatus {
            supported: true,
            check_updates: true,
            auto_install_updates: true,
            status: "idle".into(),
            version: None,
            message: None,
        },
        Err(e) => return Err(e).context("reading update state"),
    };
    result.check_updates = settings.general.check_updates;
    result.auto_install_updates = settings.general.auto_install_updates;
    Ok(result)
}

fn report(
    paths: &AgentPaths,
    state: &str,
    version: Option<&str>,
    message: Option<String>,
) -> anyhow::Result<()> {
    let settings = settings::load_strict(&paths.data_dir)?;
    let result = RemoteUpdateStatus {
        supported: true,
        check_updates: settings.general.check_updates,
        auto_install_updates: settings.general.auto_install_updates,
        status: state.into(),
        version: version.map(str::to_owned),
        message,
    };
    durable_write(
        &directory(paths).join("result.json"),
        &serde_json::to_vec(&result)?,
    )
}

pub async fn request(layout: AgentLayout, paths: &AgentPaths) -> anyhow::Result<()> {
    let guard = maintenance::lock(paths)?;
    durable_write(&directory(paths).join("requested"), b"manual\n")?;
    drop(guard);
    maintenance::systemctl(layout, &["--no-block", "start", SERVICE]).await
}

/// Never run a transaction in an SSH-owned process. systemd owns its process group,
/// kills the trial child on failure, and runs the independent recovery executable.
pub async fn scheduled(layout: AgentLayout, paths: &AgentPaths) -> anyhow::Result<()> {
    ensure!(
        std::env::var_os("INVOCATION_ID").is_some(),
        "updates must run in the systemd updater service; use update --request"
    );
    let guard = maintenance::lock(paths)?;
    ensure!(
        transaction::read(paths)?.is_none(),
        "an interrupted update needs recovery first"
    );
    let requested = directory(paths).join("requested");
    let manual = requested.exists();
    if manual {
        std::fs::remove_file(&requested)?;
    }
    let policy = settings::load_strict(&paths.data_dir)?;
    if !manual && !policy.general.check_updates {
        return report(
            paths,
            "idle",
            None,
            Some("Automatic installation is disabled".into()),
        );
    }
    if !maintenance::active(layout).await {
        return report(
            paths,
            "deferred",
            None,
            Some("Collector is stopped or disabled".into()),
        );
    }
    stage::clean(paths)?;
    drop(guard);
    let result = check_and_install(layout, paths, manual).await;
    if let Err(error) = &result {
        let version = transaction::read(paths)?.map(|journal| journal.version);
        report(
            paths,
            "failed",
            version.as_deref(),
            Some(format!("{error:#}")),
        )?;
    }
    // ExecStopPost also performs this after a killed updater; doing it here gives
    // manual callers a complete result without depending on their SSH connection.
    recover(layout, paths, true).await?;
    result
}

async fn check_and_install(
    layout: AgentLayout,
    paths: &AgentPaths,
    manual: bool,
) -> anyhow::Result<()> {
    let lock = maintenance::lock(paths)?;
    let installed = install::installed_version(paths).await?;
    let identity = health::hello(&paths.socket).await?;
    let held = std::fs::read_to_string(directory(paths).join("failed-version")).ok();
    report(paths, "checking", None, None)?;
    drop(lock);
    let client = Client::production()?;
    let release = client.fetch().await?;
    let version = &release.release().version;
    let target = if cfg!(target_arch = "aarch64") {
        "linux-aarch64"
    } else {
        "linux-x86_64"
    };
    let candidate = release.candidate(&installed.version, install::protocol(&installed), target);
    let artifact = match candidate {
        Ok(Some(artifact)) => artifact,
        Ok(None) => return report(paths, "idle", None, None),
        Err(kelvo_update::Error::Incompatible { .. }) => return report(paths, "compatibility_blocked", Some(version), Some("This release needs a coordinated upgrade. Update connected controllers, then reinstall this agent while keeping its data. Update agent cannot apply it.".into())),
        Err(e) => return report(paths, "deferred", Some(version), Some(e.to_string())),
    };
    if !manual && held.as_ref() == Some(version) {
        return report(
            paths,
            "failed",
            Some(version),
            Some("This candidate failed its trial; request an update to retry".into()),
        );
    }
    let lock = maintenance::lock(paths)?;
    ensure!(
        install::installed_version(paths).await? == installed,
        "installed agent changed while checking for updates"
    );
    let policy = settings::load_strict(&paths.data_dir)?;
    if !manual && (!policy.general.check_updates || !policy.general.auto_install_updates) {
        return report(
            paths,
            "waiting",
            Some(version),
            Some("Update available; automatic installation is disabled".into()),
        );
    }
    disk_space(paths, artifact.size)?;
    report(paths, "downloading", Some(version), None)?;
    let stage = stage::Stage::new(paths)?;
    drop(lock);
    client.download(artifact, stage.path()).await?;
    std::fs::set_permissions(stage.path(), std::fs::Permissions::from_mode(0o755))?;
    let _guard = maintenance::lock(paths)?;
    let policy = settings::load_strict(&paths.data_dir)?;
    if !manual && (!policy.general.check_updates || !policy.general.auto_install_updates) {
        return report(
            paths,
            "idle",
            Some(version),
            Some("Automatic installation was disabled".into()),
        );
    }
    ensure!(
        maintenance::active(layout).await,
        "collector was stopped or disabled during download"
    );
    disk_space(paths, artifact.size)?;
    let before = health::hello(&paths.socket).await?;
    ensure!(
        !before.db_instance_uuid.is_nil(),
        "history unavailable; automatic update deferred"
    );
    ensure!(
        install::installed_version(paths).await? == installed,
        "installed agent changed during download; refusing to overwrite it"
    );
    ensure!(
        before.host.id == identity.host.id && before.db_instance_uuid == identity.db_instance_uuid,
        "host identity or history changed during download"
    );
    install::install(
        layout,
        paths,
        artifact,
        stage.path(),
        &before,
        version,
        release.release().protocol,
    )
    .await
}

/// Invoked by the preserved executable before every collector start and after the
/// updater's process group has been stopped. Never runs under the collector sandbox.
pub async fn recover(layout: AgentLayout, paths: &AgentPaths, restart: bool) -> anyhow::Result<()> {
    let guard = maintenance::lock(paths)?;
    trial::stop(layout).await?;
    let recovered = transaction::recover(paths)?;
    let resume = match std::fs::read(directory(paths).join("restart-required")) {
        Ok(bytes) => Some(
            serde_json::from_slice::<transaction::Journal>(&bytes)
                .context("reading update restart intent")?,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).context("reading update restart intent"),
    };
    if let Some(journal) = recovered.as_ref().or(resume.as_ref()) {
        restore_context(&paths.binary).await?;
        if journal.phase != Phase::Committed {
            durable_write(
                &directory(paths).join("failed-version"),
                journal.version.as_bytes(),
            )?;
            report(
                paths,
                "failed",
                Some(&journal.version),
                Some("Interrupted or failed update was rolled back".into()),
            )?;
        } else {
            report(paths, "updated", Some(&journal.version), None)?;
        }
    }
    drop(guard);
    if restart && directory(paths).join("restart-required").exists() {
        maintenance::systemctl(layout, &["daemon-reload"]).await?;
        maintenance::systemctl(layout, &["start", COLLECTOR]).await?;
    }
    Ok(())
}

async fn restore_context(path: &Path) -> anyhow::Result<()> {
    match tokio::process::Command::new("restorecon")
        .arg("-F")
        .arg(path)
        .output()
        .await
    {
        Ok(output) => ensure!(
            output.status.success(),
            "restoring agent SELinux context: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).context("restoring agent SELinux context"),
    }
    Ok(())
}

fn disk_space(paths: &AgentPaths, binary_bytes: u64) -> anyhow::Result<()> {
    disk_space_with(paths, binary_bytes, &kelvo_store::StatVfs)
}

fn disk_space_with(
    paths: &AgentPaths,
    binary_bytes: u64,
    volume: &impl kelvo_store::FreeSpace,
) -> anyhow::Result<()> {
    let history_bytes = ["history.sqlite", "history.sqlite-wal", "history.sqlite-shm"]
        .into_iter()
        .filter_map(|name| std::fs::metadata(paths.data_dir.join(name)).ok())
        .map(|m| m.len())
        .sum::<u64>();
    let data = volume
        .volume_space(&paths.data_dir)
        .context("checking history volume space")?;
    let binary = volume
        .volume_space(paths.binary.parent().context("binary has no directory")?)
        .context("checking binary volume space")?;
    let needed = history_bytes
        .saturating_add(binary_bytes.saturating_mul(3))
        .saturating_add(kelvo_store::low_disk_threshold(data.total_bytes));
    ensure!(
        data.available_bytes > needed,
        "not enough disk space for a recoverable update"
    );
    ensure!(
        binary.available_bytes > binary_bytes.saturating_mul(2),
        "not enough disk space to replace the binary"
    );
    Ok(())
}

pub async fn disable(layout: AgentLayout) -> anyhow::Result<()> {
    maintenance::systemctl(layout, &["disable", "--now", TIMER]).await?;
    maintenance::systemctl(layout, &["stop", SERVICE]).await
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Space(Option<u64>);
    impl kelvo_store::FreeSpace for Space {
        fn volume_space(&self, _: &Path) -> std::io::Result<kelvo_store::VolumeSpace> {
            self.0
                .map(|available_bytes| kelvo_store::VolumeSpace {
                    available_bytes,
                    total_bytes: 10_000_000_000,
                })
                .ok_or_else(|| std::io::ErrorKind::PermissionDenied.into())
        }
    }
    #[test]
    fn unavailable_or_insufficient_disk_never_begins_a_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AgentPaths {
            binary: dir.path().join("bin/agent"),
            data_dir: dir.path().join("data"),
            socket: dir.path().join("socket"),
            unit: dir.path().join("unit"),
        };
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        std::fs::write(paths.data_dir.join("history.sqlite"), b"history").unwrap();
        for volume in [Space(None), Space(Some(0)), Space(Some(100))] {
            assert!(disk_space_with(&paths, 100, &volume).is_err());
            assert!(transaction::read(&paths).unwrap().is_none());
        }
        assert!(disk_space_with(&paths, 100, &Space(Some(1_000_000_000))).is_ok());
    }
}

//! Installation after download, under the host's maintenance lock.
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, ensure};
use kelvo_remote::agent_install::AgentVersion;
use kelvo_remote::{AgentLayout, AgentPaths};
use kelvo_update::{Artifact, ProtocolRange};

use super::{COLLECTOR, health, maintenance, transaction, trial};

pub async fn installed_version(paths: &AgentPaths) -> anyhow::Result<AgentVersion> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(&paths.binary)
            .args(["version", "--json"])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("installed agent version timed out")??;
    ensure!(
        output.status.success(),
        "installed agent could not report its version"
    );
    serde_json::from_slice(&output.stdout).context("reading installed agent version")
}

pub fn protocol(version: &AgentVersion) -> ProtocolRange {
    ProtocolRange {
        min: version.min_compatible,
        max: version.proto,
    }
}

pub async fn install(
    layout: AgentLayout,
    paths: &AgentPaths,
    artifact: &Artifact,
    stage: &Path,
    before: &kelvo_proto::Hello,
    version: &str,
    protocol: ProtocolRange,
) -> anyhow::Result<()> {
    super::report(paths, "installing", Some(version), None)?;
    let mut journal = transaction::begin(paths, version)?;
    maintenance::systemctl(layout, &["stop", COLLECTOR]).await?;
    transaction::prepare(paths, &mut journal)?;
    kelvo_update::verify_artifact(artifact, stage)?;
    let permissions = std::fs::metadata(&paths.binary)
        .context("reading installed binary permissions")?
        .permissions();
    std::fs::set_permissions(stage, permissions)
        .context("preserving installed binary permissions")?;
    transaction::replace(stage, &paths.binary)?;
    super::restore_context(&paths.binary).await?;
    let trial_socket = transaction::directory(paths).join("trial.sock");
    let mut observer = trial::start(layout, paths, &trial_socket)?;
    let healthy = health::trial(&trial_socket, before, version, protocol).await;
    let stopped = trial::finish(layout, &mut observer).await;
    healthy?;
    stopped?;
    candidate_units(layout, paths).await?;
    transaction::commit(paths, &mut journal)?;
    super::report(paths, "updated", Some(version), None)
}

async fn candidate_units(layout: AgentLayout, paths: &AgentPaths) -> anyhow::Result<()> {
    let mut command = tokio::process::Command::new(&paths.binary);
    command
        .arg("unit-templates")
        .arg("--data-dir")
        .arg(&paths.data_dir)
        .arg("--socket")
        .arg(&paths.socket);
    if layout == AgentLayout::System {
        command.arg("--system");
    }
    let output = tokio::time::timeout(Duration::from_secs(5), command.kill_on_drop(true).output())
        .await
        .context("candidate unit templates timed out")??;
    ensure!(
        output.status.success(),
        "candidate could not generate its service units"
    );
    let units: Vec<(std::path::PathBuf, String)> =
        serde_json::from_slice(&output.stdout).context("decoding candidate service units")?;
    let expected = transaction::unit_paths(paths);
    ensure!(
        units.len() == expected.len()
            && expected
                .iter()
                .all(|path| units.iter().filter(|(p, _)| p == path).count() == 1),
        "candidate returned units for another installation"
    );
    for (path, text) in units {
        let staged = transaction::directory(paths).join("unit-text");
        transaction::durable_write(&staged, text.as_bytes())?;
        transaction::replace(&staged, &path)?;
        std::fs::remove_file(staged)?;
    }
    Ok(())
}

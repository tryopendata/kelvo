//! A private systemd service with the public collector's sandbox and resource limits.
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, ensure};
use kelvo_remote::{AgentLayout, AgentPaths};

const UNIT: &str = "kelvo-agent-trial.service";

pub fn start(
    layout: AgentLayout,
    paths: &AgentPaths,
    socket: &Path,
) -> anyhow::Result<tokio::process::Child> {
    let mut command = tokio::process::Command::new("systemd-run");
    if layout == AgentLayout::User {
        command.arg("--user");
    }
    command.args([
        "--wait",
        "--quiet",
        "--collect",
        "--service-type=exec",
        "--unit",
        UNIT,
    ]);
    for property in layout.collector_properties().into_iter().chain([
        "KillMode=control-group",
        "TimeoutStopSec=20s",
        "UMask=0077",
        "BindsTo=kelvo-agent-update.service",
    ]) {
        command.arg("--property").arg(property);
    }
    if layout == AgentLayout::System {
        command
            .arg("--property")
            .arg(format!("ReadWritePaths={}", paths.data_dir.display()));
    }
    command
        .arg(&paths.binary)
        .arg("run-trial")
        .arg("--data-dir")
        .arg(&paths.data_dir)
        .arg("--socket")
        .arg(socket);
    if layout == AgentLayout::System {
        command.arg("--system");
    }
    command
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("starting private trial supervisor")
}

/// systemd-run --wait retains the service's main-process result, even when the
/// transient unit is collected. A successful stop job alone is not enough.
pub async fn finish(
    layout: AgentLayout,
    observer: &mut tokio::process::Child,
) -> anyhow::Result<()> {
    stop(layout).await?;
    let status = tokio::time::timeout(Duration::from_secs(30), observer.wait())
        .await
        .context("waiting for private trial shutdown timed out")??;
    ensure!(
        status.success(),
        "private trial failed during startup or shutdown: {status}"
    );
    Ok(())
}

/// `stop` waits for the whole service cgroup, including children and forced cleanup.
/// Do this explicitly before every recovery; BindsTo alone is not synchronization.
pub async fn stop(layout: AgentLayout) -> anyhow::Result<()> {
    let mut command = tokio::process::Command::new("systemctl");
    if layout == AgentLayout::User {
        command.arg("--user");
    }
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        command
            .args(["show", "--property=LoadState", "--value", UNIT])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("reading trial unit state timed out")??;
    // systemctl show returns exit 4 for an already collected transient unit.
    let state = String::from_utf8_lossy(&output.stdout);
    if state.trim() == "not-found" {
        return Ok(());
    }
    ensure!(
        output.status.success() && !state.trim().is_empty(),
        "reading private trial state: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    crate::maintenance::systemctl(layout, &["stop", UNIT]).await
}

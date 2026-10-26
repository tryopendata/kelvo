//! The one-shot commands the controller runs over SSH. Each prints JSON or one word
//! on stdout and never starts a second writer of the daemon's history.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use kelvo_engine::identity;
use kelvo_proto::{Message, RemoteInterest};
use kelvo_remote::agent_install::{StepOutcome, UninstallReport, UninstallStep};
use kelvo_remote::{AgentLayout, AgentPaths, ServedHost};
use kelvo_schema::{HostId, Settings};
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

use crate::host::AgentHost;

pub fn version() -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "proto": kelvo_proto::PROTO_VERSION,
        "min_compatible": kelvo_proto::MIN_COMPATIBLE,
        "automatic_updates": true,
    })
}

/// `status --json`. A running daemon answers with its `Hello` (id, epoch, version);
/// without one, the id comes from the data directory's `host-id` file and the epoch is
/// null. Never opens the history database.
pub async fn status(layout: AgentLayout, paths: &AgentPaths) -> Value {
    let hello = daemon_hello(&paths.socket).await;
    let (host_id, epoch, daemon_version) = match &hello {
        Some(hello) => (
            Some(hello.host.id.to_string()),
            Some(hello.db_instance_uuid.to_string()),
            Some(hello.app_version.clone()),
        ),
        None => (stored_host_id(&paths.data_dir), None, None),
    };
    json!({
        "running": hello.is_some(),
        "host_id": host_id,
        "epoch": epoch,
        "version": env!("CARGO_PKG_VERSION"),
        "daemon_version": daemon_version,
        "layout": layout.as_str(),
        "data_dir": paths.data_dir,
        "socket": paths.socket,
    })
}

/// The daemon's `Hello`: the server sends it first on every connection. Dropping the
/// connection before acknowledging ends that session on the daemon.
async fn daemon_hello(socket: &Path) -> Option<kelvo_proto::Hello> {
    let read = async {
        let mut stream = tokio::net::UnixStream::connect(socket).await.ok()?;
        let length = stream.read_u32().await.ok()?;
        if length > kelvo_proto::MAX_FRAME_LEN {
            return None;
        }
        let mut body = vec![0; length as usize];
        stream.read_exact(&mut body).await.ok()?;
        match kelvo_proto::decode_message(&body).ok()? {
            Message::Hello(hello) => Some(*hello),
            _ => None,
        }
    };
    tokio::time::timeout(Duration::from_secs(5), read)
        .await
        .ok()
        .flatten()
}

fn stored_host_id(data_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(data_dir.join(identity::HOST_ID_FILE)).ok()?;
    let line = text.lines().next()?.trim();
    uuid::Uuid::parse_str(line).ok().map(|id| id.to_string())
}

/// `install-unit`: writes the unit only when its text changed. Returns `"written"` or
/// `"unchanged"`; starting it is the controller's `systemctl` call.
pub fn install_unit(layout: AgentLayout, paths: &AgentPaths) -> anyhow::Result<&'static str> {
    let mut changed = false;
    for (path, text) in std::iter::once((paths.unit.clone(), layout.unit_text(paths)))
        .chain(layout.update_units(paths))
    {
        if std::fs::read_to_string(&path).is_ok_and(|current| current == text) {
            continue;
        }
        let dir = path.parent().context("unit has no directory")?;
        std::fs::create_dir_all(dir).context("creating systemd unit directory")?;
        let tmp = path.with_extension("service.tmp");
        std::fs::write(&tmp, text).context("writing systemd unit")?;
        std::fs::File::open(&tmp)?.sync_all()?;
        std::fs::rename(&tmp, &path).context("replacing systemd unit")?;
        std::fs::File::open(dir)?.sync_all()?;
        changed = true;
    }
    let recovery = crate::update::transaction::recovery_binary(paths);
    if !recovery.exists() {
        let executable = std::env::current_exe().context("locating recovery executable")?;
        std::fs::create_dir_all(recovery.parent().context("recovery has no directory")?)?;
        crate::update::transaction::replace(&executable, &recovery)?;
        changed = true;
    }
    Ok(if changed { "written" } else { "unchanged" })
}

/// `uninstall [--purge]`: removes the unit file and, with `purge`, the data directory.
/// Already-missing pieces are fine, and a failure is reported without stopping the
/// rest (D-105: removal never blocks on the remote side).
pub fn uninstall(paths: &AgentPaths, purge: bool) -> UninstallReport {
    let mut steps = vec![];
    for path in crate::update::transaction::unit_paths(paths) {
        steps.push(UninstallStep {
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            outcome: remove(&path, false),
        });
    }
    for (name, path, directory) in [
        (
            "recovery",
            crate::update::transaction::recovery_binary(paths),
            false,
        ),
        (
            "update_state",
            crate::update::transaction::directory(paths),
            true,
        ),
        ("binary", paths.binary.clone(), false),
    ] {
        steps.push(UninstallStep {
            name: name.into(),
            outcome: remove(&path, directory),
        });
    }
    if purge {
        steps.push(UninstallStep {
            name: "data_dir".into(),
            outcome: remove(&paths.data_dir, true),
        });
    }
    UninstallReport { steps }
}

fn remove(path: &Path, dir: bool) -> StepOutcome {
    let removed = if dir {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match removed {
        Ok(()) => StepOutcome::Ok,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => StepOutcome::Absent,
        Err(e) => {
            tracing::warn!(path = %path.display(), "removing: {e}");
            StepOutcome::Failed(e.to_string())
        }
    }
}

/// `reset-identity`: a new host id for this data directory. The daemon picks it up on
/// restart; history recorded under the old id stays as a non-local host.
pub fn reset_identity(paths: &AgentPaths, system: bool) -> anyhow::Result<HostId> {
    identity::reset(&paths.data_dir, crate::daemon::binding(system))
        .with_context(|| format!("resetting the host id in {}", paths.data_dir.display()))
}

/// How long `sample` collects after the first frame by default: long enough for the
/// 1 s rate series to take two samples (the 5 s ones need `sample --json 12`).
const SAMPLE_FOR: Duration = Duration::from_millis(2500);
/// How long the engine may take to probe its collectors before the first frame.
const FIRST_FRAME_WITHIN: Duration = Duration::from_secs(20);

/// `sample --json`: runs the engine in this process, live only, with every module at
/// its visible cadence, and prints the latest measured value of every series, the
/// capabilities and the host record. For checking readings against `free`, `mpstat`
/// and `iostat`. It uses a throwaway host id and touches no data directory.
pub async fn sample(seconds: Option<u64>) -> anyhow::Result<Value> {
    let sample_for = seconds.map_or(SAMPLE_FOR, Duration::from_secs);
    let record = kelvo_engine::host_record(HostId(uuid::Uuid::new_v4()));
    let names = crate::container_names::spawn();
    crate::container_names::first_pass(&names, crate::container_names::FIRST_PASS_WITHIN).await;
    let host = tokio::task::spawn_blocking(move || {
        let host = AgentHost::start(record, Settings::agent_default(), None, names)?;
        host.set_local_interest(RemoteInterest {
            detail: true,
            process_period_ms: Some(0),
            ..RemoteInterest::default()
        });
        anyhow::Ok(host)
    })
    .await
    .context("starting the engine")??;
    let mut latest: BTreeMap<String, Value> = BTreeMap::new();
    let mut deadline = tokio::time::Instant::now() + FIRST_FRAME_WITHIN;
    let mut last_ts = None;
    let mut frames = 0u32;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let Some(frame) = host.hub().latest_frame() else {
            continue;
        };
        if last_ts == Some(frame.ts_ms) {
            continue;
        }
        if last_ts.is_none() {
            deadline = tokio::time::Instant::now() + sample_for;
        }
        last_ts = Some(frame.ts_ms);
        frames += 1;
        for (key, value) in frame.layout.series.iter().zip(frame.values.iter()) {
            if value.is_finite() {
                let labels: BTreeMap<&str, &str> = key.labels.iter().collect();
                latest.insert(
                    key.to_string(),
                    json!({ "metric": key.metric.to_string(), "labels": labels, "value": value }),
                );
            }
        }
    }
    let capabilities = host.capabilities();
    let record = host.record();
    tokio::task::spawn_blocking(move || host.stop())
        .await
        .context("stopping the engine")?;
    Ok(json!({
        "host": record,
        "capabilities": capabilities,
        "frames": frames,
        "last_frame_ts_ms": last_ts,
        "series": latest.into_values().collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(dir: &Path) -> AgentPaths {
        AgentPaths {
            binary: dir.join("bin/kelvo-agent"),
            data_dir: dir.join("data"),
            socket: dir.join("data/metrics.sock"),
            unit: dir.join("units/kelvo-agent.service"),
        }
    }

    #[test]
    fn install_unit_writes_once_then_reports_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        assert_eq!(install_unit(AgentLayout::User, &paths).unwrap(), "written");
        assert_eq!(
            install_unit(AgentLayout::User, &paths).unwrap(),
            "unchanged"
        );
        assert_eq!(
            std::fs::read_to_string(&paths.unit).unwrap(),
            AgentLayout::User.unit_text(&paths)
        );
        // A hand-edited or older unit is replaced.
        std::fs::write(&paths.unit, "[Service]\n").unwrap();
        assert_eq!(install_unit(AgentLayout::User, &paths).unwrap(), "written");
    }

    #[test]
    fn uninstall_tolerates_missing_pieces() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let report = uninstall(&paths, true);
        assert_eq!(report.steps.len(), 8);
        assert!(
            report
                .steps
                .iter()
                .all(|step| step.outcome == StepOutcome::Absent)
        );
        install_unit(AgentLayout::User, &paths).unwrap();
        std::fs::create_dir_all(paths.data_dir.join("x")).unwrap();
        assert!(uninstall(&paths, false).is_clean());
        assert!(paths.data_dir.exists());
        assert!(uninstall(&paths, true).is_clean());
        assert!(!paths.data_dir.exists());
    }

    #[test]
    fn uninstall_reports_each_filesystem_failure_and_continues() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        install_unit(AgentLayout::User, &paths).unwrap();
        // Real filesystem failures, including under root: a directory cannot be
        // unlinked as a file, nor a regular file purged as a directory.
        std::fs::create_dir(&paths.binary).unwrap();
        std::fs::write(&paths.data_dir, "unexpected file").unwrap();
        let timer = paths.unit.with_file_name("kelvo-agent-update.timer");
        std::fs::remove_file(&timer).unwrap();
        std::fs::create_dir(&timer).unwrap();
        let report = uninstall(&paths, true);
        assert!(!report.is_clean());
        for name in ["binary", "data_dir", "kelvo-agent-update.timer"] {
            assert!(report.steps.iter().any(|step| step.name == name && matches!(step.outcome, StepOutcome::Failed(_))), "{report:?}");
        }
        assert!(!paths.unit.exists(), "other cleanup must continue");
        assert!(!crate::update::transaction::recovery_binary(&paths).exists());
    }

    #[tokio::test]
    async fn status_without_a_daemon_reads_the_stored_id() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let before = status(AgentLayout::User, &paths).await;
        assert_eq!(before["running"], false);
        assert_eq!(before["host_id"], Value::Null);
        let id = reset_identity(&paths, false).unwrap();
        let after = status(AgentLayout::User, &paths).await;
        assert_eq!(after["host_id"], id.to_string());
        assert_eq!(after["epoch"], Value::Null);
        assert_eq!(after["layout"], "user");
    }
}

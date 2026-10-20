//! `run`: the daemon. It owns the engine and the history store (their single writer)
//! and serves both on the metrics socket until SIGTERM or SIGINT.
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use kelvo_engine::{HistoryStore, identity, retention_for};
use kelvo_remote::{AgentPaths, HostServer};
use kelvo_schema::HostId;
use tokio::signal::unix::{SignalKind, signal};

use crate::host::AgentHost;

/// The binding that ties a host id to this machine (D-115). Linux binds to
/// `/etc/machine-id`, plus the firmware id for the system agent; macOS uses the
/// hardware UUID as the app does.
pub fn binding(system: bool) -> impl Fn(HostId) -> Option<String> {
    move |id| platform_binding(id, system)
}

#[cfg(target_os = "linux")]
fn platform_binding(id: HostId, system: bool) -> Option<String> {
    let scope = if system {
        identity::BindingScope::WithHardware
    } else {
        identity::BindingScope::MachineId
    };
    identity::linux_binding(id, Path::new("/"), scope)
}

#[cfg(not(target_os = "linux"))]
fn platform_binding(id: HostId, _system: bool) -> Option<String> {
    identity::machine_binding(id)
}

pub async fn run(paths: &AgentPaths, system: bool) -> anyhow::Result<()> {
    check_socket_dir(&paths.socket)?;
    let (history, id) = identity::open_local(&paths.data_dir, binding(system));
    let record = kelvo_engine::host_record(id.id);
    let settings = crate::settings::load(&paths.data_dir);
    tracing::info!(
        host = %id.id,
        id_source = ?id.source,
        id_persisted = id.persisted,
        name = %record.display_name,
        os = %record.info.os_version,
        data_dir = %paths.data_dir.display(),
        socket = %paths.socket.display(),
        retention_days = settings.history.retention_days,
        size_limit_mb = settings.history.size_limit_mb,
        "kelvo-agent {} starting",
        env!("CARGO_PKG_VERSION"),
    );
    // A store that cannot take the host leaves history unavailable; the agent still
    // serves live data (D-064).
    let writer = history.register_host(&record);
    if writer.is_none() {
        tracing::warn!("history unavailable; serving live data only");
    }
    let names = crate::container_names::spawn();
    crate::container_names::first_pass(&names, crate::container_names::FIRST_PASS_WITHIN).await;
    let host = AgentHost::start(record, settings.clone(), writer, names)?;
    let budget = settings.history;
    let housekeeping = history
        .spawn_pruner(
            move || retention_for(&budget),
            |health| tracing::info!(?health, "history health"),
        )
        .context("starting history housekeeping")?;
    let history = Arc::new(history);
    let result = serve_until_signal(&paths.socket, Arc::clone(&host), history).await;
    // The socket is gone once the server returns; now the engine closes its buckets
    // and gaps. Stopping waits on the engine thread, so off the runtime's workers.
    let stopping = Arc::clone(&host);
    tokio::task::spawn_blocking(move || stopping.stop())
        .await
        .context("stopping the engine")?;
    drop(housekeeping);
    tracing::info!("kelvo-agent stopped");
    result
}

/// The metrics server makes the socket's directory owner-only. Pointed at a shared
/// directory (`--socket /tmp/x.sock`, as root) that would lock everyone else out of it,
/// so a shared parent is refused before anything starts.
fn check_socket_dir(socket: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let parent = socket
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .context("the socket path needs a directory")?;
    match std::fs::metadata(parent) {
        Ok(meta) if meta.permissions().mode() & 0o1002 != 0 => anyhow::bail!(
            "{} is shared (world-writable or sticky); put the socket in a directory of its own",
            parent.display()
        ),
        _ => Ok(()),
    }
}

async fn serve_until_signal(
    socket: &Path,
    host: Arc<AgentHost>,
    history: Arc<HistoryStore>,
) -> anyhow::Result<()> {
    let mut term = signal(SignalKind::terminate()).context("installing the SIGTERM handler")?;
    let mut int = signal(SignalKind::interrupt()).context("installing the SIGINT handler")?;
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let server = kelvo_remote::serve_metrics(
        socket.to_path_buf(),
        Arc::new(HostServer::new(host, history)),
        stopped,
    );
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => {
            return result.with_context(|| format!("serving {}", socket.display()));
        }
        _ = term.recv() => tracing::info!("SIGTERM: stopping"),
        _ = int.recv() => tracing::info!("SIGINT: stopping"),
    }
    let _ = stop.send(true);
    server
        .await
        .with_context(|| format!("serving {}", socket.display()))
}

/// `serve --stdio`: bridges this SSH session to the daemon. A missing or refused socket
/// is [`kelvo_remote::AGENT_NOT_RUNNING`] and exit status 3, which the controller
/// reports as "agent not running".
pub async fn serve(socket: &Path) -> anyhow::Result<std::process::ExitCode> {
    match kelvo_remote::stdio_bridge(socket).await {
        Ok(()) => Ok(std::process::ExitCode::SUCCESS),
        Err(e) if not_running(&e) => {
            eprintln!("{}", kelvo_remote::AGENT_NOT_RUNNING);
            Ok(std::process::ExitCode::from(3))
        }
        Err(e) => Err(e).with_context(|| format!("bridging {}", socket.display())),
    }
}

/// Connect errors that mean no daemon is listening (a stale socket file refuses).
pub fn not_running(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_socket_directories_are_refused() {
        assert!(check_socket_dir(Path::new("/tmp/x.sock")).is_err());
        assert!(check_socket_dir(Path::new("x.sock")).is_err());
        let dir = tempfile::tempdir().unwrap();
        assert!(check_socket_dir(&dir.path().join("x.sock")).is_ok());
        // A directory that does not exist yet is created private by the server.
        assert!(check_socket_dir(&dir.path().join("new/x.sock")).is_ok());
    }

    #[test]
    fn missing_or_refused_sockets_mean_not_running() {
        assert!(not_running(&std::io::ErrorKind::NotFound.into()));
        assert!(not_running(&std::io::ErrorKind::ConnectionRefused.into()));
        assert!(!not_running(&std::io::ErrorKind::BrokenPipe.into()));
    }
}

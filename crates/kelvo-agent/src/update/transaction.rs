//! Durable file transaction. Only a stopped collector may enter or recover it.
//! A committed journal is a one-way door: its database is never restored.
use std::fs::{self, File};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::Context;
use kelvo_remote::AgentPaths;
use serde::{Deserialize, Serialize};

const FILES: &[&str] = &[
    "history.sqlite",
    "history.sqlite-wal",
    "history.sqlite-shm",
    "host-id",
    "settings.json",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Stopping,
    Prepared,
    Committed,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Journal {
    pub phase: Phase,
    pub version: String,
    pub binary: PathBuf,
}

pub fn unit_paths(paths: &AgentPaths) -> Vec<PathBuf> {
    let parent = paths.unit.parent().unwrap_or(Path::new("."));
    [
        "kelvo-agent.service",
        "kelvo-agent-update.service",
        "kelvo-agent-update.timer",
        "kelvo-agent-recovery.service",
    ]
    .into_iter()
    .map(|name| parent.join(name))
    .collect()
}

pub fn directory(paths: &AgentPaths) -> PathBuf {
    paths.data_dir.join("update")
}
pub fn recovery_binary(paths: &AgentPaths) -> PathBuf {
    paths.binary.with_file_name(".kelvo-agent-recovery")
}

pub fn read(paths: &AgentPaths) -> anyhow::Result<Option<Journal>> {
    match fs::read(directory(paths).join("transaction.json")) {
        Ok(bytes) => Ok(Some(
            serde_json::from_slice(&bytes).context("decoding update recovery journal")?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context("reading update recovery journal"),
    }
}

pub fn durable_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().context("durable file has no parent")?;
    kelvo_store::create_private_dir(parent).context("creating update state directory")?;
    let tmp = path.with_extension("tmp");
    kelvo_store::write_private(&tmp, bytes).context("writing update state")?;
    File::open(&tmp)?
        .sync_all()
        .context("syncing update state")?;
    fs::rename(&tmp, path).context("committing update state")?;
    File::open(parent)?
        .sync_all()
        .context("syncing update state directory")
}

pub fn write(paths: &AgentPaths, journal: &Journal) -> anyhow::Result<()> {
    durable_write(
        &directory(paths).join("transaction.json"),
        &serde_json::to_vec(journal)?,
    )
}

pub fn begin(paths: &AgentPaths, version: &str) -> anyhow::Result<Journal> {
    anyhow::ensure!(
        read(paths)?.is_none(),
        "an interrupted update needs recovery first"
    );
    let journal = Journal {
        phase: Phase::Stopping,
        version: version.into(),
        binary: paths.binary.clone(),
    };
    write(paths, &journal)?;
    Ok(journal)
}

pub fn prepare(paths: &AgentPaths, journal: &mut Journal) -> anyhow::Result<()> {
    let _history = history_lock(paths)?;
    let backup = directory(paths).join("backup");
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    kelvo_store::create_private_dir(&backup)?;
    for name in FILES {
        let source = paths.data_dir.join(name);
        if source.exists() {
            copy_synced(&source, &backup.join(name))?;
        }
    }
    copy_synced(&paths.binary, &backup.join("kelvo-agent"))?;
    let units = backup.join("units");
    kelvo_store::create_private_dir(&units)?;
    for source in unit_paths(paths) {
        if source.exists() {
            copy_synced(
                &source,
                &units.join(source.file_name().context("unit has no file name")?),
            )?;
        }
    }
    File::open(&units)?.sync_all()?;
    File::open(&backup)?.sync_all()?;
    journal.phase = Phase::Prepared;
    write(paths, journal)
}

pub fn replace(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let staging = destination.with_extension("update-tmp");
    copy_synced(source, &staging)?;
    if let Ok(previous) = fs::metadata(destination) {
        let staged = fs::metadata(&staging)?;
        if staged.uid() != previous.uid() || staged.gid() != previous.gid() {
            rustix::fs::chown(
                &staging,
                Some(rustix::fs::Uid::from_raw(previous.uid())),
                Some(rustix::fs::Gid::from_raw(previous.gid())),
            )
            .context("preserving installed file ownership")?;
            fs::set_permissions(&staging, fs::metadata(source)?.permissions())?;
            File::open(&staging)?.sync_all()?;
        }
    }
    fs::rename(&staging, destination).context("replacing agent binary")?;
    File::open(destination.parent().context("binary has no parent")?)?.sync_all()?;
    Ok(())
}

fn copy_synced(source: &Path, destination: &Path) -> anyhow::Result<()> {
    fs::copy(source, destination).with_context(|| format!("copying {}", source.display()))?;
    File::open(destination)?
        .sync_all()
        .context("syncing update backup")
}

pub fn commit(paths: &AgentPaths, journal: &mut Journal) -> anyhow::Result<()> {
    journal.phase = Phase::Committed;
    write(paths, journal)
}

/// Returns whether a transaction existed. Repeated recovery is idempotent, even if
/// interrupted during restoration: the durable journal is removed last.
pub fn recover(paths: &AgentPaths) -> anyhow::Result<Option<Journal>> {
    let Some(journal) = read(paths)? else {
        return Ok(None);
    };
    anyhow::ensure!(
        journal.binary == paths.binary,
        "update journal names another installation"
    );
    if journal.phase == Phase::Prepared {
        let _history = history_lock(paths)?;
        let backup = directory(paths).join("backup");
        anyhow::ensure!(
            backup.join("kelvo-agent").is_file(),
            "update recovery backup is missing"
        );
        replace(&backup.join("kelvo-agent"), &paths.binary)?;
        for name in FILES {
            let destination = paths.data_dir.join(name);
            let source = backup.join(name);
            if source.exists() {
                replace(&source, &destination)?;
            } else if destination.exists() {
                fs::remove_file(destination)?;
            }
        }
        for destination in unit_paths(paths) {
            let source = backup
                .join("units")
                .join(destination.file_name().context("unit has no file name")?);
            if source.exists() {
                replace(&source, &destination)?;
            } else if destination.exists() {
                fs::remove_file(&destination)?;
            }
        }
        if let Some(parent) = paths.unit.parent().filter(|p| p.exists()) {
            File::open(parent)?.sync_all()?;
        }
        File::open(&paths.data_dir)?.sync_all()?;
    }
    // Refresh from the now-final installed binary. Atomic replacement keeps the
    // previous recovery executable usable if this process dies during its own copy.
    replace(&paths.binary, &recovery_binary(paths))?;
    // Keep restart intent after journal cleanup. If recovery dies before starting
    // the public service, ExecStopPost (or boot recovery) must still finish it.
    durable_write(
        &directory(paths).join("restart-required"),
        &serde_json::to_vec(&journal)?,
    )?;
    // Unlink before cleanup: after commit there is never a path back into rollback.
    fs::remove_file(directory(paths).join("transaction.json"))?;
    File::open(directory(paths))?.sync_all()?;
    let _ = fs::remove_dir_all(directory(paths).join("backup"));
    Ok(Some(journal))
}

fn history_lock(paths: &AgentPaths) -> anyhow::Result<File> {
    let file = kelvo_store::open_private(&kelvo_store::lock_path(
        &paths.data_dir.join("history.sqlite"),
    ))?;
    file.try_lock()
        .context("collector still owns history; refusing to back up or restore a live database")?;
    Ok(file)
}

pub fn public_starting(paths: &AgentPaths) -> anyhow::Result<()> {
    public_allowed(paths)?;
    let marker = directory(paths).join("restart-required");
    match fs::remove_file(&marker) {
        Ok(()) => File::open(directory(paths))?
            .sync_all()
            .context("clearing update restart intent"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).context("clearing update restart intent"),
    }
}

pub fn public_allowed(paths: &AgentPaths) -> anyhow::Result<()> {
    anyhow::ensure!(
        read(paths)?.is_none_or(|j| j.phase == Phase::Committed),
        "update trial in progress; public monitoring remains stopped until commit"
    );
    Ok(())
}

#[cfg(test)]
#[path = "transaction/tests.rs"]
mod tests;

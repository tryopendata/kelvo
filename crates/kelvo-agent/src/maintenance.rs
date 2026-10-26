//! One installation lock, outside the purgeable state directory. OS release on process
//! death lets the recovery service make progress after an interrupted transaction.
use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use anyhow::Context;
use kelvo_remote::AgentPaths;

pub fn lock_path(paths: &AgentPaths) -> PathBuf {
    paths.data_dir.with_extension("installation.lock")
}

pub fn lock(paths: &AgentPaths) -> anyhow::Result<File> {
    let path = lock_path(paths);
    let parent = path.parent().context("installation lock has no parent")?;
    std::fs::create_dir_all(parent).context("creating installation lock directory")?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("opening installation lock {}", path.display()))?;
    file.try_lock()
        .context("another agent installation or maintenance operation is running")?;
    Ok(file)
}

pub async fn systemctl(layout: kelvo_remote::AgentLayout, args: &[&str]) -> anyhow::Result<()> {
    let mut cmd = tokio::process::Command::new("systemctl");
    if layout == kelvo_remote::AgentLayout::User {
        cmd.arg("--user");
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(45),
        cmd.args(args).kill_on_drop(true).output(),
    )
    .await
    .context("systemctl timed out")?
    .context("running systemctl")?;
    anyhow::ensure!(
        output.status.success(),
        "systemctl {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

pub async fn active(layout: kelvo_remote::AgentLayout) -> bool {
    systemctl(layout, &["is-active", "--quiet", "kelvo-agent.service"])
        .await
        .is_ok()
        && systemctl(layout, &["is-enabled", "--quiet", "kelvo-agent.service"])
            .await
            .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_maintenance_is_excluded_and_purge_cannot_replace_the_lock() {
        // Isolate OS lock lifetime from other tests forking between our lock and
        // drop: a child briefly inherits CLOEXEC descriptors until its exec.
        if std::env::var_os("KELVO_TEST_LOCK_CHILD").is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "maintenance::tests::concurrent_maintenance_is_excluded_and_purge_cannot_replace_the_lock", "--nocapture"])
                .env("KELVO_TEST_LOCK_CHILD", "1").output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let paths = AgentPaths {
            binary: dir.path().join("bin/kelvo-agent"),
            data_dir: dir.path().join("data"),
            socket: dir.path().join("socket"),
            unit: dir.path().join("unit"),
        };
        std::fs::create_dir(&paths.data_dir).unwrap();
        let first = lock(&paths).unwrap();
        assert!(lock(&paths).is_err());
        std::fs::remove_dir_all(&paths.data_dir).unwrap();
        std::fs::create_dir(&paths.data_dir).unwrap();
        assert!(lock(&paths).is_err());
        drop(first);
        assert!(lock(&paths).is_ok());
    }
}

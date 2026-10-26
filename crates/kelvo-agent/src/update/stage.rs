//! Each attempt owns one stage; crashed attempts are removed under the install lock.
use std::path::{Path, PathBuf};

use anyhow::Context;
use kelvo_remote::AgentPaths;

use super::transaction::directory;

pub struct Stage(PathBuf);
impl Stage {
    pub fn new(paths: &AgentPaths) -> anyhow::Result<Self> {
        let root = directory(paths).join("stages");
        kelvo_store::create_private_dir(&root)?;
        Ok(Self(root.join(uuid::Uuid::new_v4().to_string())))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub fn clean(paths: &AgentPaths) -> anyhow::Result<()> {
    let root = directory(paths).join("stages");
    match std::fs::remove_dir_all(root) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).context("cleaning abandoned update downloads"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn successive_attempts_do_not_reuse_or_leave_a_stage() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AgentPaths {
            binary: dir.path().join("bin"),
            data_dir: dir.path().join("data"),
            socket: dir.path().join("socket"),
            unit: dir.path().join("unit"),
        };
        let first = Stage::new(&paths).unwrap();
        let first_path = first.path().to_owned();
        std::fs::write(&first_path, "verified bytes").unwrap();
        drop(first);
        assert!(!first_path.exists());
        let second = Stage::new(&paths).unwrap();
        assert_ne!(second.path(), first_path);
        std::fs::write(second.path(), "next release").unwrap();
        let abandoned = second.path().to_owned();
        std::mem::forget(second);
        clean(&paths).unwrap();
        assert!(!abandoned.exists());
    }
}

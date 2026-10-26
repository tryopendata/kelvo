//! Owner-only cache. Metadata retains the exact signed bytes, never reserialized claims.
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

use super::Candidate;

#[derive(Serialize, Deserialize)]
struct Metadata {
    manifest: Vec<u8>,
    signature: String,
}

pub(crate) fn directory(app: &AppHandle) -> anyhow::Result<PathBuf> {
    let path = app.path().app_cache_dir()?.join("updates");
    private_directory(&path)?;
    Ok(path)
}

fn private_directory(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path).context("creating update cache")?;
    if !fs::symlink_metadata(path)?.is_dir() {
        bail!("Update cache must be a real directory");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub(super) fn artifact_path(app: &AppHandle) -> anyhow::Result<PathBuf> {
    Ok(directory(app)?.join("package.tar.gz"))
}

pub(super) fn save(app: &AppHandle, candidate: &Candidate) -> anyhow::Result<()> {
    let metadata = Metadata {
        manifest: candidate.manifest.clone(),
        signature: candidate.signature.clone(),
    };
    write_private(
        &directory(app)?.join("release.json"),
        &serde_json::to_vec(&metadata)?,
    )
}

pub(super) fn load(app: &AppHandle) -> anyhow::Result<Option<kelvo_update::VerifiedRelease>> {
    let path = directory(app)?.join("release.json");
    let bytes = match read_bounded(&path, 8 * 1024 * 1024) {
        Ok(bytes) => bytes,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let metadata: Metadata =
        serde_json::from_slice(&bytes).context("reading cached update metadata")?;
    Ok(Some(kelvo_update::verify_manifest(
        &metadata.manifest,
        &metadata.signature,
    )?))
}

pub(super) fn verified_bytes(app: &AppHandle, candidate: &Candidate) -> anyhow::Result<Vec<u8>> {
    verified_package(&artifact_path(app)?, candidate)
}

fn verified_package(path: &Path, candidate: &Candidate) -> anyhow::Result<Vec<u8>> {
    let bytes = read_bounded(path, candidate.artifact.size)?;
    // Hash the same bytes handed to Tauri, not a separate read of a mutable path.
    if bytes.len() as u64 != candidate.artifact.size
        || format!("{:x}", Sha256::digest(&bytes)) != candidate.artifact.sha256.to_ascii_lowercase()
    {
        bail!("Cached update does not match the signed release");
    }
    // Tauri checks the archive signature and the version in its signed comment.
    candidate.update.verify(&bytes)?;
    Ok(bytes)
}

pub(crate) fn read_bounded(path: &Path, maximum: u64) -> anyhow::Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.is_file() {
        bail!("Update cache entry must be a regular file");
    }
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > maximum {
        bail!("Update cache entry exceeds its size limit");
    }
    let mut bytes = Vec::new();
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        bail!("Update cache entry exceeds its size limit");
    }
    Ok(bytes)
}

pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let temporary = path.with_extension("tmp");
    let _ = fs::remove_file(&temporary);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;

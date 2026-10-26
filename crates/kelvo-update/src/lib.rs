//! Authenticated releases shared by the app and the headless agent. Platform owners
//! decide when to install; this crate only authenticates, selects and downloads.
use std::{collections::BTreeMap, io::Read, path::Path};

use base64::{Engine, engine::general_purpose::STANDARD};
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod client;
pub use client::Client;

pub const MANIFEST_URL: &str =
    "https://github.com/tryopendata/kelvo/releases/latest/download/release.json";
pub const PUBLIC_KEY: &str = include_str!("../public-key.txt");
pub const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
pub const MAX_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("release authentication failed: {0}")]
    Signature(String),
    #[error("invalid release: {0}")]
    Invalid(String),
    #[error("release {version} drops an installed protocol version")]
    Incompatible { version: String },
    #[error("release has no artifact for {0}")]
    MissingTarget(String),
    #[error("download size or digest does not match the signed release")]
    Integrity,
    #[error("update request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("update file operation failed: {0}")]
    Io(#[from] std::io::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolRange {
    pub min: u16,
    pub max: u16,
}

impl ProtocolRange {
    pub fn contains(self, installed: Self) -> bool {
        self.min > 0
            && installed.min > 0
            && self.min <= self.max
            && installed.min <= installed.max
            && self.min <= installed.min
            && self.max >= installed.max
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Artifact {
    pub url: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Release {
    pub schema_version: u32,
    pub version: String,
    pub notes: String,
    pub protocol: ProtocolRange,
    pub artifacts: BTreeMap<String, Artifact>,
}

/// Only signature verification constructs this type. Cache the authenticated bytes,
/// not a reserialized release, so restored candidates pass the same verification.
#[derive(Clone, Debug)]
pub struct VerifiedRelease {
    release: Release,
    bytes: Vec<u8>,
    signature: String,
}

impl VerifiedRelease {
    pub fn release(&self) -> &Release {
        &self.release
    }

    pub fn manifest_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn signature(&self) -> &str {
        &self.signature
    }

    pub fn candidate(
        &self,
        current: &str,
        protocol: ProtocolRange,
        target: &str,
    ) -> Result<Option<&Artifact>> {
        let installed = parse_version(current)?;
        let candidate = parse_version(&self.release.version)?;
        if candidate <= installed {
            return Ok(None);
        }
        if !self.release.protocol.contains(protocol) {
            return Err(Error::Incompatible {
                version: self.release.version.clone(),
            });
        }
        self.release
            .artifacts
            .get(target)
            .map(Some)
            .ok_or_else(|| Error::MissingTarget(target.into()))
    }
}

fn parse_version(version: &str) -> Result<Version> {
    Version::parse(version).map_err(|e| Error::Invalid(e.to_string()))
}

pub fn verify_manifest(bytes: &[u8], signature: &str) -> Result<VerifiedRelease> {
    verify_with_key(bytes, signature, PUBLIC_KEY)
}

fn verify_with_key(bytes: &[u8], signature: &str, key: &str) -> Result<VerifiedRelease> {
    if bytes.len() as u64 > MAX_MANIFEST_BYTES || signature.len() > 4096 {
        return Err(Error::Invalid(
            "manifest or signature exceeds its size limit".into(),
        ));
    }
    let decode = |text: &str| -> Result<String> {
        let bytes = STANDARD
            .decode(text.trim())
            .map_err(|e| Error::Signature(e.to_string()))?;
        String::from_utf8(bytes).map_err(|e| Error::Signature(e.to_string()))
    };
    let key = PublicKey::decode(&decode(key)?).map_err(|e| Error::Signature(e.to_string()))?;
    let decoded =
        Signature::decode(&decode(signature)?).map_err(|e| Error::Signature(e.to_string()))?;
    key.verify(bytes, &decoded, false)
        .map_err(|e| Error::Signature(e.to_string()))?;
    let release: Release =
        serde_json::from_slice(bytes).map_err(|e| Error::Invalid(e.to_string()))?;
    validate(&release)?;
    Ok(VerifiedRelease {
        release,
        bytes: bytes.to_vec(),
        signature: signature.trim().into(),
    })
}

fn validate(release: &Release) -> Result<()> {
    let version = parse_version(&release.version)?;
    if release.schema_version != 1
        || !version.pre.is_empty()
        || !version.build.is_empty()
        || !release.protocol.contains(release.protocol)
        || release.artifacts.is_empty()
    {
        return Err(Error::Invalid(
            "unsupported schema, stable version or protocol range".into(),
        ));
    }
    for (target, artifact) in &release.artifacts {
        let url = reqwest::Url::parse(&artifact.url).map_err(|e| Error::Invalid(e.to_string()))?;
        let prefix = format!("/tryopendata/kelvo/releases/download/v{}/", release.version);
        if url.scheme() != "https"
            || url.host_str() != Some("github.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.path().starts_with(&prefix)
            || url.path().len() <= prefix.len()
            || artifact.size == 0
            || artifact.size > MAX_ARTIFACT_BYTES
            || artifact.sha256.len() != 64
            || !artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || (target == "darwin-aarch64"
                && artifact
                    .signature
                    .as_ref()
                    .is_none_or(|s| s.trim().is_empty()))
        {
            return Err(Error::Invalid(format!("invalid artifact for {target}")));
        }
    }
    Ok(())
}

/// Reverify an on-disk stage immediately before handing it to an installer.
pub fn verify_artifact(artifact: &Artifact, path: &Path) -> Result<()> {
    let mut file = std::fs::File::open(path)?;
    if file.metadata()?.len() != artifact.size || artifact.size > MAX_ARTIFACT_BYTES {
        return Err(Error::Integrity);
    }
    let mut digest = Sha256::new();
    let mut buf = [0; 64 * 1024];
    let mut size = 0_u64;
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        if size > artifact.size {
            return Err(Error::Integrity);
        }
        digest.update(&buf[..read]);
    }
    if size != artifact.size
        || format!("{:x}", digest.finalize()) != artifact.sha256.to_ascii_lowercase()
    {
        return Err(Error::Integrity);
    }
    Ok(())
}

#[cfg(test)]
mod tests;

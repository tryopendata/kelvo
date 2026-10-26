use std::{path::Path, time::Duration};

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::{
    Artifact, Error, MANIFEST_URL, MAX_ARTIFACT_BYTES, MAX_MANIFEST_BYTES, Result, VerifiedRelease,
    verify_manifest,
};

pub struct Client {
    http: reqwest::Client,
    endpoint: String,
}

impl Client {
    pub fn production() -> Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        Ok(Self {
            http: reqwest::Client::builder()
                .https_only(true)
                .user_agent(concat!("Kelvo/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(30))
                .timeout(Duration::from_secs(10 * 60))
                .redirect(reqwest::redirect::Policy::limited(5))
                .build()?,
            endpoint: MANIFEST_URL.into(),
        })
    }

    pub async fn fetch(&self) -> Result<VerifiedRelease> {
        let bytes = self.small(&self.endpoint, MAX_MANIFEST_BYTES).await?;
        let signature = self.small(&format!("{}.sig", self.endpoint), 4096).await?;
        let signature =
            std::str::from_utf8(&signature).map_err(|e| Error::Signature(e.to_string()))?;
        verify_manifest(&bytes, signature)
    }

    async fn small(&self, url: &str, limit: u64) -> Result<Vec<u8>> {
        let mut response = self
            .http
            .get(url)
            .timeout(Duration::from_secs(30))
            .send()
            .await?
            .error_for_status()?;
        if response.content_length().is_some_and(|n| n > limit) {
            return Err(Error::Integrity);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if (bytes.len() + chunk.len()) as u64 > limit {
                return Err(Error::Integrity);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    /// Creates a fresh owner-only stage. On failure removes only the file this call
    /// created; an existing stage is never truncated or followed through a symlink.
    pub async fn download(&self, artifact: &Artifact, destination: &Path) -> Result<()> {
        if artifact.size == 0 || artifact.size > MAX_ARTIFACT_BYTES {
            return Err(Error::Integrity);
        }
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(destination).await?;
        let result = self.write_download(artifact, &mut file).await;
        drop(file);
        if result.is_err() {
            let _ = tokio::fs::remove_file(destination).await;
        }
        result
    }

    async fn write_download(&self, artifact: &Artifact, file: &mut tokio::fs::File) -> Result<()> {
        let mut response = self
            .http
            .get(&artifact.url)
            .send()
            .await?
            .error_for_status()?;
        if response
            .content_length()
            .is_some_and(|n| n != artifact.size)
        {
            return Err(Error::Integrity);
        }
        let mut digest = Sha256::new();
        let mut size = 0_u64;
        while let Some(chunk) = response.chunk().await? {
            size += chunk.len() as u64;
            if size > artifact.size {
                return Err(Error::Integrity);
            }
            digest.update(&chunk);
            file.write_all(&chunk).await?;
        }
        if size != artifact.size
            || format!("{:x}", digest.finalize()) != artifact.sha256.to_ascii_lowercase()
        {
            return Err(Error::Integrity);
        }
        file.sync_all().await?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn local(endpoint: String) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        Self {
            http: reqwest::Client::new(),
            endpoint,
        }
    }
}

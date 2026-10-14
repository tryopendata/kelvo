//! Dedicated bounded Docker storage and explicit-action lanes. Monitoring stays GET-only.
use crate::docker::{BoundedIo, DockerError, bounded, now_ms, valid_id};
use bollard::{API_DEFAULT_VERSION, BollardRequest, Docker, query_parameters::*};
use kelvo_schema::{docker::DockerEngine, docker_storage::*};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::net::UnixStream;

const MAX_ROWS: usize = 10_000;
const MAX_INSPECTS: usize = 256;
const DEADLINE: Duration = Duration::from_secs(60);

enum MutationOutcome {
    Accepted(Option<f64>),
    NoRemoval,
}

#[derive(Clone)]
pub struct DockerStorageAdapter {
    scan_client: Docker,
    action_client: Docker,
    pub engine: DockerEngine,
    exact_cache: bool,
    scan_permit: Arc<tokio::sync::Semaphore>,
    action_permit: Arc<tokio::sync::Semaphore>,
}

mod actions;
mod inventory;
mod request_policy;

use inventory::{bytes, classify, volume_fingerprint};
#[cfg(test)]
use inventory::{database, timestamp};
#[cfg(test)]
use request_policy::{allowed, literal_cache_filter, unversioned};
use request_policy::{cache_pattern, client, image_id, volume_id};

impl DockerStorageAdapter {
    pub async fn connect(engine: DockerEngine) -> Result<Self, DockerError> {
        tokio::time::timeout(DEADLINE, Self::connect_inner(engine))
            .await
            .map_err(|_| DockerError::Timeout)?
    }
    async fn connect_inner(mut engine: DockerEngine) -> Result<Self, DockerError> {
        let scan_client = client(engine.socket.clone(), false, false)?
            .negotiate_version()
            .await?;
        let version = scan_client.version().await?;
        let api = version.api_version.as_deref().unwrap_or("");
        let minor = api
            .strip_prefix("1.")
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or(DockerError::Incompatible)?;
        if minor < 40 {
            return Err(DockerError::Incompatible);
        }
        let info = scan_client.info().await?;
        let id = info
            .id
            .filter(|s| !s.is_empty())
            .ok_or(DockerError::Invalid)?;
        if engine
            .daemon_id
            .as_ref()
            .is_some_and(|expected| expected != &id)
        {
            return Err(DockerError::Unavailable);
        }
        engine.id = id.clone();
        engine.daemon_id = Some(id);
        engine.api_version = Some(format!("1.{}", minor.min(49)));
        // Pinned to disposable-fixture validation; other implementations remain read-only for cache.
        let exact_cache = version.version.as_deref() == Some("29.4.0")
            && version.git_commit.as_deref() == Some("daa0cb7f");
        let action_client = client(engine.socket.clone(), true, exact_cache)?
            .negotiate_version()
            .await?;
        Ok(Self {
            scan_client,
            action_client,
            engine,
            exact_cache,
            scan_permit: Arc::new(tokio::sync::Semaphore::new(1)),
            action_permit: Arc::new(tokio::sync::Semaphore::new(1)),
        })
    }
    pub async fn scan(&self, generation: u32) -> Result<DockerStorageSnapshot, DockerError> {
        let _permit = self
            .scan_permit
            .try_acquire()
            .map_err(|_| DockerError::Busy)?;
        tokio::time::timeout(DEADLINE, self.scan_inner(generation))
            .await
            .map_err(|_| DockerError::Timeout)?
    }
    async fn scan_inner(&self, generation: u32) -> Result<DockerStorageSnapshot, DockerError> {
        if self.scan_client.info().await?.id.as_deref() != Some(self.engine.id.as_str()) {
            return Err(DockerError::Unavailable);
        }
        let df = serde_json::to_value(self.scan_client.df(None::<DataUsageOptions>).await?)
            .map_err(|_| DockerError::Invalid)?;
        let mut details = BTreeMap::new();
        let containers = df.get("Containers").and_then(Value::as_array);
        let mut complete = containers.is_some();
        for row in containers.into_iter().flatten().take(MAX_INSPECTS) {
            let Some(id) = row
                .get("Id")
                .and_then(Value::as_str)
                .filter(|s| valid_id(s))
            else {
                complete = false;
                continue;
            };
            match self
                .scan_client
                .inspect_container(id, None::<InspectContainerOptions>)
                .await
            {
                Ok(detail) => {
                    details.insert(
                        id.to_string(),
                        serde_json::to_value(detail).map_err(|_| DockerError::Invalid)?,
                    );
                }
                Err(_) => complete = false,
            }
        }
        if containers.is_some_and(|rows| rows.len() > MAX_INSPECTS) {
            complete = false;
        }
        Ok(classify(
            &df,
            &details,
            &self.engine.id,
            generation,
            now_ms(),
            complete,
            self.exact_cache,
        ))
    }
}

#[cfg(test)]
mod tests;

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

fn unversioned(path: &str) -> &str {
    if path.starts_with("/v1.") {
        path.get(1..)
            .and_then(|p| p.find('/').map(|i| &p[i..]))
            .unwrap_or("")
    } else {
        path
    }
}
fn volume_id(id: &str) -> bool {
    id.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && id.len() <= 255
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
fn image_id(id: &str) -> bool {
    id.strip_prefix("sha256:")
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn decode_query(s: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut it = s.bytes();
    while let Some(b) = it.next() {
        bytes.push(if b == b'%' {
            let a = (it.next()? as char).to_digit(16)?;
            let b = (it.next()? as char).to_digit(16)?;
            (a * 16 + b) as u8
        } else if b == b'+' {
            b' '
        } else {
            b
        });
    }
    String::from_utf8(bytes).ok()
}
fn query_map(query: Option<&str>) -> Option<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for part in query.unwrap_or("").split('&').filter(|s| !s.is_empty()) {
        let (k, v) = part.split_once('=')?;
        if out.insert(decode_query(k)?, decode_query(v)?).is_some() {
            return None;
        }
    }
    Some(out)
}
fn cache_pattern(id: &str) -> Option<String> {
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        return None;
    }
    let mut out = String::from("^");
    for c in id.chars() {
        if r"\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('$');
    Some(out)
}
/// Only one literal, anchored cache ID; reject regex constructs and extra filters.
fn literal_cache_filter(value: &str) -> bool {
    let Ok(v) = serde_json::from_str::<BTreeMap<String, Vec<String>>>(value) else {
        return false;
    };
    if v.len() != 1 {
        return false;
    }
    let Some(values) = v.get("id") else {
        return false;
    };
    if values.len() != 1 {
        return false;
    }
    let Some(pattern) = values.first() else {
        return false;
    };
    let Some(inner) = pattern.strip_prefix('^').and_then(|s| s.strip_suffix('$')) else {
        return false;
    };
    let mut id = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let Some(next) = chars.next() else {
                return false;
            };
            id.push(next);
        } else {
            id.push(c);
        }
    }
    cache_pattern(&id).as_ref() == Some(pattern)
}
fn allowed(
    method: &hyper::Method,
    path: &str,
    query: Option<&str>,
    actions: bool,
    exact_cache: bool,
) -> bool {
    let path = unversioned(path);
    let Some(q) = query_map(query) else {
        return false;
    };
    let p: Vec<_> = path.split('/').collect();
    if method == hyper::Method::GET {
        return q.is_empty()
            && (matches!(path, "/version" | "/info" | "/system/df")
                || (p.len() == 4
                    && p.get(1) == Some(&"containers")
                    && p.get(2).is_some_and(|s| valid_id(s))
                    && p.get(3) == Some(&"json"))
                || (p.len() == 3
                    && p.get(1) == Some(&"volumes")
                    && p.get(2).is_some_and(|s| volume_id(s))));
    }
    if !actions {
        return false;
    }
    let equals = |values: &[(&str, &str)]| {
        q.len() == values.len()
            && values
                .iter()
                .all(|(k, v)| q.get(*k).is_some_and(|actual| actual == v))
    };
    if method == hyper::Method::DELETE && p.len() == 3 {
        return match (p.get(1), p.get(2)) {
            (Some(&"containers"), Some(id)) => {
                valid_id(id) && equals(&[("v", "false"), ("force", "false"), ("link", "false")])
            }
            (Some(&"images"), Some(id)) => {
                image_id(id) && equals(&[("force", "false"), ("noprune", "true")])
            }
            (Some(&"volumes"), Some(id)) => volume_id(id) && equals(&[("force", "false")]),
            _ => false,
        };
    }
    if method == hyper::Method::POST
        && p.len() == 4
        && p.get(1) == Some(&"containers")
        && p.get(2).is_some_and(|s| valid_id(s))
    {
        return match p.get(3) {
            Some(&"stop") => equals(&[("t", "30")]),
            Some(&"start") => q.is_empty(),
            _ => false,
        };
    }
    method == hyper::Method::POST
        && path == "/build/prune"
        && exact_cache
        && q.len() == 2
        && q.get("all").is_some_and(|s| s == "true")
        && q.get("filters").is_some_and(|s| literal_cache_filter(s))
}
fn client(socket: String, actions: bool, exact_cache: bool) -> Result<Docker, DockerError> {
    if !Path::new(&socket).is_absolute() || socket.contains("://") {
        return Err(DockerError::Endpoint);
    }
    Ok(Docker::connect_with_custom_transport(
        move |mut req: BollardRequest| {
            let socket = socket.clone();
            async move {
                if !allowed(
                    req.method(),
                    req.uri().path(),
                    req.uri().query(),
                    actions,
                    exact_cache,
                ) {
                    return Err(bollard::errors::Error::IOError {
                        err: std::io::Error::other("Docker storage endpoint denied"),
                    });
                }
                let stream = UnixStream::connect(socket)
                    .await
                    .map_err(|err| bollard::errors::Error::IOError { err })?;
                let io = hyper_util::rt::TokioIo::new(BoundedIo {
                    stream,
                    left: 16 * 1024 * 1024,
                });
                let (mut sender, connection) = hyper::client::conn::http1::handshake(io).await?;
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                let path = req.uri().path_and_query().cloned().ok_or_else(|| {
                    bollard::errors::Error::IOError {
                        err: std::io::Error::other("Invalid Docker URI"),
                    }
                })?;
                *req.uri_mut() = hyper::Uri::builder().path_and_query(path).build()?;
                sender.send_request(req).await.map_err(Into::into)
            }
        },
        Some("http://localhost"),
        60,
        API_DEFAULT_VERSION,
    )?)
}
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
    pub async fn execute(
        &self,
        selection: &DockerStorageSelection,
        reviewed: &DockerStorageResource,
        can_dispatch: impl Fn() -> bool,
    ) -> DockerStorageItemResult {
        let result = |status, message: String, reclaimed_bytes| DockerStorageItemResult {
            selection: selection.clone(),
            status,
            message,
            reclaimed_bytes,
        };
        let Ok(_permit) = self.action_permit.try_acquire() else {
            return result(
                DockerStorageItemStatus::Skipped,
                "Another Docker action is running".into(),
                None,
            );
        };
        if selection.kind != reviewed.kind || selection.id != reviewed.id {
            return result(
                DockerStorageItemStatus::Skipped,
                "Selection does not match review".into(),
                None,
            );
        }
        let fresh = match self.scan(0).await {
            Ok(snapshot) => snapshot,
            Err(_) => {
                return result(
                    DockerStorageItemStatus::Skipped,
                    "Cannot refresh references and identity".into(),
                    None,
                );
            }
        };
        let Some(current) = fresh
            .resources
            .iter()
            .find(|r| r.kind == selection.kind && r.id == selection.id)
        else {
            return result(
                DockerStorageItemStatus::Skipped,
                "Resource no longer exists".into(),
                None,
            );
        };
        let eligible = match selection.action {
            DockerStorageAction::Start => false,
            DockerStorageAction::Delete => current.can_delete && reviewed.can_delete,
            DockerStorageAction::Stop => current.can_stop && reviewed.can_stop,
        };
        if !eligible || current.fingerprint != reviewed.fingerprint || !fresh.complete {
            return result(
                DockerStorageItemStatus::Skipped,
                "Resource changed, is in use, or has incomplete evidence; review again".into(),
                None,
            );
        }
        // Volume names are reusable. This narrows, but cannot eliminate, Docker's check/delete race.
        if selection.kind == DockerStorageKind::Volume {
            let inspected = match self.scan_client.inspect_volume(&selection.id).await {
                Ok(v) => serde_json::to_value(v).ok(),
                Err(_) => None,
            };
            if inspected.as_ref().map(volume_fingerprint).as_ref() != Some(&reviewed.fingerprint) {
                return result(
                    DockerStorageItemStatus::Skipped,
                    "Volume identity changed or is unavailable".into(),
                    None,
                );
            }
        }
        if self
            .scan_client
            .info()
            .await
            .ok()
            .and_then(|info| info.id)
            .as_deref()
            != Some(self.engine.id.as_str())
        {
            return result(
                DockerStorageItemStatus::Skipped,
                "Docker daemon identity changed or is unavailable".into(),
                None,
            );
        }
        if !can_dispatch() {
            return result(
                DockerStorageItemStatus::Skipped,
                "Engine generation or availability changed".into(),
                None,
            );
        }
        self.dispatch(selection).await
    }
    /// Revalidate only the selected container. This lane never scans storage.
    pub async fn container_action(
        &self,
        container_id: &str,
        action: DockerContainerAction,
        can_dispatch: impl Fn() -> bool,
    ) -> DockerStorageItemResult {
        let selection = DockerStorageSelection {
            kind: DockerStorageKind::Container,
            id: container_id.into(),
            action: action.into(),
        };
        let skipped = |message: &str| DockerStorageItemResult {
            selection: selection.clone(),
            status: DockerStorageItemStatus::Skipped,
            message: message.into(),
            reclaimed_bytes: None,
        };
        let Ok(_permit) = self.action_permit.try_acquire() else {
            return skipped("Another Docker action is running");
        };
        if !valid_id(container_id) {
            return skipped("Container identity is invalid");
        }
        let inspected = match self
            .scan_client
            .inspect_container(container_id, None::<InspectContainerOptions>)
            .await
        {
            Ok(container) => container,
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {
                return skipped("Container no longer exists");
            }
            Err(_) => return skipped("Container information is unavailable; no action was sent"),
        };
        if inspected.id.as_deref() != Some(container_id) {
            return skipped("Container identity changed or is unavailable");
        }
        let lifecycle = inspected
            .state
            .and_then(|state| state.status)
            .map(|status| status.to_string());
        match action {
            DockerContainerAction::Start => {
                if !matches!(lifecycle.as_deref(), Some("exited" | "created")) {
                    return skipped("Only an exited or newly created container can be started");
                }
            }
            DockerContainerAction::Stop => {
                if !matches!(
                    lifecycle.as_deref(),
                    Some("running" | "restarting" | "paused")
                ) {
                    return skipped("Container is no longer running");
                }
                match inspected.host_config.and_then(|config| config.auto_remove) {
                    Some(false) => (),
                    Some(true) => {
                        return skipped(
                            "Stop is unavailable because Docker would automatically remove this container",
                        );
                    }
                    None => {
                        return skipped(
                            "Stop is unavailable because the container host configuration is unknown",
                        );
                    }
                }
            }
        }
        if self
            .scan_client
            .info()
            .await
            .ok()
            .and_then(|info| info.id)
            .as_deref()
            != Some(self.engine.id.as_str())
        {
            return skipped("Docker engine changed or is unavailable");
        }
        if !can_dispatch() {
            return skipped("Docker engine changed, paused, or disconnected before dispatch");
        }
        self.dispatch(&selection).await
    }
    async fn dispatch(&self, selection: &DockerStorageSelection) -> DockerStorageItemResult {
        let result = |status, message: String, reclaimed_bytes| DockerStorageItemResult {
            selection: selection.clone(),
            status,
            message,
            reclaimed_bytes,
        };
        let response = tokio::time::timeout(DEADLINE, self.mutate(selection)).await;
        match response {
            Ok(Ok(MutationOutcome::NoRemoval)) => result(
                DockerStorageItemStatus::Skipped,
                "Docker did not remove the selected cache; it may now be in use. Refresh and review again".into(),
                None,
            ),
            Ok(Ok(MutationOutcome::Accepted(bytes))) => result(
                DockerStorageItemStatus::Succeeded,
                "Docker accepted the action".into(),
                bytes,
            ),
            Ok(Err(bollard::errors::Error::DockerResponseServerError { status_code, message })) => {
                result(
                    if status_code == 404 || status_code == 409 {
                        DockerStorageItemStatus::Skipped
                    } else {
                        DockerStorageItemStatus::Failed
                    },
                    format!("Docker returned HTTP {status_code}: {}; no retry was sent", bounded(&message, 512)),
                    None,
                )
            }
            _ => result(
                DockerStorageItemStatus::OutcomeUnknown,
                "Connection or deadline ended after dispatch; rescan before further action".into(),
                None,
            ),
        }
    }
    async fn mutate(
        &self,
        selection: &DockerStorageSelection,
    ) -> Result<MutationOutcome, bollard::errors::Error> {
        match (selection.kind, selection.action) {
            (DockerStorageKind::Container, DockerStorageAction::Start) => {
                self.action_client
                    .start_container(&selection.id, None::<StartContainerOptions>)
                    .await?
            }
            (DockerStorageKind::Container, DockerStorageAction::Stop) => {
                self.action_client
                    .stop_container(
                        &selection.id,
                        Some(StopContainerOptions {
                            t: Some(30),
                            signal: None,
                        }),
                    )
                    .await?
            }
            (DockerStorageKind::Container, DockerStorageAction::Delete) => {
                self.action_client
                    .remove_container(
                        &selection.id,
                        Some(RemoveContainerOptions {
                            force: false,
                            v: false,
                            link: false,
                        }),
                    )
                    .await?
            }
            (DockerStorageKind::Image, DockerStorageAction::Delete) => {
                self.action_client
                    .remove_image(
                        &selection.id,
                        Some(RemoveImageOptions {
                            force: false,
                            noprune: true,
                        }),
                        None,
                    )
                    .await?;
            }
            (DockerStorageKind::Volume, DockerStorageAction::Delete) => {
                self.action_client
                    .remove_volume(&selection.id, Some(RemoveVolumeOptions { force: false }))
                    .await?
            }
            (DockerStorageKind::BuildCache, DockerStorageAction::Delete) if self.exact_cache => {
                let pattern = cache_pattern(&selection.id).ok_or_else(|| {
                    bollard::errors::Error::IOError {
                        err: std::io::Error::other("Invalid cache ID"),
                    }
                })?;
                let response = self
                    .action_client
                    .prune_build(Some(PruneBuildOptions {
                        all: Some(true),
                        filters: Some(HashMap::from([("id".into(), vec![pattern])])),
                        ..Default::default()
                    }))
                    .await?;
                if response
                    .caches_deleted
                    .as_ref()
                    .is_some_and(|ids| ids.iter().any(|id| id != &selection.id))
                {
                    return Err(bollard::errors::Error::IOError {
                        err: std::io::Error::other(
                            "Daemon reported cache deletion outside selection",
                        ),
                    });
                }
                if !response
                    .caches_deleted
                    .as_ref()
                    .is_some_and(|ids| ids.contains(&selection.id))
                {
                    return Ok(MutationOutcome::NoRemoval);
                }
                return Ok(MutationOutcome::Accepted(bytes(response.space_reclaimed)));
            }
            _ => {
                return Err(bollard::errors::Error::IOError {
                    err: std::io::Error::other("Unsupported action"),
                });
            }
        }
        Ok(MutationOutcome::Accepted(None))
    }
}
fn bytes(v: Option<i64>) -> Option<f64> {
    v.filter(|n| *n >= 0).map(|n| n as f64)
}
fn number(v: &Value, key: &str) -> Option<f64> {
    bytes(v.get(key).and_then(Value::as_i64))
}
fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(|s| bounded(s, 1024))
}
fn fingerprint(v: Value) -> String {
    format!("{:x}", Sha256::digest(v.to_string().as_bytes()))
}
fn volume_fingerprint(v: &Value) -> String {
    fingerprint(json!([
        v.get("Name"),
        v.get("CreatedAt"),
        v.get("Driver"),
        v.get("Options"),
        v.get("Labels")
    ]))
}
fn project(v: &Value) -> Option<String> {
    v.get("Labels")
        .and_then(|l| text(l, "com.docker.compose.project"))
}
fn database(image: &str) -> Option<String> {
    let repo = image.split('@').next()?.rsplit(':').next_back()?;
    let repo = repo
        .strip_prefix("docker.io/")
        .unwrap_or(repo)
        .strip_prefix("library/")
        .unwrap_or(repo.strip_prefix("docker.io/").unwrap_or(repo));
    match repo {
        "postgres"
        | "mysql"
        | "mariadb"
        | "mongo"
        | "redis"
        | "valkey/valkey"
        | "couchdb"
        | "pgvector/pgvector"
        | "cassandra"
        | "clickhouse/clickhouse-server" => Some(repo.into()),
        _ => None,
    }
}
fn mounts(v: &Value) -> Vec<DockerStorageMount> {
    v.get("Mounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|m| DockerStorageMount {
            kind: text(m, "Type").unwrap_or_else(|| "unknown".into()),
            name: text(m, "Name"),
            source: text(m, "Source"),
            destination: text(m, "Destination"),
        })
        .collect()
}
fn resource(kind: DockerStorageKind, id: String) -> DockerStorageResource {
    DockerStorageResource {
        kind,
        name: id.clone(),
        id,
        project: None,
        size_bytes: None,
        reclaimable_bytes: None,
        references: vec![],
        age_ms: None,
        age_basis: None,
        database: None,
        mounts: vec![],
        auto_remove: None,
        can_delete: false,
        can_stop: false,
        reason: None,
        fingerprint: String::new(),
    }
}
/// Docker emits RFC3339 UTC timestamps. Unknown/non-UTC forms stay unknown.
fn timestamp(s: &str) -> Option<i64> {
    if !s.ends_with('Z') {
        return None;
    }
    let (date, time) = s.trim_end_matches('Z').split_once('T')?;
    let d: Vec<i64> = date
        .split('-')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let t: Vec<i64> = time
        .split('.')
        .next()?
        .split(':')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let [year, month, day] = d.as_slice() else {
        return None;
    };
    let [hour, minute, second] = t.as_slice() else {
        return None;
    };
    if !(1970..=9999).contains(year)
        || !(1..=12).contains(month)
        || !(1..=31).contains(day)
        || !(0..=23).contains(hour)
        || !(0..=59).contains(minute)
        || !(0..=59).contains(second)
    {
        return None;
    }
    // Gregorian civil date -> Unix days (400-year eras).
    let y = year - i64::from(*month <= 2);
    let era = y / 400;
    let yo = y - era * 400;
    let mp = month + if *month > 2 { -3 } else { 9 };
    let days = era * 146097 + yo * 365 + yo / 4 - yo / 100 + (153 * mp + 2) / 5 + day - 1 - 719468;
    days.checked_mul(86400)?
        .checked_add(hour * 3600 + minute * 60 + second)?
        .checked_mul(1000)
}
fn age(v: &Value, key: &str, now: i64) -> Option<i64> {
    timestamp(v.get(key)?.as_str()?)
        .filter(|time| *time <= now)
        .map(|time| now - time)
}
fn classify(
    df: &Value,
    details: &BTreeMap<String, Value>,
    engine: &str,
    generation: u32,
    now: i64,
    mut complete: bool,
    exact_cache: bool,
) -> DockerStorageSnapshot {
    let mut rows = Vec::new();
    let containers = df.get("Containers").and_then(Value::as_array);
    let counts = ["Containers", "Images", "Volumes", "BuildCache"]
        .iter()
        .map(|key| df.get(key).and_then(Value::as_array).map_or(0, Vec::len))
        .sum::<usize>();
    if counts > MAX_ROWS || containers.is_none() {
        complete = false;
    }
    for c in containers.into_iter().flatten().take(MAX_ROWS) {
        let Some(id) = text(c, "Id").filter(|s| valid_id(s)) else {
            complete = false;
            continue;
        };
        let detail = details.get(&id);
        if detail.is_none_or(|d| {
            d.get("Id").and_then(Value::as_str) != Some(&id)
                || d.get("Mounts").and_then(Value::as_array).is_none()
        }) || c.get("ImageID").and_then(Value::as_str).is_none()
        {
            complete = false;
        }
        let mut r = resource(DockerStorageKind::Container, id);
        r.name = c
            .get("Names")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .map(|s| bounded(s.trim_start_matches('/'), 512))
            .unwrap_or(r.name);
        r.project = project(c);
        r.size_bytes = number(c, "SizeRw");
        r.mounts = mounts(detail.unwrap_or(c));
        r.database = text(c, "Image").and_then(|s| database(&s));
        let state = detail.and_then(|d| d.get("State"));
        r.auto_remove = detail
            .and_then(|d| d.get("HostConfig"))
            .and_then(|h| h.get("AutoRemove"))
            .and_then(Value::as_bool);
        let stopped = state
            .and_then(|s| s.get("Status"))
            .and_then(Value::as_str)
            .is_some_and(|s| matches!(s, "exited" | "created" | "dead"));
        let running =
            state.and_then(|s| s.get("Status")).and_then(Value::as_str) == Some("running");
        r.can_delete = stopped;
        r.can_stop = running && r.auto_remove == Some(false);
        r.reclaimable_bytes = Some(0.0);
        if stopped {
            r.age_ms = state.and_then(|s| age(s, "FinishedAt", now));
            r.age_basis = Some("stopped".into());
            r.reclaimable_bytes = r.size_bytes;
        }
        r.reason = if r.auto_remove == Some(true) {
            Some(
                "Stop is disabled: AutoRemove can delete the container and anonymous volumes"
                    .into(),
            )
        } else if !stopped && !r.can_stop {
            Some("Container state or AutoRemove metadata is unavailable".into())
        } else if running {
            Some("Running container; stop it before deletion".into())
        } else {
            None
        };
        r.fingerprint = fingerprint(json!([
            r.id,
            detail.and_then(|d| d.get("Created")),
            state.and_then(|s| s.get("Status")),
            state.and_then(|s| s.get("StartedAt")),
            state.and_then(|s| s.get("FinishedAt")),
            r.auto_remove,
            r.mounts
        ]));
        rows.push(r);
    }
    for i in df
        .get("Images")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_ROWS.saturating_sub(rows.len()))
    {
        let Some(id) = text(i, "Id").filter(|s| image_id(s)) else {
            complete = false;
            continue;
        };
        let mut r = resource(DockerStorageKind::Image, id);
        r.name = i
            .get("RepoTags")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .map(|s| bounded(s, 1024))
            .unwrap_or(r.name);
        r.project = project(i);
        r.size_bytes = number(i, "Size");
        r.references = containers
            .into_iter()
            .flatten()
            .filter(|c| c.get("ImageID").and_then(Value::as_str) == Some(&r.id))
            .filter_map(|c| text(c, "Id"))
            .collect();
        r.can_delete =
            r.references.is_empty() && i.get("Containers").and_then(Value::as_i64) == Some(0);
        // Unique layers are conservative; shared ownership is never added per image.
        r.reclaimable_bytes = if r.can_delete {
            r.size_bytes
                .zip(number(i, "SharedSize"))
                .and_then(|(size, shared)| (shared <= size).then_some(size - shared))
        } else {
            Some(0.0)
        };
        r.reason =
            (!r.can_delete).then(|| "Image is referenced or reference count is unknown".into());
        r.fingerprint = fingerprint(json!([
            r.id,
            i.get("RepoTags"),
            i.get("RepoDigests"),
            i.get("Created")
        ]));
        rows.push(r);
    }
    for v in df
        .get("Volumes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_ROWS.saturating_sub(rows.len()))
    {
        let Some(id) = text(v, "Name").filter(|s| volume_id(s)) else {
            complete = false;
            continue;
        };
        let mut r = resource(DockerStorageKind::Volume, id);
        r.project = project(v);
        r.size_bytes = v.get("UsageData").and_then(|v| number(v, "Size"));
        r.references = rows
            .iter()
            .filter(|c| {
                c.kind == DockerStorageKind::Container
                    && c.mounts
                        .iter()
                        .any(|m| m.kind == "volume" && m.name.as_ref() == Some(&r.id))
            })
            .map(|c| c.id.clone())
            .collect();
        r.database = rows
            .iter()
            .find(|c| r.references.contains(&c.id) && c.database.is_some())
            .and_then(|c| c.database.clone());
        r.can_delete = r.references.is_empty()
            && v.get("UsageData")
                .and_then(|v| v.get("RefCount"))
                .and_then(Value::as_i64)
                == Some(0)
            && v.get("CreatedAt").and_then(Value::as_str).is_some();
        let default_local = v.get("Driver").and_then(Value::as_str) == Some("local")
            && v.get("Options")
                .and_then(Value::as_object)
                .is_some_and(|options| options.is_empty());
        r.reclaimable_bytes = if !default_local {
            None
        } else if r.can_delete {
            r.size_bytes
        } else {
            Some(0.0)
        };
        r.reason = Some(if !r.can_delete {
            "Volume has container references or incomplete identity/reference metadata"
        } else if !default_local {
            "Driver or mount options determine deletion effects; reclaimed bytes are unknown. Previous use is unknown. Docker cannot prevent a concurrent replacement by another client."
        } else {
            "Previous use unknown; deleting permanently loses its data. Docker cannot prevent a concurrent replacement by another client."
        }.into());
        r.fingerprint = volume_fingerprint(v);
        rows.push(r);
    }
    for c in df
        .get("BuildCache")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_ROWS.saturating_sub(rows.len()))
    {
        let Some(id) = text(c, "ID").filter(|s| cache_pattern(s).is_some()) else {
            complete = false;
            continue;
        };
        let mut r = resource(DockerStorageKind::BuildCache, id);
        r.size_bytes = number(c, "Size");
        r.age_ms = age(c, "LastUsedAt", now);
        r.age_basis = Some("last_used".into());
        let unused = c.get("InUse").and_then(Value::as_bool) == Some(false);
        r.can_delete = exact_cache && unused;
        r.reclaimable_bytes = match (
            c.get("InUse").and_then(Value::as_bool),
            c.get("Shared").and_then(Value::as_bool),
        ) {
            (Some(false), Some(false)) => r.size_bytes,
            (Some(true), _) | (_, Some(true)) => Some(0.0),
            _ => None,
        };
        r.reason = if !exact_cache {
            Some("Exact cache cleanup is unsupported for this engine build".into())
        } else if !unused {
            Some("Cache is in use or usage is unknown".into())
        } else {
            None
        };
        r.fingerprint = fingerprint(json!([
            r.id,
            c.get("CreatedAt"),
            c.get("LastUsedAt"),
            c.get("InUse"),
            c.get("Parents"),
            c.get("Parent")
        ]));
        rows.push(r);
    }
    if !complete {
        for r in &mut rows {
            r.can_delete = false;
            r.can_stop = false;
            r.reason = Some("Incomplete reference inventory; refresh before any action".into());
        }
    }
    let mut categories = Vec::new();
    for (kind, key) in [
        (DockerStorageKind::Container, "Containers"),
        (DockerStorageKind::Image, "Images"),
        (DockerStorageKind::Volume, "Volumes"),
        (DockerStorageKind::BuildCache, "BuildCache"),
    ] {
        let known = df.get(key).and_then(Value::as_array).is_some() && counts <= MAX_ROWS;
        let selected: Vec<_> = rows.iter().filter(|r| r.kind == kind).collect();
        let size_bytes = if !known {
            None
        } else if kind == DockerStorageKind::Image {
            number(df, "LayersSize")
        } else {
            selected.iter().map(|r| r.size_bytes).sum::<Option<f64>>()
        };
        let reclaimable_bytes = if known && complete {
            selected
                .iter()
                .map(|r| r.reclaimable_bytes)
                .sum::<Option<f64>>()
        } else {
            None
        };
        categories.push(DockerStorageCategory {
            kind,
            size_bytes,
            reclaimable_bytes,
        });
    }
    let mut warnings = vec!["Docker-managed storage excludes VM allocation, logs, bind-mounted host files and other Buildx builders. Image and cache row sizes can overlap; do not add them.".into()];
    if !complete {
        warnings.push("Inventory is incomplete; actions are disabled. Scan admits at most 10,000 resources and 256 container inspections.".into());
    }
    if !exact_cache {
        warnings.push(
            "Cache cleanup requires an engine build with verified exact-ID filtering.".into(),
        );
    }
    DockerStorageSnapshot {
        engine_id: engine.into(),
        generation,
        observed_ms: now,
        complete,
        resources: rows,
        categories,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelvo_schema::lock::LockExt;
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn fixture() -> (Value, BTreeMap<String, Value>) {
        let image = format!("sha256:{}", "a".repeat(64));
        let df = json!({"LayersSize":150,"Images":[{"Id":image,"ParentId":"","RepoTags":["postgres:16"],"RepoDigests":[],"Created":100,"Size":100,"SharedSize":50,"Labels":{},"Containers":1},{"Id":format!("sha256:{}","b".repeat(64)),"ParentId":"","RepoTags":[],"RepoDigests":[],"Created":100,"Size":100,"SharedSize":50,"Labels":{},"Containers":0}],"Containers":[{"Id":"abc123","Image":"postgres:16","ImageID":image,"Names":["/db"],"SizeRw":20,"State":"exited","Mounts":[{"Type":"volume","Name":"database","Destination":"/data"}],"Labels":{}}],"Volumes":[{"Name":"database","Driver":"local","Scope":"local","Mountpoint":"/data","CreatedAt":"2026-01-01T00:00:00Z","Labels":{},"Options":{},"UsageData":{"Size":1000,"RefCount":1}},{"Name":"unused","Driver":"local","Scope":"local","Mountpoint":"/unused","CreatedAt":"2026-01-01T00:00:00Z","Labels":{},"Options":{},"UsageData":{"Size":80,"RefCount":0}}],"BuildCache":[{"ID":"cache.*one","Size":40,"InUse":false,"Shared":false,"CreatedAt":"2026-01-01T00:00:00Z","LastUsedAt":"2026-01-02T00:00:00Z"}]});
        let details = BTreeMap::from([(
            "abc123".into(),
            json!({"Id":"abc123","Created":"2026-01-01T00:00:00Z","State":{"Status":"exited","StartedAt":"2026-01-01T00:00:00Z","FinishedAt":"2026-01-02T00:00:00Z"},"HostConfig":{"AutoRemove":false},"Mounts":[{"Type":"volume","Name":"database","Destination":"/data"}]}),
        )]);
        (df, details)
    }
    fn snapshot(df: &Value, details: &BTreeMap<String, Value>) -> DockerStorageSnapshot {
        classify(
            df,
            details,
            "fake",
            1,
            timestamp("2026-01-10T00:00:00Z").unwrap(),
            true,
            true,
        )
    }
    fn row(s: &DockerStorageSnapshot, id: &str) -> DockerStorageResource {
        s.resources.iter().find(|r| r.id == id).unwrap().clone()
    }
    #[test]
    fn classification_preserves_stopped_references_and_shared_accounting() {
        let (df, details) = fixture();
        let s = snapshot(&df, &details);
        assert!(s.complete);
        let volume = row(&s, "database");
        assert_eq!(volume.references, vec!["abc123"]);
        assert!(!volume.can_delete);
        assert_eq!(volume.database.as_deref(), Some("postgres"));
        assert_eq!(row(&s, "abc123").age_ms, Some(8 * 86400 * 1000));
        assert_eq!(
            s.categories
                .iter()
                .find(|c| c.kind == DockerStorageKind::Image)
                .unwrap()
                .size_bytes,
            Some(150.0)
        );
        assert_eq!(
            s.categories
                .iter()
                .find(|c| c.kind == DockerStorageKind::Image)
                .unwrap()
                .reclaimable_bytes,
            Some(50.0)
        );
        assert!(row(&s, "unused").can_delete);
        assert_eq!(row(&s, "unused").age_ms, None);
        assert!(
            s.resources
                .iter()
                .filter(|r| r.kind == DockerStorageKind::Image)
                .all(|r| r.age_ms.is_none())
        );
    }
    #[test]
    fn mixed_running_and_shared_resources_do_not_hide_conservative_estimates() {
        let (mut df, mut details) = fixture();
        details.get_mut("abc123").unwrap()["State"]["Status"] = json!("running");
        df["BuildCache"]
            .as_array_mut()
            .unwrap()
            .push(json!({"ID":"shared","Size":90,"Shared":true,"InUse":false}));
        df["BuildCache"]
            .as_array_mut()
            .unwrap()
            .push(json!({"ID":"active","Size":30,"Shared":false,"InUse":true}));
        let s = snapshot(&df, &details);
        assert_eq!(
            s.categories
                .iter()
                .find(|c| c.kind == DockerStorageKind::Container)
                .unwrap()
                .reclaimable_bytes,
            Some(0.0)
        );
        assert_eq!(
            s.categories
                .iter()
                .find(|c| c.kind == DockerStorageKind::BuildCache)
                .unwrap()
                .reclaimable_bytes,
            Some(40.0)
        );
        df["BuildCache"]
            .as_array_mut()
            .unwrap()
            .push(json!({"ID":"unknown","Size":30}));
        let s = snapshot(&df, &details);
        assert_eq!(
            s.categories
                .iter()
                .find(|c| c.kind == DockerStorageKind::BuildCache)
                .unwrap()
                .reclaimable_bytes,
            None
        );
    }
    #[test]
    fn external_volume_storage_never_promises_reclaimed_bytes() {
        let (mut df, details) = fixture();
        assert_eq!(
            row(&snapshot(&df, &details), "unused").reclaimable_bytes,
            Some(80.0)
        );
        for (driver, options) in [
            (
                "local",
                json!({"type":"none","o":"bind","device":"/host/data"}),
            ),
            ("local", json!({"type":"nfs","device":":/remote"})),
            ("plugin", json!({})),
            ("local", Value::Null),
        ] {
            df["Volumes"][1]["Driver"] = json!(driver);
            df["Volumes"][1]["Options"] = options;
            let volume = row(&snapshot(&df, &details), "unused");
            assert!(volume.can_delete);
            assert_eq!(volume.size_bytes, Some(80.0));
            assert_eq!(volume.reclaimable_bytes, None);
            assert!(
                volume
                    .reason
                    .as_deref()
                    .unwrap()
                    .contains("reclaimed bytes are unknown")
            );
        }
        assert_eq!(
            database("pgvector/pgvector:pg17"),
            Some("pgvector/pgvector".into())
        );
        assert_eq!(database("evil/pgvector:pg17"), None);
    }
    #[test]
    fn missing_inventory_metadata_and_unknown_sizes_never_become_zero_evidence() {
        let (mut df, mut details) = fixture();
        df["Volumes"][1]["UsageData"]["Size"] = json!(-1);
        df["Images"][1]["SharedSize"] = json!(-1);
        let s = snapshot(&df, &details);
        assert_eq!(row(&s, "unused").size_bytes, None);
        assert_eq!(
            s.categories
                .iter()
                .find(|c| c.kind == DockerStorageKind::Image)
                .unwrap()
                .reclaimable_bytes,
            None
        );
        details.clear();
        let s = snapshot(&df, &details);
        assert!(!s.complete);
        assert!(s.resources.iter().all(|r| !r.can_delete && !r.can_stop));
        df.as_object_mut().unwrap().remove("Containers");
        let s = snapshot(&df, &details);
        assert!(!s.complete);
        assert!(!row(&s, "unused").can_delete);
    }
    #[test]
    fn autoremove_and_unknown_hostconfig_disable_stop() {
        let (df, mut details) = fixture();
        details.get_mut("abc123").unwrap()["State"]["Status"] = json!("running");
        assert!(row(&snapshot(&df, &details), "abc123").can_stop);
        details.get_mut("abc123").unwrap()["HostConfig"]["AutoRemove"] = json!(true);
        assert!(!row(&snapshot(&df, &details), "abc123").can_stop);
        details.get_mut("abc123").unwrap()["HostConfig"] = Value::Null;
        assert!(!row(&snapshot(&df, &details), "abc123").can_stop);
    }
    #[test]
    fn ages_use_only_actual_stopped_or_last_used_timestamps() {
        let (mut df, mut details) = fixture();
        details.get_mut("abc123").unwrap()["State"]["FinishedAt"] = json!("0001-01-01T00:00:00Z");
        df["BuildCache"][0]["LastUsedAt"] = Value::Null;
        let s = snapshot(&df, &details);
        assert_eq!(row(&s, "abc123").age_ms, None);
        assert_eq!(row(&s, "cache.*one").age_ms, None);
        assert_eq!(timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(timestamp("2024-02-29T00:00:00Z"), Some(1709164800000));
        assert_eq!(database("evil/postgres:16"), None);
        assert_eq!(
            database("docker.io/library/postgres:16"),
            Some("postgres".into())
        );
    }
    #[test]
    fn closed_allowlist_denies_force_parent_prune_paths_and_query_injection() {
        let digest = format!("/v1.49/images/sha256:{}", "a".repeat(64));
        assert!(allowed(
            &hyper::Method::DELETE,
            &digest,
            Some("force=false&noprune=true"),
            true,
            false
        ));
        assert!(allowed(
            &hyper::Method::DELETE,
            "/containers/abc",
            Some("v=false&force=false&link=false"),
            true,
            false
        ));
        assert!(allowed(
            &hyper::Method::POST,
            "/containers/abc/stop",
            Some("t=30"),
            true,
            false
        ));
        assert!(allowed(
            &hyper::Method::POST,
            "/containers/abc/start",
            None,
            true,
            false
        ));
        assert!(!allowed(
            &hyper::Method::POST,
            "/containers/abc/start",
            None,
            false,
            false
        ));
        for (method, path, query) in [
            (hyper::Method::POST, "/containers/abc/start", "detachKeys=x"),
            (
                hyper::Method::DELETE,
                digest.as_str(),
                "force=true&noprune=true",
            ),
            (
                hyper::Method::DELETE,
                digest.as_str(),
                "force=false&noprune=false",
            ),
            (
                hyper::Method::DELETE,
                "/containers/abc",
                "v=true&force=false&link=false",
            ),
            (
                hyper::Method::DELETE,
                "/volumes/unused",
                "force=false&force=true",
            ),
            (hyper::Method::DELETE, "/volumes/../unused", "force=false"),
            (hyper::Method::DELETE, "/volumes/%2E%2E", "force=false"),
            (hyper::Method::POST, "/containers/abc/stop", "t=0"),
            (
                hyper::Method::POST,
                "/containers/abc/stop",
                "t=30&signal=SIGKILL",
            ),
            (hyper::Method::POST, "/system/prune", ""),
            (hyper::Method::POST, "/build/prune", "all=true"),
            (hyper::Method::GET, "/containers/abc/json", "size=true"),
        ] {
            assert!(
                !allowed(&method, path, Some(query), true, true),
                "{method} {path}?{query}"
            );
        }
        assert!(!allowed(
            &hyper::Method::DELETE,
            "/volumes/unused",
            Some("force=false"),
            false,
            false
        ));
    }
    #[test]
    fn cache_filters_are_literal_single_anchored_ids_and_capability_gated() {
        let pattern = cache_pattern("cache.*one").unwrap();
        assert_eq!(pattern, r"^cache\.\*one$");
        assert!(literal_cache_filter(&json!({"id":[pattern]}).to_string()));
        for v in [
            json!({"id":[".*"]}),
            json!({"id":["^cache.*$"]}),
            json!({"id":["^one$","^two$"]}),
            json!({"id":["^one$"],"until":["1h"]}),
            json!({"id":["^\\d+$"]}),
        ] {
            assert!(!literal_cache_filter(&v.to_string()));
        }
        let q = "all=true&filters=%7B%22id%22%3A%5B%22%5Eone%24%22%5D%7D";
        assert!(allowed(
            &hyper::Method::POST,
            "/build/prune",
            Some(q),
            true,
            true
        ));
        assert!(!allowed(
            &hyper::Method::POST,
            "/build/prune",
            Some(q),
            true,
            false
        ));
        let (df, details) = fixture();
        let s = classify(&df, &details, "fake", 1, now_ms(), true, false);
        assert!(!row(&s, "cache.*one").can_delete);
    }
    struct Fake {
        _dir: tempfile::TempDir,
        engine: DockerEngine,
        df: Arc<Mutex<Value>>,
        details: Arc<Mutex<BTreeMap<String, Value>>>,
        requests: Arc<Mutex<Vec<String>>>,
        revoke_during_inspect: Arc<AtomicBool>,
        dispatch_allowed: Arc<AtomicBool>,
        drop_mutation_response: Arc<AtomicBool>,
        mutation_error: Arc<Mutex<Option<u16>>>,
        cache_response: Arc<Mutex<Value>>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Fake {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    async fn fake() -> Fake {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let (df, details) = fixture();
        let df = Arc::new(Mutex::new(df));
        let details = Arc::new(Mutex::new(details));
        let requests = Arc::new(Mutex::new(vec![]));
        let revoke_during_inspect = Arc::new(AtomicBool::new(false));
        let dispatch_allowed = Arc::new(AtomicBool::new(true));
        let drop_mutation_response = Arc::new(AtomicBool::new(false));
        let cache_response = Arc::new(Mutex::new(
            json!({"CachesDeleted":["cache.*one"],"SpaceReclaimed":40}),
        ));
        let mutation_error = Arc::new(Mutex::new(None));
        let serve_error = mutation_error.clone();
        let serve_cache = cache_response.clone();
        let serve_drop = drop_mutation_response.clone();
        let serve_revoke = revoke_during_inspect.clone();
        let serve_allowed = dispatch_allowed.clone();
        let (serve_df, serve_details, serve_requests) =
            (df.clone(), details.clone(), requests.clone());
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mutation_error = serve_error.clone();
                let cache_response = serve_cache.clone();
                let drop_response = serve_drop.clone();
                let revoke = serve_revoke.clone();
                let dispatch = serve_allowed.clone();
                let (df, details, requests) = (
                    serve_df.clone(),
                    serve_details.clone(),
                    serve_requests.clone(),
                );
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut byte = [0];
                    while stream.read_exact(&mut byte).await.is_ok() {
                        request.push(byte[0]);
                        if request.ends_with(b"\r\n\r\n") || request.len() > 16384 {
                            break;
                        }
                    }
                    let request = String::from_utf8_lossy(&request);
                    let first = request.lines().next().unwrap_or("").to_string();
                    requests.lock_ok().push(first.clone());
                    if drop_response.load(Ordering::SeqCst)
                        && (first.starts_with("DELETE") || first.starts_with("POST"))
                    {
                        return;
                    }
                    let path = first.split_whitespace().nth(1).unwrap_or("");
                    let path = unversioned(path.split('?').next().unwrap_or(""));
                    let error_status = if first.starts_with("POST") || first.starts_with("DELETE") {
                        *mutation_error.lock_ok()
                    } else if path.starts_with("/containers/") && details.lock_ok().is_empty() {
                        Some(404)
                    } else {
                        None
                    };
                    let body = if error_status.is_some() {
                        json!({"message":"Port 8080 is already in use"})
                    } else if path == "/version" {
                        json!({"Version":"29.4.0","GitCommit":"daa0cb7f","ApiVersion":"1.49","MinAPIVersion":"1.40"})
                    } else if path == "/info" {
                        json!({"ID":"fake"})
                    } else if first.starts_with("DELETE") || first.starts_with("POST") {
                        if path == "/build/prune" {
                            cache_response.lock_ok().clone()
                        } else if path.starts_with("/images/") {
                            json!([])
                        } else {
                            Value::Null
                        }
                    } else if path == "/system/df" {
                        df.lock_ok().clone()
                    } else if path.starts_with("/containers/") {
                        if revoke.load(Ordering::SeqCst) {
                            dispatch.store(false, Ordering::SeqCst);
                        }
                        details
                            .lock_ok()
                            .get("abc123")
                            .cloned()
                            .unwrap_or(Value::Null)
                    } else if path.starts_with("/volumes/") {
                        df.lock_ok()["Volumes"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|v| v["Name"] == path.trim_start_matches("/volumes/"))
                            .cloned()
                            .unwrap_or(Value::Null)
                    } else {
                        Value::Null
                    };
                    let status = if let Some(code) = error_status {
                        format!("{code} Fixture Error")
                    } else if body.is_null() {
                        "204 No Content".into()
                    } else {
                        "200 OK".into()
                    };
                    let body = if body.is_null() {
                        String::new()
                    } else {
                        body.to_string()
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });
        Fake {
            _dir: dir,
            engine: engine(path.to_string_lossy().into()),
            df,
            details,
            requests,
            revoke_during_inspect,
            dispatch_allowed,
            drop_mutation_response,
            mutation_error,
            cache_response,
            task,
        }
    }
    fn engine(socket: String) -> DockerEngine {
        DockerEngine {
            id: "fake".into(),
            daemon_id: None,
            runtime: "test".into(),
            context: None,
            socket,
            reachable: true,
            preferred: false,
            error: None,
            api_version: None,
            capacity_bytes: None,
            capacity_observed_ms: None,
        }
    }
    fn selection(r: &DockerStorageResource, action: DockerStorageAction) -> DockerStorageSelection {
        DockerStorageSelection {
            kind: r.kind,
            id: r.id.clone(),
            action,
        }
    }
    #[tokio::test]
    async fn fake_daemon_executes_only_reviewed_nonforce_actions() {
        let f = fake().await;
        let adapter = DockerStorageAdapter::connect(f.engine.clone())
            .await
            .unwrap();
        let s = adapter.scan(1).await.unwrap();
        for id in [
            "abc123",
            "unused",
            "cache.*one",
            &format!("sha256:{}", "b".repeat(64)),
        ] {
            let r = row(&s, id);
            let result = adapter
                .execute(&selection(&r, DockerStorageAction::Delete), &r, || true)
                .await;
            assert_eq!(
                result.status,
                DockerStorageItemStatus::Succeeded,
                "{id}: {}",
                result.message
            );
        }
        let requests = f.requests.lock_ok();
        assert!(
            requests
                .iter()
                .any(|r| r.contains("v=false&force=false&link=false"))
        );
        assert!(
            requests
                .iter()
                .any(|r| r.contains("force=false&noprune=true"))
        );
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.starts_with("POST") && r.contains("build/prune"))
                .count(),
            1
        );
    }
    #[tokio::test]
    async fn revalidation_blocks_volume_replacement_attachment_and_generation_change() {
        let f = fake().await;
        let adapter = DockerStorageAdapter::connect(f.engine.clone())
            .await
            .unwrap();
        let s = adapter.scan(1).await.unwrap();
        let r = row(&s, "unused");
        let sel = selection(&r, DockerStorageAction::Delete);
        f.df.lock_ok()["Volumes"][1]["CreatedAt"] = json!("2026-01-03T00:00:00Z");
        assert_eq!(
            adapter.execute(&sel, &r, || true).await.status,
            DockerStorageItemStatus::Skipped
        );
        f.df.lock_ok()["Volumes"][1]["CreatedAt"] = json!("2026-01-01T00:00:00Z");
        f.details.lock_ok().get_mut("abc123").unwrap()["Mounts"] =
            json!([{"Type":"volume","Name":"unused"}]);
        assert_eq!(
            adapter.execute(&sel, &r, || true).await.status,
            DockerStorageItemStatus::Skipped
        );
        f.details.lock_ok().get_mut("abc123").unwrap()["Mounts"] = json!([]);
        assert_eq!(
            adapter.execute(&sel, &r, || false).await.status,
            DockerStorageItemStatus::Skipped
        );
        assert!(
            !f.requests
                .lock_ok()
                .iter()
                .any(|r| r.starts_with("DELETE") || r.starts_with("POST"))
        );
    }
    #[tokio::test]
    async fn authorization_revoked_during_async_revalidation_prevents_dispatch() {
        let f = fake().await;
        let adapter = DockerStorageAdapter::connect(f.engine.clone())
            .await
            .unwrap();
        let snapshot = adapter.scan(1).await.unwrap();
        let reviewed = row(&snapshot, "unused");
        assert!(f.dispatch_allowed.load(Ordering::SeqCst));
        f.revoke_during_inspect.store(true, Ordering::SeqCst);
        let result = adapter
            .execute(
                &selection(&reviewed, DockerStorageAction::Delete),
                &reviewed,
                || f.dispatch_allowed.load(Ordering::SeqCst),
            )
            .await;
        assert!(!f.dispatch_allowed.load(Ordering::SeqCst));
        assert_eq!(result.status, DockerStorageItemStatus::Skipped);
        assert!(
            !f.requests
                .lock_ok()
                .iter()
                .any(|r| r.starts_with("DELETE") || r.starts_with("POST"))
        );
    }
    #[tokio::test]
    async fn cache_success_without_selected_deletion_is_skipped() {
        let f = fake().await;
        let adapter = DockerStorageAdapter::connect(f.engine.clone())
            .await
            .unwrap();
        let snapshot = adapter.scan(1).await.unwrap();
        let reviewed = row(&snapshot, "cache.*one");
        for response in [
            json!({"SpaceReclaimed":0}),
            json!({"CachesDeleted":[],"SpaceReclaimed":0}),
        ] {
            *f.cache_response.lock_ok() = response;
            let result = adapter
                .execute(
                    &selection(&reviewed, DockerStorageAction::Delete),
                    &reviewed,
                    || true,
                )
                .await;
            assert_eq!(result.status, DockerStorageItemStatus::Skipped);
            assert_eq!(result.reclaimed_bytes, None);
            assert!(result.message.contains("did not remove"));
        }
        assert_eq!(
            f.requests
                .lock_ok()
                .iter()
                .filter(|r| r.starts_with("POST"))
                .count(),
            2
        );
    }
    #[tokio::test]
    async fn disconnected_mutation_is_unknown_and_never_retried() {
        let f = fake().await;
        let adapter = DockerStorageAdapter::connect(f.engine.clone())
            .await
            .unwrap();
        let snapshot = adapter.scan(1).await.unwrap();
        let reviewed = row(&snapshot, "unused");
        f.drop_mutation_response.store(true, Ordering::SeqCst);
        let result = adapter
            .execute(
                &selection(&reviewed, DockerStorageAction::Delete),
                &reviewed,
                || true,
            )
            .await;
        assert_eq!(result.status, DockerStorageItemStatus::OutcomeUnknown);
        assert_eq!(
            f.requests
                .lock_ok()
                .iter()
                .filter(|r| r.starts_with("DELETE"))
                .count(),
            1
        );
    }
    #[tokio::test]
    async fn changed_container_and_autoremove_never_dispatch_stop() {
        let f = fake().await;
        f.details.lock_ok().get_mut("abc123").unwrap()["State"]["Status"] = json!("running");
        let adapter = DockerStorageAdapter::connect(f.engine.clone())
            .await
            .unwrap();
        let s = adapter.scan(1).await.unwrap();
        let r = row(&s, "abc123");
        assert!(r.can_stop);
        f.details.lock_ok().get_mut("abc123").unwrap()["HostConfig"]["AutoRemove"] = json!(true);
        assert_eq!(
            adapter
                .execute(&selection(&r, DockerStorageAction::Stop), &r, || true)
                .await
                .status,
            DockerStorageItemStatus::Skipped
        );
        assert!(!f.requests.lock_ok().iter().any(|r| r.starts_with("POST")));
    }
    #[tokio::test]
    async fn container_actions_inspect_only_target_and_use_exact_start_stop_endpoints() {
        let f = fake().await;
        let adapter = DockerStorageAdapter::connect(f.engine.clone())
            .await
            .unwrap();
        for (state, action) in [
            ("exited", DockerContainerAction::Start),
            ("created", DockerContainerAction::Start),
            ("running", DockerContainerAction::Stop),
            ("restarting", DockerContainerAction::Stop),
            ("paused", DockerContainerAction::Stop),
        ] {
            f.details.lock_ok().get_mut("abc123").unwrap()["State"]["Status"] = json!(state);
            let result = adapter.container_action("abc123", action, || true).await;
            assert_eq!(
                result.status,
                DockerStorageItemStatus::Succeeded,
                "{state}: {result:?}"
            );
        }
        let requests = f.requests.lock_ok();
        assert!(!requests.iter().any(|r| r.contains("/system/df")));
        let mutations: Vec<_> = requests.iter().filter(|r| r.starts_with("POST")).collect();
        assert_eq!(mutations.len(), 5);
        assert_eq!(
            mutations
                .iter()
                .filter(|r| r.contains("/start HTTP"))
                .count(),
            2
        );
        assert_eq!(
            mutations
                .iter()
                .filter(|r| r.contains("/stop?t=30 HTTP"))
                .count(),
            3
        );
    }
    #[tokio::test]
    async fn container_actions_reject_ineligible_missing_and_changed_evidence_before_dispatch() {
        let f = fake().await;
        let mut adapter = DockerStorageAdapter::connect(f.engine.clone())
            .await
            .unwrap();
        for (state, auto_remove, action) in [
            ("running", json!(false), DockerContainerAction::Start),
            ("paused", json!(false), DockerContainerAction::Start),
            ("exited", json!(false), DockerContainerAction::Stop),
            ("running", json!(true), DockerContainerAction::Stop),
            ("running", Value::Null, DockerContainerAction::Stop),
        ] {
            {
                let mut details = f.details.lock_ok();
                let row = details.get_mut("abc123").unwrap();
                row["State"]["Status"] = json!(state);
                row["HostConfig"]["AutoRemove"] = auto_remove;
            }
            assert_eq!(
                adapter
                    .container_action("abc123", action, || true)
                    .await
                    .status,
                DockerStorageItemStatus::Skipped
            );
        }
        f.details.lock_ok().get_mut("abc123").unwrap()["State"]["Status"] = json!("exited");
        f.revoke_during_inspect.store(true, Ordering::SeqCst);
        assert_eq!(
            adapter
                .container_action("abc123", DockerContainerAction::Start, || f
                    .dispatch_allowed
                    .load(Ordering::SeqCst))
                .await
                .status,
            DockerStorageItemStatus::Skipped
        );
        adapter.engine.id = "replacement".into();
        assert_eq!(
            adapter
                .container_action("abc123", DockerContainerAction::Start, || true)
                .await
                .status,
            DockerStorageItemStatus::Skipped
        );
        adapter.engine.id = "fake".into();
        f.details.lock_ok().clear();
        let missing = adapter
            .container_action("abc123", DockerContainerAction::Start, || true)
            .await;
        assert_eq!(missing.status, DockerStorageItemStatus::Skipped);
        assert!(missing.message.contains("no longer exists"));
        assert!(!f.requests.lock_ok().iter().any(|r| r.starts_with("POST")));
    }
    #[tokio::test]
    async fn container_action_failures_and_unknown_outcomes_are_never_retried() {
        let f = fake().await;
        let adapter = DockerStorageAdapter::connect(f.engine.clone())
            .await
            .unwrap();
        *f.mutation_error.lock_ok() = Some(500);
        let result = adapter
            .container_action("abc123", DockerContainerAction::Start, || true)
            .await;
        assert_eq!(result.status, DockerStorageItemStatus::Failed);
        assert!(result.message.contains("Port 8080 is already in use"));
        *f.mutation_error.lock_ok() = None;
        f.drop_mutation_response.store(true, Ordering::SeqCst);
        let result = adapter
            .container_action("abc123", DockerContainerAction::Start, || true)
            .await;
        assert_eq!(result.status, DockerStorageItemStatus::OutcomeUnknown);
        assert_eq!(
            f.requests
                .lock_ok()
                .iter()
                .filter(|r| r.starts_with("POST"))
                .count(),
            2
        );
    }
    /// Explicit opt-in only; no resource is selected by discovery. Names must identify
    /// parent-created disposable fixtures. Cache uses its exact supplied immutable ID.
    #[tokio::test]
    #[ignore = "Requires explicitly supplied disposable fixture IDs and local Docker socket"]
    async fn installed_storage_fixture_action() {
        let socket =
            std::env::var("KELVO_DOCKER_TEST_SOCKET").expect("explicit test socket required");
        let adapter = DockerStorageAdapter::connect(engine(socket)).await.unwrap();
        let s = adapter.scan(1).await.unwrap();
        assert!(s.complete);
        println!(
            "storage scan: {} resources, {:?}",
            s.resources.len(),
            s.categories
        );
        for (env, kind, action) in [
            (
                "KELVO_DOCKER_TEST_CACHE_ID",
                DockerStorageKind::BuildCache,
                DockerStorageAction::Delete,
            ),
            (
                "KELVO_DOCKER_TEST_IMAGE_ID",
                DockerStorageKind::Image,
                DockerStorageAction::Delete,
            ),
            (
                "KELVO_DOCKER_TEST_VOLUME",
                DockerStorageKind::Volume,
                DockerStorageAction::Delete,
            ),
            (
                "KELVO_DOCKER_TEST_STOP_ID",
                DockerStorageKind::Container,
                DockerStorageAction::Stop,
            ),
        ] {
            if let Ok(id) = std::env::var(env) {
                let r = row(&s, &id);
                assert_eq!(r.kind, kind);
                if kind != DockerStorageKind::BuildCache {
                    assert!(r.name.starts_with("kelvo-storage-test-"));
                }
                if kind == DockerStorageKind::Image {
                    let df = adapter
                        .scan_client
                        .df(None::<DataUsageOptions>)
                        .await
                        .unwrap();
                    let images = df.images.unwrap();
                    let image = images.iter().find(|image| image.id == id).unwrap();
                    assert!(!image.repo_tags.is_empty());
                    assert!(
                        image
                            .repo_tags
                            .iter()
                            .all(|tag| tag.starts_with("kelvo-storage-test-"))
                    );
                }
                let result = adapter.execute(&selection(&r, action), &r, || true).await;
                println!("fixture {id}: {:?}", result);
                assert_eq!(result.status, DockerStorageItemStatus::Succeeded);
                if kind == DockerStorageKind::BuildCache {
                    let after = adapter.scan(1).await.unwrap();
                    assert!(
                        !after.resources.iter().any(|r| r.kind == kind && r.id == id),
                        "selected cache must actually disappear"
                    );
                    for before in s.resources.iter().filter(|r| r.kind == kind && r.id != id) {
                        assert!(
                            after
                                .resources
                                .iter()
                                .any(|r| r.kind == kind && r.id == before.id),
                            "unselected cache {} disappeared",
                            before.id
                        );
                    }
                }
            }
        }
        if let Ok(id) = std::env::var("KELVO_DOCKER_TEST_START_ID") {
            let detail = adapter
                .scan_client
                .inspect_container(&id, None::<InspectContainerOptions>)
                .await
                .unwrap();
            assert!(detail.name.as_deref().is_some_and(|name| {
                name.trim_start_matches('/')
                    .starts_with("kelvo-storage-test-")
            }));
            for action in [DockerContainerAction::Start, DockerContainerAction::Stop] {
                let result = adapter.container_action(&id, action, || true).await;
                println!("container fixture {id}: {result:?}");
                assert_eq!(result.status, DockerStorageItemStatus::Succeeded);
            }
        }
        if let Ok(id) = std::env::var("KELVO_DOCKER_TEST_AUTOREMOVE_ID") {
            let r = row(&s, &id);
            assert_eq!(r.auto_remove, Some(true));
            assert!(!r.can_stop);
        }
    }
}

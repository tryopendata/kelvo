//! Read-only local Docker adapter. No CLI, environment inspection or command proxy.
//! The transport rejects every method/path outside the monitoring allowlist and
//! limits bytes before Bollard decodes them. Long streams rotate at 8 MiB; callers
//! must publish the resulting interruption as uncertain coverage.
use bollard::query_parameters::{
    EventsOptions, InspectContainerOptions, ListContainersOptions, LogsOptions, StatsOptions,
};
use bollard::{API_DEFAULT_VERSION, BollardRequest, Docker};
use futures_util::{Stream, StreamExt};
use kelvo_schema::docker::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::UnixStream;

#[derive(Debug, thiserror::Error)]
pub enum DockerError {
    #[error("Docker request timed out")]
    Timeout,
    #[error("Permission denied accessing the local Docker socket")]
    PermissionDenied,
    #[error("Local Docker socket is unavailable")]
    Unavailable,
    #[error("Docker API version is incompatible (requires 1.40 or newer)")]
    Incompatible,
    #[error("Docker returned an invalid or oversized monitoring response")]
    Invalid,
    #[error("Docker request failed (socket, permissions, daemon or response limit)")]
    Api(#[from] bollard::errors::Error),
    #[error("Container no longer exists")]
    NotFound,
    #[error("Docker request pool is busy; retry shortly")]
    Busy,
    #[error("Only an absolute local Unix socket is supported")]
    Endpoint,
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
pub fn bounded(s: &str, max: usize) -> String {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}
fn allowed(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or("");
    let path = if path.starts_with("/v1.") {
        path.get(1..)
            .and_then(|p| p.find('/').map(|i| &p[i..]))
            .unwrap_or("")
    } else {
        path
    };
    matches!(path, "/version" | "/info" | "/containers/json" | "/events") || {
        let p: Vec<_> = path.split('/').collect();
        p.len() == 4
            && p.get(1) == Some(&"containers")
            && p.get(2).is_some_and(|id| valid_id(id))
            && matches!(p.get(3), Some(&"json" | &"stats" | &"logs"))
    }
}
/// Bounds the entire HTTP response, including decoder lookahead and partial lines.
struct BoundedIo {
    stream: UnixStream,
    left: usize,
}
impl AsyncRead for BoundedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.left == 0 {
            return Poll::Ready(Err(std::io::Error::other(
                "Docker response byte limit reached",
            )));
        }
        let max = buf.remaining().min(self.left);
        let mut limited = ReadBuf::new(buf.initialize_unfilled_to(max));
        let result = Pin::new(&mut self.stream).poll_read(cx, &mut limited);
        let n = limited.filled().len();
        buf.advance(n);
        self.left -= n;
        result
    }
}
impl AsyncWrite for BoundedIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
#[derive(Clone)]
pub struct DockerAdapter {
    client: Docker,
    pub engine: DockerEngine,
    permits: std::sync::Arc<tokio::sync::Semaphore>,
}
impl DockerAdapter {
    pub async fn connect(mut engine: DockerEngine) -> Result<Self, DockerError> {
        if !Path::new(&engine.socket).is_absolute() || engine.socket.contains("://") {
            return Err(DockerError::Endpoint);
        }
        let socket = engine.socket.clone();
        let client = Docker::connect_with_custom_transport(
            move |mut req: BollardRequest| {
                let socket = socket.clone();
                async move {
                    if req.method() != hyper::Method::GET || !allowed(req.uri().path()) {
                        return Err(bollard::errors::Error::IOError {
                            err: std::io::Error::other("Docker endpoint denied"),
                        });
                    }
                    let streaming = req.uri().path().ends_with("/events")
                        || req.uri().path().ends_with("/logs");
                    let socket = UnixStream::connect(socket)
                        .await
                        .map_err(|err| bollard::errors::Error::IOError { err })?;
                    let io = hyper_util::rt::TokioIo::new(BoundedIo {
                        stream: socket,
                        left: if streaming {
                            8 * 1024 * 1024
                        } else {
                            16 * 1024 * 1024
                        },
                    });
                    let (mut sender, connection) =
                        hyper::client::conn::http1::handshake(io).await?;
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
            2,
            API_DEFAULT_VERSION,
        )?;
        let client = finite(client.negotiate_version()).await?;
        let v = finite(client.version()).await?;
        let api = v.api_version.unwrap_or_default();
        let major = api
            .split('.')
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let min_minor = v
            .min_api_version
            .as_deref()
            .and_then(|s| s.split('.').nth(1))
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let minor = api
            .split('.')
            .nth(1)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        if major != 1 || minor < 40 || min_minor > 49 {
            return Err(DockerError::Incompatible);
        }
        let info = finite(client.info()).await?;
        let daemon = info
            .id
            .filter(|s| !s.is_empty())
            .ok_or(DockerError::Invalid)?;
        engine.id = bounded(&daemon, 256);
        engine.daemon_id = Some(engine.id.clone());
        engine.api_version = Some(format!("1.{}", minor.min(49)));
        engine.reachable = true;
        engine.error = None;
        engine.capacity_bytes = info.mem_total.filter(|m| *m > 0).map(|m| m as f64);
        engine.capacity_observed_ms = Some(now_ms());
        Ok(Self {
            client,
            engine,
            permits: std::sync::Arc::new(tokio::sync::Semaphore::new(8)),
        })
    }
    pub async fn refresh_capacity(&mut self) -> Result<(), DockerError> {
        let _permit = self.permits.try_acquire().map_err(|_| DockerError::Busy)?;
        let info = finite(self.client.info()).await?;
        self.engine.capacity_bytes = info.mem_total.filter(|v| *v > 0).map(|v| v as f64);
        self.engine.capacity_observed_ms = Some(now_ms());
        Ok(())
    }
    pub async fn inventory(&self) -> Result<(Vec<DockerContainer>, bool), DockerError> {
        let _permit = self.permits.try_acquire().map_err(|_| DockerError::Busy)?;
        let list = finite(self.client.list_containers(Some(ListContainersOptions {
            all: true,
            limit: Some(10001),
            ..Default::default()
        })))
        .await?;
        let mut complete = list.len() <= 10_000;
        let now = now_ms();
        let mut containers: Vec<DockerContainer> = list
            .into_iter()
            .take(10_000)
            .filter_map(|c| {
                let id = c.id.filter(|id| valid_id(id))?;
                let labels = c.labels.unwrap_or_default();
                Some(DockerContainer {
                    engine_id: self.engine.id.clone(),
                    id,
                    name: bounded(
                        c.names
                            .as_ref()
                            .and_then(|n| n.first())
                            .map(|s| s.trim_start_matches('/'))
                            .unwrap_or("Unnamed"),
                        512,
                    ),
                    image: bounded(c.image.as_deref().unwrap_or("Unknown"), 1024),
                    project: labels
                        .get("com.docker.compose.project")
                        .map(|s| bounded(s, 256)),
                    service: labels
                        .get("com.docker.compose.service")
                        .map(|s| bounded(s, 256)),
                    lifecycle: lifecycle(
                        c.state
                            .as_ref()
                            .map(|v| v.to_string())
                            .as_deref()
                            .unwrap_or("unknown"),
                    ),
                    health: DockerHealth::Unknown,
                    restart_count: None,
                    metadata_available: false,
                    metadata_observed_ms: None,
                    observed_restarts: 0,
                    repeated_restarts: false,
                    observed_restart_window_ms: 300000,
                    oom_killed: false,
                    explicit_limit_bytes: None,
                    cpu_quota: None,
                    restart_policy: None,
                    ports: Vec::new(),
                    mounts: Vec::new(),
                    created: c.created.map(|s| s.to_string()),
                    last_seen_ms: now,
                    observed_ms: None,
                    interval_ms: None,
                    monitored: false,
                    metrics: DockerMetrics::default(),
                })
            })
            .collect();
        let mut bytes = 0usize;
        let mut retained = 0;
        for row in &containers {
            let size = serde_json::to_vec(row)
                .map_err(|_| DockerError::Invalid)?
                .len();
            if bytes.saturating_add(size) > 7 * 1024 * 1024 {
                complete = false;
                break;
            }
            bytes += size;
            retained += 1;
        }
        containers.truncate(retained);
        Ok((containers, complete))
    }
    pub async fn inspect(&self, mut row: DockerContainer) -> Result<DockerContainer, DockerError> {
        if !valid_id(&row.id) {
            return Err(DockerError::Invalid);
        }
        let _permit = self.permits.try_acquire().map_err(|_| DockerError::Busy)?;
        let c = finite(
            self.client
                .inspect_container(&row.id, None::<InspectContainerOptions>),
        )
        .await?;
        // Consume allowlisted fields only; Env, Cmd, health output and commands die here.
        let has_healthcheck = c.config.as_ref().is_some_and(|c| c.healthcheck.is_some());
        row.metadata_available = true;
        row.metadata_observed_ms = Some(now_ms());
        if let Some(state) = c.state {
            row.lifecycle = lifecycle(&state.status.map(|s| s.to_string()).unwrap_or_default());
            row.oom_killed = state.oom_killed.unwrap_or(false);
            row.health = match state
                .health
                .and_then(|h| h.status)
                .map(|s| s.to_string())
                .as_deref()
            {
                None if !has_healthcheck => DockerHealth::NoCheck,
                None => DockerHealth::Unknown,
                Some("healthy") => DockerHealth::Healthy,
                Some("unhealthy") => DockerHealth::Unhealthy,
                Some("starting") => DockerHealth::Starting,
                _ => DockerHealth::Unknown,
            };
        }
        row.restart_count = c.restart_count.map(|n| n.clamp(0, u32::MAX as i64) as u32);
        if let Some(h) = c.host_config {
            row.explicit_limit_bytes = h.memory.filter(|n| *n > 0).map(|n| n as f64);
            row.cpu_quota = h
                .cpu_quota
                .filter(|q| *q > 0)
                .zip(h.cpu_period.filter(|p| *p > 0))
                .map(|(q, p)| q as f64 / p as f64);
            row.restart_policy = h.restart_policy.and_then(|p| p.name).map(|s| s.to_string());
        }
        row.created = c.created.map(|t| bounded(&t.to_string(), 64));
        row.mounts = c
            .mounts
            .unwrap_or_default()
            .into_iter()
            .take(16)
            .filter_map(|m| m.destination)
            .map(|s| bounded(&s, 256))
            .collect();
        row.ports = c
            .network_settings
            .and_then(|n| n.ports)
            .unwrap_or_default()
            .into_iter()
            .take(64)
            .map(|(p, _)| bounded(&p, 64))
            .collect();
        Ok(row)
    }
    pub async fn stats(&self, id: &str) -> Result<DockerCounters, DockerError> {
        if !valid_id(id) {
            return Err(DockerError::Invalid);
        }
        let _permit = self.permits.try_acquire().map_err(|_| DockerError::Busy)?;
        let mut stream = self.client.stats(
            id,
            Some(StatsOptions {
                stream: false,
                one_shot: true,
            }),
        );
        tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .map_err(|_| DockerError::Timeout)?
            .ok_or(DockerError::Invalid)?
            .map(DockerCounters::from)
            .map_err(Into::into)
    }
    pub fn events(&self) -> impl Stream<Item = Result<DockerEvent, DockerError>> + Send + 'static {
        self.events_since(None)
    }
    pub fn events_since(
        &self,
        since: Option<String>,
    ) -> impl Stream<Item = Result<DockerEvent, DockerError>> + Send + 'static {
        let client = self.client.clone();
        let engine = self.engine.id.clone();
        let stream = client.events(Some(EventsOptions {
            since,
            filters: Some(std::collections::HashMap::from([(
                "type".into(),
                vec!["container".into()],
            )])),
            ..Default::default()
        }));
        stream.map(move |result| {
            let event = result?;
            let actor = event.actor.unwrap_or_default();
            let attrs = actor.attributes.unwrap_or_default();
            let action = bounded(event.action.as_deref().unwrap_or("unknown"), 128);
            let ns = event
                .time_nano
                .filter(|n| *n > 0)
                .ok_or(DockerError::Invalid)?;
            let id = actor.id.filter(|s| valid_id(s));
            let name = attrs.get("name").map(|s| bounded(s, 512));
            let exit_code = attrs.get("exitCode").map(|s| bounded(s, 16));
            let attributes: BTreeMap<String, String> =
                ["name", "image", "exitCode", "signal", "health_status"]
                    .into_iter()
                    .filter_map(|key| {
                        attrs
                            .get(key)
                            .map(|value| (key.to_string(), bounded(value, 1024)))
                    })
                    .collect();
            let mut bytes = Vec::new();
            ciborium::into_writer(&(&engine, &id, ns, &action, &attributes), &mut bytes)
                .map_err(|_| DockerError::Invalid)?;
            use sha2::Digest;
            let key = format!("{:x}", sha2::Sha256::digest(bytes));
            Ok(DockerEvent {
                id: key,
                engine_id: engine.clone(),
                container_id: id,
                time_ns: ns.to_string(),
                time_ms: ns / 1_000_000,
                action,
                name,
                exit_code,
                attributes,
            })
        })
    }
    pub fn logs(
        &self,
        id: &str,
        follow: bool,
    ) -> impl Stream<Item = Result<Vec<u8>, DockerError>> + Send + 'static {
        self.logs_window(id, follow, 0)
    }
    pub fn logs_window(
        &self,
        id: &str,
        follow: bool,
        boundary: i32,
    ) -> impl Stream<Item = Result<Vec<u8>, DockerError>> + Send + 'static {
        // IDs are validated by the worker before acquiring a stream slot; transport independently enforces it.
        let stream = self.client.logs(
            id,
            Some(LogsOptions {
                follow,
                stdout: true,
                stderr: true,
                tail: if follow && boundary > 0 { "all" } else { "200" }.into(),
                since: if follow { boundary } else { 0 },
                until: 0,
                timestamps: true,
            }),
        );
        stream.map(|r| r.map(|b| b.into_bytes().to_vec()).map_err(Into::into))
    }
}
async fn finite<T>(
    f: impl std::future::Future<Output = Result<T, bollard::errors::Error>>,
) -> Result<T, DockerError> {
    tokio::time::timeout(Duration::from_secs(2), f)
        .await
        .map_err(|_| DockerError::Timeout)?
        .map_err(classify)
}
fn classify(e: bollard::errors::Error) -> DockerError {
    match &e {
        bollard::errors::Error::IOError { err }
            if err.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            DockerError::PermissionDenied
        }
        bollard::errors::Error::DockerResponseServerError {
            status_code: 403, ..
        } => DockerError::PermissionDenied,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        } => DockerError::NotFound,
        _ => DockerError::Api(e),
    }
}
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
fn lifecycle(s: &str) -> DockerLifecycle {
    match s {
        "created" => DockerLifecycle::Created,
        "running" => DockerLifecycle::Running,
        "paused" => DockerLifecycle::Paused,
        "restarting" => DockerLifecycle::Restarting,
        "exited" => DockerLifecycle::Exited,
        "removing" => DockerLifecycle::Removing,
        "dead" => DockerLifecycle::Dead,
        _ => DockerLifecycle::Unknown,
    }
}

/// Discovery reads only local context endpoint/name fields. No DOCKER_HOST, SSH or TCP.
pub fn candidates(home: &Path) -> Vec<DockerEngine> {
    let mut paths: Vec<(PathBuf, Option<String>, bool)> = vec![
        (home.join(".docker/run/docker.sock"), None, false),
        (home.join(".orbstack/run/docker.sock"), None, false),
        (PathBuf::from("/var/run/docker.sock"), None, false),
    ];
    let active = read_json(&home.join(".docker/config.json")).and_then(|v| {
        v.get("currentContext")
            .and_then(|v| v.as_str())
            .map(String::from)
    });
    if let Ok(entries) = std::fs::read_dir(home.join(".docker/contexts/meta")) {
        for entry in entries.flatten().take(256) {
            if let Some(v) = read_json(&entry.path().join("meta.json"))
                && let Some(endpoint) = v
                    .pointer("/Endpoints/docker/Host")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.strip_prefix("unix://"))
            {
                let name = v
                    .get("Name")
                    .and_then(|v| v.as_str())
                    .map(|s| bounded(s, 256));
                paths.push((PathBuf::from(endpoint), name.clone(), name == active));
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(home.join(".colima")) {
        for entry in entries.flatten().take(256) {
            paths.push((
                entry.path().join("docker.sock"),
                entry.file_name().to_str().map(String::from),
                false,
            ));
        }
    }
    let mut found: BTreeMap<PathBuf, DockerEngine> = BTreeMap::new();
    for (path, context, preferred) in paths {
        if let Ok(canonical) = std::fs::canonicalize(&path) {
            if !canonical.is_absolute() {
                continue;
            }
            let socket = canonical.to_string_lossy().into_owned();
            let runtime = if socket.contains("orbstack") {
                "OrbStack"
            } else if socket.contains("colima") {
                "Colima"
            } else if socket.contains(".docker/run") {
                "Docker Desktop"
            } else {
                "Local Docker (unverified)"
            };
            let e = found.entry(canonical).or_insert_with(|| DockerEngine {
                id: socket.clone(),
                daemon_id: None,
                runtime: runtime.into(),
                context: None,
                socket,
                reachable: false,
                preferred: false,
                error: None,
                api_version: None,
                capacity_bytes: None,
                capacity_observed_ms: None,
            });
            if preferred || e.context.is_none() {
                e.context = context;
            }
            e.preferred |= preferred;
        }
    }
    found.into_values().collect()
}
fn read_json(path: &Path) -> Option<serde_json::Value> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > 262144 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(262145).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 262144 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_allowlist_is_closed() {
        for path in [
            "/version",
            "/v1.49/info",
            "/containers/abc123/stats",
            "/v1.40/events",
        ] {
            assert!(allowed(path));
        }
        for path in [
            "/containers/abc/exec",
            "/containers/create",
            "/images/json",
            "/containers/../json",
            "/containers/a%2fb/logs",
            "/v1.49/containers/abc/restart",
        ] {
            assert!(!allowed(path));
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DockerCounters {
    pub cpu: Option<u64>,
    pub system: Option<u64>,
    pub online_cpus: Option<u32>,
    pub raw_memory: Option<u64>,
    pub inactive_file: Option<u64>,
    pub effective_limit: Option<u64>,
    pub rx: Option<u64>,
    pub tx: Option<u64>,
    pub read: Option<u64>,
    pub write: Option<u64>,
}
impl From<bollard::models::ContainerStatsResponse> for DockerCounters {
    fn from(stats: bollard::models::ContainerStatsResponse) -> Self {
        let cpu = stats.cpu_stats.unwrap_or_default();
        let usage = cpu.cpu_usage.unwrap_or_default();
        let memory = stats.memory_stats.unwrap_or_default();
        let fields = memory.stats.unwrap_or_default();
        let (rx, tx) = stats
            .networks
            .map(|n| {
                (
                    n.values()
                        .try_fold(0u64, |sum, v| sum.checked_add(v.rx_bytes?)),
                    n.values()
                        .try_fold(0u64, |sum, v| sum.checked_add(v.tx_bytes?)),
                )
            })
            .unwrap_or((None, None));
        let io = stats.blkio_stats.and_then(|s| s.io_service_bytes_recursive);
        let block = |op: &str| {
            io.as_ref().and_then(|entries| {
                entries
                    .iter()
                    .filter(|e| e.op.as_deref().is_some_and(|v| v.eq_ignore_ascii_case(op)))
                    .try_fold(0u64, |sum, e| sum.checked_add(e.value?))
            })
        };
        Self {
            cpu: usage.total_usage,
            system: cpu.system_cpu_usage,
            online_cpus: cpu.online_cpus.filter(|n| *n > 0).or_else(|| {
                usage
                    .percpu_usage
                    .map(|v| v.len() as u32)
                    .filter(|n| *n > 0)
            }),
            raw_memory: memory.usage,
            inactive_file: fields
                .get("total_inactive_file")
                .or_else(|| fields.get("inactive_file"))
                .copied(),
            effective_limit: memory.limit,
            rx,
            tx,
            read: block("read"),
            write: block("write"),
        }
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn raw_response_cap_applies_before_decoder_and_cancellation_closes_socket() {
        let (left, mut right) = UnixStream::pair().unwrap();
        let writer = tokio::spawn(async move {
            right.write_all(&[b'x'; 4096]).await.unwrap();
            let mut byte = [0];
            right.read(&mut byte).await
        });
        let mut bounded = BoundedIo {
            stream: left,
            left: 1024,
        };
        let mut output = Vec::new();
        assert!(bounded.read_to_end(&mut output).await.is_err());
        assert_eq!(output.len(), 1024);
        drop(bounded);
        let closed = tokio::time::timeout(Duration::from_secs(1), writer)
            .await
            .unwrap()
            .unwrap();
        assert!(closed.is_err() || matches!(closed, Ok(0)));
    }
    async fn fake_daemon() -> (tempfile::TempDir, DockerEngine, tokio::task::JoinHandle<()>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut request = vec![];
                    let mut byte = [0];
                    while socket.read_exact(&mut byte).await.is_ok() {
                        request.push(byte[0]);
                        if request.ends_with(b"\r\n\r\n") {
                            break;
                        }
                        if request.len() > 16384 {
                            return;
                        }
                    }
                    let req = String::from_utf8_lossy(&request);
                    if req.contains("/events") {
                        let _=socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n").await;
                        tokio::time::sleep(Duration::from_millis(2300)).await;
                        let body=b"{\"Type\":\"container\",\"Action\":\"start\",\"Actor\":{\"ID\":\"abc123\"},\"timeNano\":1770000000000000000}\n";
                        let frame =
                            format!("{:x}\r\n{}\r\n", body.len(), String::from_utf8_lossy(body));
                        let _ = socket.write_all(frame.as_bytes()).await;
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    } else {
                        let body = if req.contains("/version") {
                            r#"{"ApiVersion":"1.49","MinAPIVersion":"1.40"}"#
                        } else if req.contains("/info") {
                            r#"{"ID":"fake-daemon","MemTotal":1024}"#
                        } else {
                            "[]"
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                    }
                });
            }
        });
        let engine = DockerEngine {
            id: "fake".into(),
            daemon_id: None,
            runtime: "test".into(),
            context: None,
            socket: path.to_string_lossy().into(),
            reachable: false,
            preferred: false,
            error: None,
            api_version: None,
            capacity_bytes: None,
            capacity_observed_ms: None,
        };
        (dir, engine, task)
    }
    #[tokio::test]
    async fn quiet_event_stream_survives_finite_request_timeout() {
        let (_dir, engine, task) = fake_daemon().await;
        let adapter = DockerAdapter::connect(engine).await.unwrap();
        let mut events = Box::pin(adapter.events());
        let event = tokio::time::timeout(Duration::from_secs(4), events.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(event.action, "start");
        assert_eq!(event.time_ns, "1770000000000000000");
        task.abort();
    }
    #[tokio::test]
    #[ignore = "Read-only check against an explicitly installed local Docker engine"]
    async fn installed_engine_read_only_monitoring() {
        let home = std::env::var_os("HOME").unwrap();
        let engines = candidates(Path::new(&home));
        let candidate = engines
            .iter()
            .find(|e| e.preferred)
            .or_else(|| engines.first())
            .unwrap()
            .clone();
        let adapter = DockerAdapter::connect(candidate).await.unwrap();
        let (rows, complete) = adapter.inventory().await.unwrap();
        assert!(complete);
        let running = rows
            .iter()
            .filter(|c| c.lifecycle == DockerLifecycle::Running)
            .count();
        for row in rows
            .into_iter()
            .filter(|c| c.lifecycle == DockerLifecycle::Running)
            .take(2)
        {
            let inspected = adapter.inspect(row).await.unwrap();
            let raw = adapter.stats(&inspected.id).await.unwrap();
            assert!(raw.raw_memory.is_some());
            assert!(raw.inactive_file.is_some());
            assert!(raw.cpu.is_some());
        }
        eprintln!(
            "Read-only negotiation, inventory, inspect and stats passed: {running} running; API {}",
            adapter.engine.api_version.unwrap_or_default()
        );
    }
}

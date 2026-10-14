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
    let (serve_df, serve_details, serve_requests) = (df.clone(), details.clone(), requests.clone());
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
    let socket = std::env::var("KELVO_DOCKER_TEST_SOCKET").expect("explicit test socket required");
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

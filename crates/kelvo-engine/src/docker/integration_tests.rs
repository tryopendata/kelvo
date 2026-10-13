//! Real Unix HTTP fixtures exercise cancellation, pool fairness and independent streams.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
struct Fixture {
    _dir: tempfile::TempDir,
    socket: PathBuf,
    task: tokio::task::JoinHandle<()>,
    state: Arc<FixtureState>,
}
struct FixtureState {
    count: AtomicUsize,
    running_from: AtomicUsize,
    delay_ms: u64,
    active: AtomicUsize,
    maximum: AtomicUsize,
    stats: Mutex<BTreeSet<String>>,
    log_requests: AtomicUsize,
    log_follow_requests: AtomicUsize,
    event_requests: AtomicUsize,
    inventory_requests: AtomicUsize,
    probe_requests: AtomicUsize,
    event_eof: AtomicBool,
    inspect_status: AtomicUsize,
    inspect_requests: AtomicUsize,
    events: broadcast::Sender<Vec<u8>>,
    logs: broadcast::Sender<(String, Vec<u8>)>,
}
struct Active(Arc<FixtureState>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}
impl Fixture {
    async fn new(count: usize, delay_ms: u64) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("docker.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (events, _) = broadcast::channel(8);
        let (logs, _) = broadcast::channel(8);
        let state = Arc::new(FixtureState {
            count: AtomicUsize::new(count),
            running_from: AtomicUsize::new(0),
            delay_ms,
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
            stats: Mutex::new(BTreeSet::new()),
            log_requests: AtomicUsize::new(0),
            log_follow_requests: AtomicUsize::new(0),
            event_requests: AtomicUsize::new(0),
            inventory_requests: AtomicUsize::new(0),
            probe_requests: AtomicUsize::new(0),
            event_eof: AtomicBool::new(false),
            inspect_status: AtomicUsize::new(200),
            inspect_requests: AtomicUsize::new(0),
            events,
            logs,
        });
        let shared = state.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    break;
                };
                let state = shared.clone();
                tokio::spawn(async move {
                    serve(socket, state).await;
                });
            }
        });
        Self {
            _dir: dir,
            socket,
            task,
            state,
        }
    }
    fn settings(&self) -> DockerSettings {
        DockerSettings {
            enabled: true,
            selected_engine: Some(self.socket.to_string_lossy().into()),
            ..Default::default()
        }
    }
    fn handle(&self) -> DockerHandle {
        DockerHandle::new(
            HostId(uuid::Uuid::new_v4()),
            self.settings(),
            self._dir.path().into(),
        )
    }
    async fn adapter(&self) -> DockerAdapter {
        DockerAdapter::connect(DockerEngine {
            id: "fixture".into(),
            daemon_id: None,
            runtime: "fixture".into(),
            context: None,
            socket: self.socket.to_string_lossy().into(),
            reachable: false,
            preferred: false,
            error: None,
            api_version: None,
            capacity_bytes: None,
            capacity_observed_ms: None,
        })
        .await
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn serve(mut socket: UnixStream, state: Arc<FixtureState>) {
    let mut bytes = Vec::new();
    let mut one = [0];
    while socket.read_exact(&mut one).await.is_ok() {
        bytes.push(one[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            break;
        }
        if bytes.len() > 16384 {
            return;
        }
    }
    let request = String::from_utf8_lossy(&bytes);
    let path = request.split_whitespace().nth(1).unwrap_or("");
    let id = path
        .split("/containers/")
        .nth(1)
        .and_then(|s| s.split('/').next())
        .unwrap_or("")
        .to_owned();
    if path.contains("/events") {
        state.event_requests.fetch_add(1, Ordering::SeqCst);
        if state.event_eof.load(Ordering::SeqCst) {
            reply(&mut socket, b"").await;
            return;
        }
        let mut receiver = state.events.subscribe();
        let _=socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n").await;
        loop {
            tokio::select! {data=receiver.recv()=>{if let Ok(data)=data{if !chunk(&mut socket,&data).await{return;}}else{return;}},_=socket.read(&mut one)=>return}
        }
    }
    if path.contains("/logs") {
        state.log_requests.fetch_add(1, Ordering::SeqCst);
        if path.contains("follow=true") {
            let mut receiver = state.logs.subscribe();
            state.log_follow_requests.fetch_add(1, Ordering::SeqCst);
            let _=socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/vnd.docker.raw-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await;
            loop {
                tokio::select! {data=receiver.recv()=>{if let Ok((container,data))=data{if container==id&&!chunk(&mut socket,&data).await{return;}}else{return;}},_=socket.read(&mut one)=>return}
            }
        }
        let body = if id == "filtered" {
            let parameter = |name: &str| {
                path.split('?')
                    .nth(1)
                    .unwrap_or("")
                    .split('&')
                    .find_map(|pair| {
                        let (key, value) = pair.split_once('=')?;
                        (key == name).then(|| value.parse::<i64>().ok()).flatten()
                    })
                    .unwrap_or(0)
            };
            let since = parameter("since");
            let until = parameter("until");
            // A fixture record within this second, after its whole-second boundary.
            // Unlike the framing fixtures, this daemon honors both log filters.
            let timestamp_ms = now_ms().div_euclid(1000) * 1000 + 500;
            if timestamp_ms >= since * 1000 && (until == 0 || timestamp_ms <= until * 1000) {
                b"current-second burst\n".to_vec()
            } else {
                Vec::new()
            }
        } else if id == "tty" {
            b"plain tty\n".to_vec()
        } else if id == "overlap" {
            b"2026-10-11T12:00:00.000000000Z repeated\n2026-10-11T12:00:00.000000000Z repeated\n"
                .to_vec()
        } else if id == "mux" {
            multiplex(b"stdout\n", 1)
                .into_iter()
                .chain(multiplex(b"stderr\n", 2))
                .collect()
        } else {
            Vec::new()
        };
        reply(&mut socket, &body).await;
        return;
    }
    let active = state.active.fetch_add(1, Ordering::SeqCst) + 1;
    state.maximum.fetch_max(active, Ordering::SeqCst);
    let _active = Active(state.clone());
    let body = if path.contains("/version") {
        r#"{"ApiVersion":"1.49","MinAPIVersion":"1.40"}"#.into()
    } else if path.contains("/info") {
        r#"{"ID":"fixture-daemon","MemTotal":8000000000}"#.into()
    } else if path.contains("/containers/json") {
        state.inventory_requests.fetch_add(1, Ordering::SeqCst);
        if path.contains("all=false") {
            assert!(
                path.contains("limit=1"),
                "idle probe must list only one running row"
            );
            state.probe_requests.fetch_add(1, Ordering::SeqCst);
        }
        let rows:Vec<_>=(0..state.count.load(Ordering::SeqCst)).map(|n|serde_json::json!({"Id":format!("c{n:04}"),"Names":[format!("/container-{n}")],"Image":"fixture","State":if n >= state.running_from.load(Ordering::SeqCst) { "running" } else { "exited" }})).collect();
        serde_json::to_string(&rows).unwrap()
    } else if path.contains("/stats") {
        state.stats.lock_ok().insert(id.clone());
        if state.delay_ms > 0 {
            tokio::select! {_=tokio::time::sleep(Duration::from_millis(state.delay_ms))=>{},_=socket.read(&mut one)=>return}
        }
        r#"{"cpu_stats":{"cpu_usage":{"total_usage":1000},"system_cpu_usage":10000,"online_cpus":8},"memory_stats":{"usage":1000,"limit":10000,"stats":{"inactive_file":100}},"networks":{"eth0":{"rx_bytes":100,"tx_bytes":200}}}"#.into()
    } else {
        state.inspect_requests.fetch_add(1, Ordering::SeqCst);
        let status = state.inspect_status.load(Ordering::SeqCst);
        if status != 200 {
            let body = br#"{"message":"private daemon detail"}"#;
            let header = format!(
                "HTTP/1.1 {status} Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(header.as_bytes()).await;
            let _ = socket.write_all(body).await;
            return;
        }
        let lifecycle = if id
            .strip_prefix('c')
            .and_then(|n| n.parse::<usize>().ok())
            .is_none_or(|n| n >= state.running_from.load(Ordering::SeqCst))
        {
            "running"
        } else {
            "exited"
        };
        format!(
            r#"{{"Id":"{id}","State":{{"Status":"{lifecycle}","OOMKilled":false}},"HostConfig":{{"Memory":10000}},"RestartCount":0,"Config":{{"Env":["SECRET=not-published"],"Cmd":["hidden-command"]}}}}"#
        )
    };
    reply(&mut socket, body.as_bytes()).await;
}
async fn reply(socket: &mut UnixStream, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = socket.write_all(header.as_bytes()).await;
    let _ = socket.write_all(body).await;
}
async fn chunk(socket: &mut UnixStream, body: &[u8]) -> bool {
    socket
        .write_all(format!("{:x}\r\n", body.len()).as_bytes())
        .await
        .is_ok()
        && socket.write_all(body).await.is_ok()
        && socket.write_all(b"\r\n").await.is_ok()
}
fn multiplex(body: &[u8], stream: u8) -> Vec<u8> {
    let mut frame = vec![stream, 0, 0, 0];
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(body);
    frame
}
async fn until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(4), async {
        while !predicate() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

// With paused time, a missing state must fail instead of yielding forever while
// its logical timeout cannot advance. Socket fixtures settle without clock ticks.
async fn settled(mut predicate: impl FnMut() -> bool) {
    for _ in 0..10_000 {
        if predicate() {
            return;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        predicate(),
        "worker did not settle at the current logical time"
    );
}

#[tokio::test]
async fn partial_cycles_never_exceed_eight_requests_and_do_not_starve_the_tail() {
    let fixture = Fixture::new(40, 2500).await;
    let handle = fixture.handle();
    handle.set_visible(true);
    let worker = tokio::spawn(handle.clone().run());
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            if fixture.state.stats.lock_ok().len() == 40 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(fixture.state.maximum.load(Ordering::SeqCst) <= 8);
    assert!(
        handle
            .snapshot()
            .coverage
            .iter()
            .any(|g| g.reason == "partial_cycle")
    );
    handle.configure(DockerSettings::default()).unwrap();
    let generation = handle.snapshot().generation;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(handle.snapshot().generation, generation);
    assert_eq!(handle.snapshot().status, DockerStatus::Disabled);
    assert!(handle.snapshot().containers.is_empty());
    until(|| fixture.state.active.load(Ordering::SeqCst) == 0).await;
    handle.shutdown();
    worker.await.unwrap();
}
#[tokio::test]
async fn shared_quiet_logs_use_two_slots_and_recover_after_unsubscribe() {
    let fixture = Fixture::new(1, 0).await;
    let handle = fixture.handle();
    let adapter = fixture.adapter().await;
    handle.inner.lock_ok().adapter = Some(adapter);
    let request = |id: &str| DockerLogsRequest {
        engine_id: "fixture-daemon".into(),
        container_id: id.into(),
        follow: true,
    };
    let mut first = handle.open_logs(request("one")).unwrap();
    until(|| fixture.state.log_follow_requests.load(Ordering::SeqCst) == 1).await;
    let _ = fixture
        .state
        .logs
        .send(("one".into(), b"cached tail\n".to_vec()));
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), first.receiver.recv())
            .await
            .unwrap()
            .unwrap()
            .lines,
        ["cached tail"]
    );
    let shared = handle.open_logs(request("one")).unwrap();
    assert_eq!(shared.initial.as_ref().unwrap().lines, ["cached tail"]);
    let mut second = handle.open_logs(request("two")).unwrap();
    assert!(handle.open_logs(request("three")).is_err());
    until(|| fixture.state.log_follow_requests.load(Ordering::SeqCst) == 2).await;
    handle.close_logs(first.id);
    assert!(handle.open_logs(request("three")).is_err());
    handle.close_logs(shared.id);
    let third = handle.open_logs(request("three")).unwrap();
    until(|| fixture.state.log_follow_requests.load(Ordering::SeqCst) == 3).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(2200), second.receiver.recv())
            .await
            .is_err(),
        "quiet follow remains connected without an idle timeout"
    );
    let _ = fixture
        .state
        .logs
        .send(("two".into(), b"still connected\n".to_vec()));
    let batch = tokio::time::timeout(Duration::from_secs(1), second.receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(batch.lines, ["still connected"]);
    assert!(!batch.complete);
    handle.close_logs(second.id);
    handle.close_logs(third.id);
    handle.shutdown();
}
#[tokio::test]
async fn tty_and_multiplexed_tails_are_plain_text_and_large_logs_do_not_block_other_budgets() {
    let fixture = Fixture::new(1, 0).await;
    let adapter = fixture.adapter().await;
    let (rows, _) = adapter.inventory().await.unwrap();
    let inspected = adapter
        .inspect(rows.into_iter().next().unwrap())
        .await
        .unwrap();
    let published = serde_json::to_string(&inspected).unwrap();
    assert!(!published.contains("SECRET") && !published.contains("hidden-command"));
    let handle = fixture.handle();
    handle.inner.lock_ok().adapter = Some(adapter.clone());
    for (id, expected) in [
        ("tty", vec!["plain tty"]),
        ("mux", vec!["stdout", "stderr"]),
    ] {
        let mut sub = handle
            .open_logs(DockerLogsRequest {
                engine_id: "fixture-daemon".into(),
                container_id: id.into(),
                follow: false,
            })
            .unwrap();
        let mut lines = Vec::new();
        loop {
            let batch = tokio::time::timeout(Duration::from_secs(1), sub.receiver.recv())
                .await
                .unwrap()
                .unwrap();
            lines.extend(batch.lines);
            if batch.complete {
                break;
            }
        }
        assert_eq!(lines, expected);
        handle.close_logs(sub.id);
    }
    let mut logs = handle
        .open_logs(DockerLogsRequest {
            engine_id: "fixture-daemon".into(),
            container_id: "burst".into(),
            follow: true,
        })
        .unwrap();
    until(|| fixture.state.log_follow_requests.load(Ordering::SeqCst) == 1).await;
    let events_adapter = adapter.clone();
    let event_task = tokio::spawn(async move {
        let mut events = Box::pin(events_adapter.events());
        events.next().await
    });
    until(|| fixture.state.events.receiver_count() > 0).await;
    let _ = fixture
        .state
        .logs
        .send(("burst".into(), multiplex(&vec![b'x'; 3 * 1024 * 1024], 1)));
    let _=fixture.state.events.send(br#"{"Type":"container","Action":"oom","Actor":{"ID":"c0000"},"timeNano":1770000000000000000}
"#.to_vec());
    assert!(
        tokio::time::timeout(Duration::from_secs(1), adapter.stats("c0000"))
            .await
            .unwrap()
            .is_ok()
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), event_task)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .unwrap()
            .action,
        "oom"
    );
    let batch = tokio::time::timeout(Duration::from_secs(2), logs.receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(batch.truncated && batch.complete);
    handle.close_logs(logs.id);
    handle.shutdown();
}

#[test]
fn observed_restart_pairs_deduplicate_explicit_restart_and_do_not_infer_oom_from_137() {
    let handle = DockerHandle::new(
        HostId(uuid::Uuid::new_v4()),
        DockerSettings::default(),
        PathBuf::new(),
    );
    let mut restarts = BTreeMap::new();
    let start = now_ms();
    let event = |action: &str, offset: i64| DockerEvent {
        id: format!("{action}-{offset}"),
        engine_id: "fixture".into(),
        container_id: Some("container".into()),
        time_ns: ((start + offset) * 1_000_000).to_string(),
        time_ms: start + offset,
        action: action.into(),
        name: None,
        exit_code: (action == "die").then(|| "137".into()),
        attributes: BTreeMap::new(),
    };
    handle.record_event(event("start", 0), &mut restarts, 1);
    assert!(restarts.is_empty());
    for n in 0..3 {
        let offset = 10 + n * 10;
        handle.record_event(event("die", offset), &mut restarts, 1);
        handle.record_event(event("start", offset + 1), &mut restarts, 1);
        handle.record_event(event("restart", offset + 2), &mut restarts, 1);
    }
    assert_eq!(restarts.get("container").unwrap().len(), 3);
    assert!(
        !handle
            .inner
            .lock_ok()
            .events
            .iter()
            .any(|e| e.action == "oom")
    );
    let before = handle.inner.lock_ok().events.len();
    handle.record_event(event("oom", 100), &mut restarts, 0);
    assert_eq!(
        handle.inner.lock_ok().events.len(),
        before,
        "stale generation events cannot reach live or persisted history"
    );
}

#[tokio::test]
async fn mixed_tail_and_follow_share_one_container_slot_in_both_open_orders() {
    for first_follows in [false, true] {
        let fixture = Fixture::new(1, 0).await;
        let handle = fixture.handle();
        handle.inner.lock_ok().adapter = Some(fixture.adapter().await);
        let request = |id: &str, follow| DockerLogsRequest {
            engine_id: "fixture-daemon".into(),
            container_id: id.into(),
            follow,
        };
        let first = handle.open_logs(request("one", first_follows)).unwrap();
        let second = handle.open_logs(request("one", !first_follows)).unwrap();
        let (mut tail, mut follow) = if first_follows {
            (second, first)
        } else {
            (first, second)
        };
        let other = handle
            .open_logs(request("two", true))
            .expect("mixed modes use only one of two container slots");
        until(|| fixture.state.log_follow_requests.load(Ordering::SeqCst) == 2).await;
        if !tail.initial.as_ref().is_some_and(|b| b.complete) {
            let complete = tokio::time::timeout(Duration::from_secs(1), tail.receiver.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(complete.complete, "tail completes independently of Follow");
        }
        handle.close_logs(tail.id);
        let _ = fixture
            .state
            .logs
            .send(("one".into(), b"following after tail closed\n".to_vec()));
        let batch = tokio::time::timeout(Duration::from_secs(1), follow.receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(batch.lines, ["following after tail closed"]);
        assert!(!batch.complete);
        assert_eq!(
            fixture.state.log_requests.load(Ordering::SeqCst),
            4,
            "one finite tail then one Follow request per container"
        );
        handle.close_logs(follow.id);
        handle.close_logs(other.id);
        handle.shutdown();
    }
}

#[tokio::test]
async fn retained_removed_detail_and_inspect_races_preserve_metadata_without_hiding_other_errors() {
    let fixture = Fixture::new(1, 0).await;
    let handle = fixture.handle();
    let adapter = fixture.adapter().await;
    let mut row = adapter.inventory().await.unwrap().0.remove(0);
    row.name = "retained name".into();
    row.lifecycle = DockerLifecycle::Removed;
    handle.inner.lock_ok().adapter = Some(adapter.clone());
    handle.changed.send_modify(|s| {
        s.engine = Some(adapter.engine.clone());
        s.containers = vec![row.clone()];
    });
    fixture.state.inspect_status.store(404, Ordering::SeqCst);
    let detail = handle.detail_live("fixture-daemon", &row.id).await.unwrap();
    assert_eq!(detail.name, "retained name");
    assert_eq!(fixture.state.inspect_requests.load(Ordering::SeqCst), 0);
    handle.recorder.record(&handle.snapshot());
    handle.changed.send_modify(|s| s.containers.clear());
    assert_eq!(
        handle
            .detail_live("fixture-daemon", &row.id)
            .await
            .unwrap()
            .name,
        "retained name"
    );
    assert_eq!(
        fixture.state.inspect_requests.load(Ordering::SeqCst),
        0,
        "history-only detail never inspects"
    );
    row.lifecycle = DockerLifecycle::Running;
    handle
        .changed
        .send_modify(|s| s.containers = vec![row.clone()]);
    assert_eq!(
        handle
            .detail_live("fixture-daemon", &row.id)
            .await
            .unwrap()
            .lifecycle,
        DockerLifecycle::Removed
    );
    // The 404 is newer removal evidence until inventory lists the container again;
    // after that, a failing inspection must surface rather than be masked.
    handle
        .changed
        .send_modify(|s| s.containers = vec![row.clone()]);
    fixture.state.inspect_status.store(500, Ordering::SeqCst);
    let error = handle
        .detail_live("fixture-daemon", &row.id)
        .await
        .unwrap_err();
    assert!(!error.contains("private daemon detail"));
    handle.shutdown();
}

#[tokio::test(start_paused = true)]
async fn successful_handshake_with_repeated_event_eof_backs_off_and_shutdown_interrupts_wait() {
    let fixture = Fixture::new(1, 0).await;
    fixture.state.event_eof.store(true, Ordering::SeqCst);
    let handle = fixture.handle();
    handle.set_visible(true);
    let worker = tokio::spawn(handle.clone().run());
    for (attempt, backoff) in [(1, 1), (2, 2), (3, 4)] {
        settled(|| handle.snapshot().status == DockerStatus::Connected).await;
        assert_eq!(fixture.state.event_requests.load(Ordering::SeqCst), attempt);
        tokio::time::advance(Duration::from_secs(1)).await;
        settled(|| handle.snapshot().status == DockerStatus::Unavailable).await;
        if attempt == 3 {
            break;
        }
        tokio::time::advance(Duration::from_millis(backoff * 1000 - 1)).await;
        assert_eq!(fixture.state.event_requests.load(Ordering::SeqCst), attempt);
        tokio::time::advance(Duration::from_millis(1)).await;
    }
    handle.shutdown();
    worker.await.unwrap();
    tokio::time::advance(Duration::from_secs(60)).await;
    assert_eq!(fixture.state.event_requests.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn tail_follow_boundary_removes_only_the_counted_timestamped_overlap() {
    let fixture = Fixture::new(1, 0).await;
    let handle = fixture.handle();
    handle.inner.lock_ok().adapter = Some(fixture.adapter().await);
    let mut follow = handle
        .open_logs(DockerLogsRequest {
            engine_id: "fixture-daemon".into(),
            container_id: "overlap".into(),
            follow: true,
        })
        .unwrap();
    until(|| fixture.state.log_follow_requests.load(Ordering::SeqCst) == 1).await;
    let mut initial = Vec::new();
    while initial.len() < 2 {
        initial.extend(follow.receiver.recv().await.unwrap().lines);
    }
    assert_eq!(
        initial.len(),
        2,
        "identical timestamped records in the tail retain multiplicity"
    );
    let _ = fixture.state.logs.send(("overlap".into(), b"2026-10-11T12:00:00.000000000Z repeated\n2026-10-11T12:00:00.000000000Z repeated\n2026-10-11T12:00:00.000000000Z repeated\n2026-10-11T12:00:01.000000000Z next\n".to_vec()));
    let mut next = Vec::new();
    while next.len() < 2 {
        next.extend(
            tokio::time::timeout(Duration::from_secs(1), follow.receiver.recv())
                .await
                .unwrap()
                .unwrap()
                .lines,
        );
    }
    assert_eq!(
        next,
        [
            "2026-10-11T12:00:00.000000000Z repeated",
            "2026-10-11T12:00:01.000000000Z next"
        ]
    );
    handle.close_logs(follow.id);
    handle.shutdown();
}

#[tokio::test]
async fn finite_tail_includes_current_second_logs_with_timestamp_filters_honored() {
    let fixture = Fixture::new(1, 0).await;
    let handle = fixture.handle();
    handle.inner.lock_ok().adapter = Some(fixture.adapter().await);
    let mut tail = handle
        .open_logs(DockerLogsRequest {
            engine_id: "fixture-daemon".into(),
            container_id: "filtered".into(),
            follow: false,
        })
        .unwrap();
    let mut lines = Vec::new();
    loop {
        let batch = tokio::time::timeout(Duration::from_secs(1), tail.receiver.recv())
            .await
            .unwrap()
            .unwrap();
        lines.extend(batch.lines);
        if batch.complete {
            break;
        }
    }
    assert_eq!(
        lines,
        ["current-second burst"],
        "finite latest200 must not exclude the current partial second"
    );
    handle.close_logs(tail.id);
    handle.shutdown();
}

#[tokio::test]
async fn hiding_one_log_pane_notifies_it_and_show_can_resume_shared_follow() {
    let fixture = Fixture::new(1, 0).await;
    let handle = fixture.handle();
    handle.inner.lock_ok().adapter = Some(fixture.adapter().await);
    let request = || DockerLogsRequest {
        engine_id: "fixture-daemon".into(),
        container_id: "one".into(),
        follow: true,
    };
    let mut hidden_pane = handle.open_logs(request()).unwrap();
    let mut visible_pane = handle.open_logs(request()).unwrap();
    until(|| fixture.state.log_follow_requests.load(Ordering::SeqCst) == 1).await;
    handle.cancel_logs(
        hidden_pane.id,
        "Logs paused because this window was hidden; reopen logs to continue",
    );
    let terminal = tokio::time::timeout(Duration::from_secs(1), hidden_pane.receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(terminal.complete);
    assert!(terminal.error.unwrap().contains("window was hidden"));
    let _ = fixture
        .state
        .logs
        .send(("one".into(), b"other pane still following\n".to_vec()));
    let live = tokio::time::timeout(Duration::from_secs(1), visible_pane.receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(live.lines, ["other pane still following"]);
    assert!(!live.complete);
    let mut resumed = handle.open_logs(request()).unwrap();
    assert_eq!(
        resumed.initial.take().unwrap().lines,
        ["other pane still following"]
    );
    let _ = fixture
        .state
        .logs
        .send(("one".into(), b"resumed\n".to_vec()));
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), resumed.receiver.recv())
            .await
            .unwrap()
            .unwrap()
            .lines,
        ["resumed"]
    );
    assert_eq!(
        fixture.state.log_follow_requests.load(Ordering::SeqCst),
        1,
        "reopening joins the still-active shared pipeline"
    );
    handle.close_logs(visible_pane.id);
    handle.close_logs(resumed.id);
    handle.shutdown();
}

#[tokio::test]
async fn reset_publishes_new_generation_and_drops_live_detail_and_event_cursor_immediately() {
    let fixture = Fixture::new(1, 0).await;
    let handle = fixture.handle();
    let row = fixture
        .adapter()
        .await
        .inventory()
        .await
        .unwrap()
        .0
        .remove(0);
    handle.changed.send_modify(|s| s.containers = vec![row]);
    handle.inner.lock_ok().events.push_back(DockerEvent {
        id: "old".into(),
        engine_id: "fixture-daemon".into(),
        container_id: None,
        time_ns: "1".into(),
        time_ms: 1,
        action: "start".into(),
        name: None,
        exit_code: None,
        attributes: BTreeMap::new(),
    });
    let previous = handle.snapshot().generation;
    let generation = handle.invalidate_history();
    assert_ne!(generation, previous);
    assert_eq!(handle.snapshot().generation, generation);
    assert!(handle.snapshot().containers.is_empty());
    assert!(handle.inner.lock_ok().events.is_empty());
}

#[tokio::test]
async fn metadata_receipt_tracks_inspection_independently_and_404_clears_live_observations() {
    let fixture = Fixture::new(1, 0).await;
    let handle = fixture.handle();
    let adapter = fixture.adapter().await;
    let mut row = adapter.inventory().await.unwrap().0.remove(0);
    assert_eq!(row.metadata_observed_ms, None);
    let mut old_json = serde_json::to_value(&row).unwrap();
    old_json
        .as_object_mut()
        .unwrap()
        .remove("metadata_observed_ms");
    let old: DockerContainer = serde_json::from_value(old_json).unwrap();
    assert_eq!(
        old.metadata_observed_ms, None,
        "older stored metadata remains readable"
    );
    let before = now_ms();
    let inspected = adapter.inspect(row.clone()).await.unwrap();
    assert!(inspected.metadata_observed_ms.unwrap() >= before);
    assert_eq!(
        inspected.observed_ms, row.observed_ms,
        "inspection does not fabricate a stats sample"
    );
    row.observed_ms = Some(before);
    row.interval_ms = Some(1000);
    row.metrics.cpu_pct = Some(12.);
    handle
        .changed
        .send_modify(|s| s.containers = vec![row.clone()]);
    handle.inner.lock_ok().adapter = Some(adapter);
    fixture.state.inspect_status.store(404, Ordering::SeqCst);
    let removed = handle.detail_live("fixture-daemon", &row.id).await.unwrap();
    assert_eq!(removed.lifecycle, DockerLifecycle::Removed);
    assert!(removed.metadata_observed_ms.unwrap() >= before);
    assert_eq!(removed.observed_ms, None);
    assert_eq!(removed.interval_ms, None);
    assert_eq!(removed.metrics.cpu_pct, None);
    handle.shutdown();
}

#[tokio::test]
async fn detail_inspection_does_not_steal_stats_capacity() {
    let fixture = Fixture::new(8, 200).await;
    let adapter = fixture.adapter().await;
    let (rows, _) = adapter.inventory().await.unwrap();
    let tasks: Vec<_> = rows
        .iter()
        .map(|row| {
            let adapter = adapter.clone();
            let id = row.id.clone();
            tokio::spawn(async move { adapter.stats(&id).await })
        })
        .collect();
    until(|| fixture.state.stats.lock_ok().len() == 8).await;
    let detail = adapter.inspect_detail(rows[0].clone()).await;
    assert!(
        detail.is_ok(),
        "detail must not collide with an eight-request cycle: {detail:?}"
    );
    for task in tasks {
        assert!(task.await.unwrap().is_ok());
    }
}

#[tokio::test]
async fn daemon_inventory_replaces_stopped_population_with_new_running_container() {
    let fixture = Fixture::new(128, 0).await;
    let handle = fixture.handle();
    let mut adapter = fixture.adapter().await;
    let mut rows = Vec::new();
    let mut baseline = BTreeMap::new();
    let mut cursor = 0;
    let mut inventory_at = Instant::now();
    let mut capacity_at = Instant::now();
    let mut complete = true;
    let mut population = BTreeSet::new();
    let mut revision = 0;
    for (count, running_from) in [(128, 0), (129, 128), (128, 0), (130, 0)] {
        fixture.state.count.store(count, Ordering::SeqCst);
        fixture
            .state
            .running_from
            .store(running_from, Ordering::SeqCst);
        assert!(
            handle
                .cycle(
                    &mut adapter,
                    &mut rows,
                    &mut baseline,
                    &mut cursor,
                    &mut inventory_at,
                    &mut capacity_at,
                    &mut complete,
                    &mut population,
                    &mut revision,
                    true,
                    1
                )
                .await
        );
        assert_eq!(
            rows.len(),
            count,
            "all provisioned containers remain visible"
        );
        assert_eq!(population.len(), 128);
        let live = aggregate(&rows, None, complete, now_ms(), 1000);
        assert_eq!(live.expected, (count - running_from).min(128) as u32);
        assert_eq!(live.running, (count - running_from) as u32);
        if count == 130 {
            assert!(live.partial);
        }
        if count == 129 {
            assert!(population.contains("c0128"));
            assert!(baseline.contains_key("c0128"));
            assert_eq!(baseline.len(), 1);
        } else {
            assert!(!population.contains("c0128"));
            assert!(!baseline.contains_key("c0128"));
        }
    }
    assert_eq!(revision, 3);
}

#[tokio::test(start_paused = true)]
async fn idle_probe_resumes_within_one_minute_without_stats_while_idle() {
    let fixture = Fixture::new(0, 0).await;
    let handle = fixture.handle();
    let worker = tokio::spawn(handle.clone().run());
    settled(|| handle.snapshot().status == DockerStatus::IdleSuspended).await;
    assert_eq!(
        handle.snapshot().message.as_deref(),
        Some("No running containers. Monitoring paused to save power.")
    );
    let idle_at = Instant::now();
    let events = fixture.state.event_requests.load(Ordering::SeqCst);
    tokio::time::advance(Duration::from_secs(59)).await;
    assert!(fixture.state.stats.lock_ok().is_empty());
    assert_eq!(fixture.state.probe_requests.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.state.inventory_requests.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.state.event_requests.load(Ordering::SeqCst), events);
    fixture.state.count.store(1, Ordering::SeqCst);
    tokio::time::advance(Duration::from_secs(1)).await;
    settled(|| handle.snapshot().status == DockerStatus::Connected).await;
    assert_eq!(handle.snapshot().aggregate.running, 1);
    assert_eq!(idle_at.elapsed(), Duration::from_secs(60));
    assert_eq!(fixture.state.probe_requests.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.state.inventory_requests.load(Ordering::SeqCst), 3);
    handle.shutdown();
    worker.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn visible_transition_resumes_idle_without_waiting_for_probe() {
    for already_visible in [false, true] {
        let fixture = Fixture::new(0, 0).await;
        let handle = fixture.handle();
        if already_visible {
            handle.set_visible(true);
        }
        let worker = tokio::spawn(handle.clone().run());
        settled(|| handle.snapshot().status == DockerStatus::IdleSuspended).await;
        fixture.state.count.store(1, Ordering::SeqCst);
        handle.set_visible(true);
        settled(|| handle.snapshot().status == DockerStatus::Connected).await;
        assert_eq!(handle.snapshot().aggregate.running, 1);
        handle.shutdown();
        worker.await.unwrap();
    }
}

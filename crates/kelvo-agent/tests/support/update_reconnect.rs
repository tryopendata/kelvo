use super::*;
use kelvo_remote::AgentPaths;
use kelvo_schema::Cursor;
use kelvo_store::rusqlite::Connection;

#[allow(dead_code)]
#[path = "../../src/update/transaction.rs"]
mod transaction;

async fn stop(child: &mut Child) {
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(child.id().unwrap() as i32).unwrap(),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(20), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

async fn start(paths: &AgentPaths, private: bool) -> Child {
    let mut child = Command::new(&paths.binary)
        .arg(if private { "run-trial" } else { "run" })
        .arg("--data-dir")
        .arg(&paths.data_dir)
        .arg("--socket")
        .arg(&paths.socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    for _ in 0..300 {
        if tokio::net::UnixStream::connect(&paths.socket).await.is_ok() {
            return child;
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "collector exited early"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("collector never started");
}

async fn page(socket: &Path, after: Option<Cursor>) -> kelvo_proto::SyncPage {
    let mut bridge = Bridge::spawn(socket);
    let hello = bridge.hello().await;
    let negotiated = kelvo_proto::negotiate(
        &Offer::local(kelvo_proto::local_features()),
        &Offer::from(&hello),
    )
    .unwrap();
    bridge.write(&Message::HelloAck(negotiated.ack())).await;
    bridge
        .write(&Message::SyncRequest(SyncRequest {
            tier: Tier::M1,
            after,
            max_rows: 10000,
            from_ms: None,
            max_bytes: None,
        }))
        .await;
    let page = bridge
        .until(Duration::from_secs(10), |message| match message {
            Message::SyncPage(page) => Some(page),
            Message::Error { message, .. } => panic!("sync failed: {message}"),
            _ => None,
        })
        .await;
    bridge.close().await;
    page
}

fn append_gap(paths: &AgentPaths, jump: i64) -> i64 {
    let db = Connection::open(paths.data_dir.join("history.sqlite")).unwrap();
    let sequence: i64 = db
        .query_row("SELECT value FROM meta WHERE key='next_seq'", [], |row| {
            row.get(0)
        })
        .unwrap();
    let sequence = sequence + jump;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let inserted = db.execute("INSERT INTO gaps(host_id,start_ts,end_ts,reason,seq,export_id) SELECT id,?1,?2,'app_not_running',?3,?3 FROM hosts", (now - 1000, now, sequence)).unwrap();
    assert_eq!(inserted, 1);
    db.execute(
        "UPDATE meta SET value=?1 WHERE key='next_seq'",
        [sequence + 1],
    )
    .unwrap();
    sequence
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_after_failed_trial_cannot_skip_or_observe_rolled_back_sequences() {
    let mut initial = Daemon::start().await;
    stop(&mut initial.child).await;
    let paths = AgentPaths {
        binary: initial._dir.path().join("kelvo-agent"),
        data_dir: initial._dir.path().join("data"),
        socket: initial.socket.clone(),
        unit: initial._dir.path().join("units/kelvo-agent.service"),
    };
    std::fs::copy(AGENT, &paths.binary).unwrap();
    append_gap(&paths, 0);
    let mut public = start(&paths, false).await;
    let before = page(&paths.socket, None).await;
    assert!(before.last_seq > 0);
    let cursor = Cursor {
        epoch: before.epoch,
        seq: before.last_seq,
    };
    stop(&mut public).await;

    let mut journal = transaction::begin(&paths, "failed-trial").unwrap();
    transaction::prepare(&paths, &mut journal).unwrap();
    let trial_seq = append_gap(&paths, 100000);
    let private = AgentPaths {
        socket: transaction::directory(&paths).join("trial.sock"),
        ..paths.clone()
    };
    let mut trial = start(&private, true).await;
    let trial_page = page(&private.socket, Some(cursor)).await;
    assert!(trial_page.gaps.iter().any(|row| row.seq == trial_seq));
    assert!(
        tokio::net::UnixStream::connect(&paths.socket)
            .await
            .is_err()
    );
    let refused = tokio::time::timeout(
        Duration::from_secs(10),
        Command::new(&paths.binary)
            .arg("run")
            .arg("--data-dir")
            .arg(&paths.data_dir)
            .arg("--socket")
            .arg(&paths.socket)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        !refused.status.success(),
        "Prepared must never serve the public socket"
    );
    stop(&mut trial).await;
    transaction::recover(&paths).unwrap();
    let next_seq = append_gap(&paths, 0);
    assert!(next_seq > cursor.seq && next_seq < trial_seq);
    let mut restored = start(&paths, false).await;
    let after = page(&paths.socket, Some(cursor)).await;
    assert_eq!(after.epoch, cursor.epoch);
    assert!(after.last_seq < trial_seq, "failed trial sequence leaked");
    assert!(
        after.gaps.iter().any(|row| row.seq == next_seq),
        "reconnecting controller skipped new history"
    );
    stop(&mut restored).await;
}

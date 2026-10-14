use super::*;

#[test]
fn rejected_content_retries_without_claiming_measurement_loss() {
    let r = DockerRecorder::new(host().id);
    r.record(&fixture(10_000));
    let mut s = r.state.lock_ok();
    let key = ("daemon".into(), "c".into(), 0);
    s.entries.get_mut(&key).unwrap().pins = 1;
    let (tx, rx) = mpsc::sync_channel(1);
    let event = db::DockerEvent {
        entity: Some("c".into()),
        time_ns: 1000,
        action: "start".into(),
        attributes: BTreeMap::new(),
    };
    let coverage = db::DockerCoverage {
        entity: Some("c".into()),
        metric: None,
        start_ms: 0,
        end_ms: Some(1),
        reason: "events_disconnected".into(),
        revision: 1,
        sampled_ms: 0,
        expected_ms: 1,
    };
    s.pending.push(Receipt {
        rx,
        retry: Retry {
            events: vec![event.clone()],
            coverage: vec![coverage.clone()],
            ..Default::default()
        },
        keys: vec![key.clone()],
        engine: "daemon".into(),
        start: 0,
        end: 10_000,
    });
    tx.send(db::DockerAck {
        generation: 1,
        revision: 1,
        outcome: db::DockerOutcome::Rejected,
        reason: Some("queue full".into()),
    })
    .unwrap();
    drain_at(&mut s, 10_000);
    assert!(
        s.coverage
            .values()
            .flatten()
            .all(|g| g.reason != "write_failed")
    );
    let entry = s.entries.get(&key).unwrap();
    assert_eq!(entry.pins, 0);
    assert!(entry.bucket.revision > entry.durable);
    let retry = r.batch(&mut s, "daemon", 1, 11_000);
    assert_eq!(retry.events, vec![event]);
    assert_eq!(retry.coverage, vec![coverage]);
    assert!(s.retry.is_empty());
}

fn assert_resumed_engine_closes_gap(reason: &str) {
    let (_dir, store, r) = setup();
    let t = wall_ms();
    let mut s = fixture(t);
    s.coverage.push(api::DockerCoverage {
        start_ms: t + 50,
        end_ms: None,
        container_id: None,
        reason: reason.into(),
    });
    r.record(&s);
    store.writer().flush().unwrap();
    s.coverage.clear();
    at(&mut s, t + 2000, 1000);
    r.record(&s);
    let page = r.history(&query(t, t + 10000, 10)).unwrap();
    let gaps: Vec<_> = page
        .coverage
        .iter()
        .filter(|g| g.reason == reason)
        .collect();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].end_ms, Some(t + 2000));
    assert!(
        r.history(&query(t + 2001, t + 10000, 10))
            .unwrap()
            .coverage
            .iter()
            .all(|g| g.reason != reason)
    );
    assert!(
        r.comparison(&api::DockerComparisonRequest {
            engine_id: "daemon".into(),
            from_ms: t + 2001,
            to_ms: t + 10000,
            metric: api::DockerComparisonMetric::Cpu,
            project: None,
            selected: None,
            max_points: 10
        })
        .unwrap()
        .coverage
        .iter()
        .all(|g| g.reason != reason)
    );
    let comparison = r
        .comparison(&api::DockerComparisonRequest {
            engine_id: "daemon".into(),
            from_ms: t,
            to_ms: t + 10000,
            metric: api::DockerComparisonMetric::Cpu,
            project: None,
            selected: None,
            max_points: 10,
        })
        .unwrap();
    let gaps: Vec<_> = comparison
        .coverage
        .iter()
        .filter(|g| g.reason == reason)
        .collect();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].end_ms, Some(t + 2000));
}

#[test]
fn resumed_engine_closes_switch_gap_before_commit() {
    assert_resumed_engine_closes_gap("engine_switch");
}

#[test]
fn resumed_engine_closes_shutdown_gap_before_commit() {
    assert_resumed_engine_closes_gap("shutdown");
}

#[test]
fn resumed_engine_closes_paused_gap_before_commit() {
    assert_resumed_engine_closes_gap("paused");
}

#[test]
fn resumed_engine_closes_collector_interruptions_before_commit() {
    for reason in ["disabled", "disconnected", "idle_suspended"] {
        assert_resumed_engine_closes_gap(reason);
    }
}

#[test]
fn quit_and_reopen_keeps_cpu_and_memory_visible_before_and_after_commit() {
    assert_restart_keeps_cpu_and_memory_visible("shutdown");
}

#[test]
fn restart_while_paused_keeps_cpu_and_memory_visible_before_and_after_commit() {
    assert_restart_keeps_cpu_and_memory_visible("paused");
}

fn assert_restart_keeps_cpu_and_memory_visible(reason: &str) {
    let (dir, store, unused) = setup();
    drop(unused);
    let handle =
        crate::docker::DockerHandle::new(host().id, api::DockerSettings::default(), PathBuf::new());
    handle
        .recorder
        .attach(store.writer(), store.path().to_path_buf());
    let mut sample = fixture(wall_ms() - 1000);
    sample.generation = handle.snapshot().generation;
    handle.publish(sample);
    if reason == "shutdown" {
        handle.shutdown();
    } else {
        handle.set_power(true, 1000);
        let generation = handle.control.borrow().generation;
        assert!(handle.transition(generation, api::DockerStatus::Paused, None, reason));
        // Persist the pause, then drop the process without a graceful shutdown.
        handle.recorder.shutdown_flush().unwrap();
    }
    let interrupted = handle.snapshot().coverage.last().unwrap().start_ms;
    drop(handle);
    store.close().unwrap();

    let mut config = StoreConfig::new(dir.path().join("h.sqlite"));
    config.commit_interval = Duration::from_secs(300);
    let store = Store::open(config).unwrap();
    let restarted = DockerRecorder::new(host().id);
    restarted.attach(store.writer(), store.path().to_path_buf());
    // Deterministic source timestamps model downtime and a new process. No
    // sampler, recorder, query, or SQLite implementation is mocked.
    let resume = (interrupted + 20_000).div_euclid(10_000) * 10_000;
    let before = restarted.history(&query(interrupted, resume, 10)).unwrap();
    assert!(
        before
            .coverage
            .iter()
            .any(|g| g.reason == reason && g.end_ms.is_none())
    );
    restarted.record(&fixture(resume + 1000));
    restarted.record(&fixture(resume + 2000));
    for committed in [false, true] {
        if committed {
            restarted.record(&fixture(resume + 11_000));
            store.writer().flush().unwrap();
        }
        for metric in [
            api::DockerComparisonMetric::Cpu,
            api::DockerComparisonMetric::Memory,
        ] {
            let page = restarted
                .comparison(&api::DockerComparisonRequest {
                    engine_id: "daemon".into(),
                    from_ms: interrupted,
                    to_ms: resume + 10_000,
                    metric,
                    project: None,
                    selected: None,
                    max_points: 30,
                })
                .unwrap();
            assert_eq!(page.series.len(), 1);
            assert!(page.series[0].points.iter().any(|p| p.time_ms >= resume
                && p.metrics.cpu_pct == Some(100.)
                && p.metrics.working_set_bytes == Some(128.)));
            let gaps: Vec<_> = page
                .coverage
                .iter()
                .filter(|g| g.reason == reason)
                .collect();
            assert_eq!(gaps.len(), 1);
            assert!(
                gaps[0]
                    .end_ms
                    .is_some_and(|end| end > interrupted && end <= resume + 1000),
                "real downtime stays bounded after restart, committed={committed}: {gaps:?}"
            );
        }
        let page = restarted
            .history(&query(resume + 1001, resume + 10_000, 10))
            .unwrap();
        assert!(!page.points.is_empty());
        assert!(page.coverage.iter().all(|g| g.reason != reason));
    }
}

#[test]
fn a_new_gap_after_the_last_observation_does_not_poison_retries() {
    let (_dir, store, r) = setup();
    let t = wall_ms();
    let mut snapshot = fixture(t);
    snapshot.coverage.push(api::DockerCoverage {
        start_ms: t + 50,
        end_ms: None,
        container_id: None,
        reason: "event_replay_uncertain".into(),
    });
    r.record(&snapshot);
    store.writer().flush().unwrap();
    assert_eq!(r.status().error, None);
    at(&mut snapshot, t + 11_000, 10_000);
    snapshot.coverage[0].end_ms = Some(t + 11_000);
    r.record(&snapshot);
    store.writer().flush().unwrap();
    assert_eq!(r.status().error, None);
    assert!(r.state.lock_ok().retry.is_empty());
}

#[test]
fn failed_writes_release_pins_and_replay_metadata_with_samples() {
    let (_dir, store, r) = setup();
    let t = wall_ms().div_euclid(900_000) * 900_000;
    let mut s = fixture(t + 1000);
    r.record(&s);
    store.writer().flush().unwrap();
    store.writer().set_fail_commits(true);
    at(&mut s, t + 11_000, 10000);
    s.containers[0].name = "renamed".into();
    r.record(&s);
    assert!(store.writer().flush().is_err());
    r.status();
    {
        let state = r.state.lock_ok();
        assert!(state.entries.values().all(|e| e.pins == 0));
        assert!(
            state.coverage["daemon"]
                .iter()
                .all(|g| g.reason != "write_failed")
        );
    }
    store.writer().set_fail_commits(false);
    at(&mut s, t + 21_000, 10000);
    r.record(&s);
    store.writer().flush().unwrap();
    r.status();
    let mut reader = store.reader().unwrap();
    let snap = reader.docker_snapshot(host().id, "daemon").unwrap();
    let page = snap
        .history(&db::DockerHistoryQuery {
            entity: "c".into(),
            start_ms: t,
            end_ms: t + 60_000,
            tier: Tier::S10,
            max_points: 100,
        })
        .unwrap();
    assert!(page.metadata.iter().any(|e| e.name == "renamed"));
    assert!(!page.buckets.is_empty());
}

#[test]
fn shutdown_commits_latest_open_bucket_after_pending_older_revision() {
    let (_dir, store, r) = setup();
    let t = wall_ms().div_euclid(10_000) * 10_000;
    let mut s = fixture(t + 1000);
    r.record(&s);
    // Metadata is still pending, and both samples belong to the open bucket.
    at(&mut s, t + 2000, 1000);
    r.record(&s);
    r.shutdown_flush().unwrap();
    let mut reader = store.reader().unwrap();
    let snap = reader.docker_snapshot(host().id, "daemon").unwrap();
    let page = snap
        .history(&db::DockerHistoryQuery {
            entity: "c".into(),
            start_ms: t,
            end_ms: t + 10000,
            tier: Tier::S10,
            max_points: 10,
        })
        .unwrap();
    assert_eq!(page.buckets.len(), 1);
    assert_eq!(
        page.buckets[0]
            .metrics
            .iter()
            .find(|m| m.key == "cpu_percent")
            .unwrap()
            .weight_ms,
        2000
    );
    let state = r.state.lock_ok();
    assert!(state.pending.is_empty());
    assert!(
        state
            .entries
            .values()
            .all(|e| e.pins == 0 && e.durable >= e.bucket.revision)
    );
}

#[test]
fn review_event_coverage_survives_recorder_restart() {
    let (_dir, store, r) = setup();
    let t = wall_ms();
    let mut s = fixture(t);
    s.coverage = vec![api::DockerCoverage {
        start_ms: t - 1000,
        end_ms: Some(t),
        container_id: None,
        reason: "replay_uncertain".into(),
    }];
    r.record(&s);
    store.writer().flush().unwrap();
    let fresh = DockerRecorder::new(host().id);
    fresh.attach(store.writer(), store.path().to_path_buf());
    let page = fresh
        .events(&api::DockerEventsRequest {
            engine_id: "daemon".into(),
            container_id: Some("c".into()),
            from_ms: t - 2000,
            to_ms: t + 1000,
            limit: 10,
            before_id: None,
        })
        .unwrap();
    assert_eq!(page.coverage, s.coverage);
    // A newer ring close supersedes the older durable gap identity.
    s.coverage[0].end_ms = Some(t + 500);
    at(&mut s, t + 1000, 1000);
    fresh.record(&s);
    let merged = fresh
        .events(&api::DockerEventsRequest {
            engine_id: "daemon".into(),
            container_id: Some("c".into()),
            from_ms: t - 2000,
            to_ms: t + 2000,
            limit: 10,
            before_id: None,
        })
        .unwrap();
    assert_eq!(merged.coverage, s.coverage);
}

#[test]
fn review_failed_reset_resumes_detached_ring_history_and_events() {
    let (_dir, _store, r) = setup();
    let t = wall_ms();
    let result: Result<(), String> = r.with_suspended(|| {
        r.detach();
        r.reset(2).map_err(|error| error.to_string())?;
        Err("injected store reopen failure".into())
    });
    assert!(result.is_err());
    let mut s = fixture(t);
    s.generation = 2;
    r.record(&s);
    r.event(
        &api::DockerEvent {
            id: "event".into(),
            engine_id: "daemon".into(),
            container_id: Some("c".into()),
            time_ns: (t * 1000000).to_string(),
            time_ms: t,
            action: "start".into(),
            name: None,
            exit_code: None,
            attributes: BTreeMap::new(),
        },
        2,
    );
    assert!(
        !r.history(&query(t - 10000, t + 10000, 100))
            .unwrap()
            .points
            .is_empty()
    );
    assert_eq!(
        r.events(&api::DockerEventsRequest {
            engine_id: "daemon".into(),
            container_id: None,
            from_ms: t - 1000,
            to_ms: t + 1000,
            limit: 10,
            before_id: None
        })
        .unwrap()
        .events
        .len(),
        1
    );
    assert_eq!(r.status().status, "live_only");
}

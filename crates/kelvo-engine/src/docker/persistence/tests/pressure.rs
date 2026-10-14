use super::*;

#[test]
fn eviction_records_one_scoped_contiguous_permanent_loss() {
    let r = DockerRecorder::new(host().id);
    r.record(&fixture(10_000));
    r.record(&fixture(20_000));
    let mut s = r.state.lock_ok();
    evict(&mut s, HOUR + 20_000);
    let gaps = &s.retry.get("daemon").unwrap().coverage;
    let scoped: Vec<_> = gaps
        .iter()
        .filter(|g| g.entity.as_deref() == Some("c"))
        .collect();
    assert_eq!(scoped.len(), 1);
    assert_eq!((scoped[0].start_ms, scoped[0].end_ms), (0, Some(20_000)));
    assert_eq!(scoped[0].reason, "write_failed");
    evict(&mut s, HOUR + 30_000);
    assert_eq!(
        s.retry
            .get("daemon")
            .unwrap()
            .coverage
            .iter()
            .filter(|g| g.entity.as_deref() == Some("c"))
            .count(),
        1
    );
}

#[test]
fn aggregate_eviction_does_not_gap_durable_container_history() {
    let (_dir, store, r) = setup();
    r.record(&fixture(10_000));
    store.writer().flush().unwrap();
    {
        let mut s = r.state.lock_ok();
        drain(&mut s);
        let key = ("daemon".into(), String::new(), 0);
        let revision = next(&mut s);
        Arc::make_mut(&mut s.entries.get_mut(&key).unwrap().bucket).revision = revision;
        evict_entry(&mut s, &key);
    }
    r.record(&fixture(10_000));
    store.writer().flush().unwrap();
    let container = r.history(&query(0, 20_000, 10)).unwrap();
    assert!(!container.points.is_empty());
    assert!(
        container
            .coverage
            .iter()
            .all(|gap| gap.reason != "write_failed"),
        "aggregate eviction must not invalidate durable container measurements"
    );
    let aggregate = r
        .history(&api::DockerHistoryRequest {
            container_id: None,
            ..query(0, 20_000, 10)
        })
        .unwrap();
    assert!(
        aggregate
            .coverage
            .iter()
            .any(|gap| gap.reason == "write_failed" && gap.container_id.as_deref() == Some(""))
    );
    let comparison = r
        .comparison(&api::DockerComparisonRequest {
            engine_id: "daemon".into(),
            from_ms: 0,
            to_ms: 20_000,
            metric: api::DockerComparisonMetric::Cpu,
            project: None,
            selected: Some(vec!["c".into()]),
            max_points: 10,
        })
        .unwrap();
    assert_eq!(comparison.series.len(), 1);
    assert!(
        comparison
            .coverage
            .iter()
            .all(|gap| gap.reason != "write_failed")
    );
}

#[test]
fn oversized_fresh_and_retry_payloads_drain_without_losing_evidence() {
    let (_dir, store, r) = setup();
    r.record(&fixture(10_000));
    store.writer().flush().unwrap();
    let (attached, _) = r.context();
    let mut s = r.state.lock_ok();
    drain(&mut s);
    let mut batch = r.batch(&mut s, "daemon", 1, 20_000);
    batch.entities.extend((0..100).map(|n| db::DockerEntity {
        external_id: format!("large{n}"),
        revision: batch.revision,
        name: "large".into(),
        extras: vec![b'x'; 16 * 1024],
        ..Default::default()
    }));
    batch.events.extend((0..500).map(|n| {
        db::DockerEvent {
            entity: None,
            time_ns: (n + 1) * 1_000_000,
            action: "start".into(),
            attributes: ["name", "image", "exitCode", "signal", "health_status"]
                .into_iter()
                .map(|key| (key.into(), "x".repeat(1024)))
                .collect(),
        }
    }));
    batch.coverage.extend((0..100).map(|n| db::DockerCoverage {
        entity: Some(format!("large{n}")),
        metric: None,
        start_ms: 0,
        end_ms: Some(1),
        reason: "event_replay_uncertain".into(),
        revision: batch.revision,
        sampled_ms: 0,
        expected_ms: 1,
    }));
    batch.cursor = Some(db::DockerCursor {
        time_ns: 500_000_000,
        event_key: batch.events.last().unwrap().key("daemon").unwrap(),
    });
    let seed = s
        .entries
        .get(&("daemon".into(), "c".into(), 0))
        .unwrap()
        .bucket
        .clone();
    for n in 0..1024 {
        let mut bucket = (*seed).clone();
        bucket.bucket_ms = n * 10_000;
        bucket.revision = batch.revision;
        let key = ("daemon".into(), "c".into(), bucket.bucket_ms);
        s.entries.insert(
            key,
            Entry {
                bytes: bucket_bytes(&bucket),
                bucket: Arc::new(bucket),
                pins: 0,
                durable: 0,
            },
        );
    }
    for attempt in 0..20 {
        let keys: Vec<_> = s
            .entries
            .iter()
            .filter(|(_, e)| e.pins == 0 && e.bucket.revision > e.durable)
            .map(|(key, _)| key.clone())
            .collect();
        batch.buckets = keys
            .iter()
            .map(|key| (*s.entries[key].bucket).clone())
            .collect();
        queue(&mut s, attached.as_ref(), batch, keys);
        assert!(
            s.error.is_none(),
            "valid rows must be split before admission: {:?}",
            s.error
        );
        assert!(!s.pending.is_empty());
        if s.retry
            .get("daemon")
            .is_some_and(|retry| !retry.events.is_empty() || !retry.coverage.is_empty())
        {
            assert!(
                s.pending.iter().all(|p| p.retry.cursor.is_none()),
                "cursor skips deferred evidence"
            );
        }
        if attempt == 0 {
            assert!(s.retry.contains_key("daemon"));
            assert!(
                s.entries
                    .values()
                    .any(|e| e.pins == 0 && e.bucket.revision > e.durable)
            );
        }
        drop(s);
        store.writer().flush().unwrap();
        s = r.state.lock_ok();
        drain(&mut s);
        assert!(s.error.is_none(), "{:?}", s.error);
        let conn = rusqlite::Connection::open(store.path()).unwrap();
        let events: i64 = conn
            .query_row("SELECT count(*) FROM docker_events", [], |row| row.get(0))
            .unwrap();
        let gaps: i64 = conn
            .query_row(
                "SELECT count(*) FROM docker_coverage WHERE reason='event_replay_uncertain'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let cursors: i64 = conn
            .query_row("SELECT count(*) FROM docker_event_cursors", [], |row| {
                row.get(0)
            })
            .unwrap();
        if cursors > 0 {
            assert_eq!((events, gaps), (500, 100));
        }
        if s.retry.is_empty() && s.entries.values().all(|e| e.bucket.revision <= e.durable) {
            assert!(attempt > 0, "oversized input must need multiple batches");
            assert_eq!((events, gaps, cursors), (500, 100, 1));
            let metadata: i64 = conn
                .query_row(
                    "SELECT count(*) FROM docker_metadata WHERE name='large'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(metadata, 100);
            let buckets: i64 = conn.query_row("SELECT count(*) FROM docker_10s b JOIN docker_entities e ON e.id=b.entity_id WHERE e.external_id='c'", [], |row| row.get(0)).unwrap();
            assert_eq!(buckets, 1024);
            return;
        }
        batch = r.batch(&mut s, "daemon", 1, 20_000);
    }
    panic!("retry did not drain");
}

#[test]
fn retry_events_are_bounded_and_warning_clock_is_per_engine() {
    let r = DockerRecorder::new(host().id);
    let mut s = r.state.lock_ok();
    let events = (0..4100)
        .map(|n| db::DockerEvent {
            entity: None,
            time_ns: n * 1_000_000,
            action: "start".into(),
            attributes: BTreeMap::new(),
        })
        .collect();
    requeue(
        &mut s,
        "a",
        Retry {
            events,
            ..Default::default()
        },
    );
    let retry = s.retry.get("a").unwrap();
    assert_eq!(retry.events.len(), 4096);
    assert_eq!(retry.coverage.len(), 1);
    assert_eq!(
        (retry.coverage[0].start_ms, retry.coverage[0].end_ms),
        (0, Some(4))
    );
    assert_eq!(retry.coverage[0].reason, "event_replay_uncertain");
    assert!(should_warn(&mut s, "a", 0));
    assert!(!should_warn(&mut s, "a", 59_999));
    assert!(should_warn(&mut s, "b", 59_999));
    assert!(should_warn(&mut s, "a", 60_000));
    requeue(
        &mut s,
        "b",
        Retry {
            events: (5000..5100)
                .map(|n| db::DockerEvent {
                    entity: None,
                    time_ns: n * 1_000_000,
                    action: "start".into(),
                    attributes: BTreeMap::new(),
                })
                .collect(),
            ..Default::default()
        },
    );
    assert_eq!(
        s.retry
            .values()
            .map(|retry| retry.events.len())
            .sum::<usize>(),
        4096
    );
    let mut delivered = 0;
    while let Some(engine) = s.retry.keys().next().cloned() {
        let batch = r.batch(&mut s, &engine, 1, 10_000);
        assert!(batch.events.len() <= 999);
        assert!(batch.logical_bytes().is_ok());
        delivered += batch.events.len();
    }
    assert_eq!(delivered, 4096);
}

#[test]
fn churn_accounts_for_metadata_and_engine_maps_and_expires_them() {
    let r = DockerRecorder::new(host().id);
    let t = wall_ms();
    let mut s = fixture(t);
    for i in 0..200 {
        s.engine.as_mut().unwrap().daemon_id = Some(format!("engine{i}"));
        s.containers[0].engine_id = format!("engine{i}");
        r.record(&s);
    }
    let mut state = r.state.lock_ok();
    assert!(state.aux_bytes > state.facts.len() * 512);
    evict(&mut state, t + HOUR + 60000);
    assert!(state.entries.is_empty());
    assert!(state.facts.is_empty());
    assert!(state.runtime.is_empty());
    assert!(state.bytes + state.aux_bytes + state.reader_bytes <= RING_BYTES);
}

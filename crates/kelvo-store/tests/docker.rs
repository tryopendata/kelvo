#![allow(clippy::unwrap_used)]
mod common;
use common::*;
use kelvo_schema::Tier;
use kelvo_store::{Retention, docker::*};
use std::collections::BTreeMap;
use std::sync::mpsc::TryRecvError;
fn batch(revision: i64) -> DockerBatch {
    DockerBatch {
        host: host(1).id,
        daemon_id: "daemon".into(),
        generation: 1,
        revision,
        start_ms: T0,
        end_ms: T0 + 10_000,
        runtime: "OrbStack".into(),
        entities: vec![DockerEntity {
            external_id: "c".into(),
            name: "before".into(),
            effective_ms: T0 - 1,
            revision,
            last_seen_ms: T0,
            ..Default::default()
        }],
        buckets: vec![DockerBucket {
            entity: "c".into(),
            tier: Tier::S10,
            bucket_ms: T0,
            revision,
            metrics: vec![WeightedMetric {
                key: "cpu_percent".into(),
                min: 100.,
                max: 100.,
                sum: 100. * 1000.,
                weight_ms: 1000,
                expected_ms: 10000,
            }],
            populations: vec![],
        }],
        events: vec![],
        coverage: vec![],
        cursor: None,
        populations: vec![],
    }
}
fn query(tier: Tier) -> DockerHistoryQuery {
    DockerHistoryQuery {
        entity: "c".into(),
        start_ms: T0,
        end_ms: T0 + 900_000,
        tier,
        max_points: 100,
    }
}
#[test]
fn commit_receipts_snapshot_and_weighted_rollups() {
    let dir = temp_dir("docker-snapshot");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let ack = w.write_docker(batch(1)).unwrap();
    assert!(matches!(ack.try_recv(), Err(TryRecvError::Empty)));
    w.flush().unwrap();
    assert_eq!(ack.recv().unwrap().outcome, DockerOutcome::Persisted);
    let mut reader = store.reader().unwrap();
    let snapshot = reader.docker_snapshot(host(1).id, "daemon").unwrap();
    assert_eq!(snapshot.durable_revision, 1);
    let mut b = batch(2);
    b.buckets[0].metrics[0].sum = 300. * 9000.;
    b.buckets[0].metrics[0].weight_ms = 9000;
    b.buckets[0].metrics[0].min = 300.;
    b.buckets[0].metrics[0].max = 300.;
    b.buckets[0].bucket_ms += 10_000;
    b.entities.clear();
    let ack = w.write_docker(b).unwrap();
    w.flush().unwrap();
    assert_eq!(ack.recv().unwrap().outcome, DockerOutcome::Persisted);
    assert_eq!(
        snapshot.history(&query(Tier::M1)).unwrap().buckets[0].metrics[0].average(),
        Some(100.)
    );
    drop(snapshot);
    let snapshot = reader.docker_snapshot(host(1).id, "daemon").unwrap();
    for tier in [Tier::M1, Tier::M15] {
        let h = snapshot.history(&query(tier)).unwrap();
        assert_eq!(h.buckets[0].metrics[0].average(), Some(280.));
        assert_eq!(h.buckets[0].metrics[0].weight_ms, 10000);
        assert_eq!(
            h.buckets[0].metrics[0].expected_ms,
            tier.bucket_ms().unwrap() as u64
        );
    }
    assert_eq!(
        snapshot.history(&query(Tier::S10)).unwrap().metadata[0].name,
        "before"
    );
}
#[test]
fn events_deduplicate_full_identity_and_cursor_is_atomic() {
    let dir = temp_dir("docker-events");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let mut b = batch(1);
    let event = DockerEvent {
        entity: Some("c".into()),
        time_ns: T0 * 1_000_000 + 123,
        action: "oom".into(),
        attributes: BTreeMap::new(),
    };
    let mut other = event.clone();
    other.entity = Some("d".into());
    assert_ne!(event.key("daemon").unwrap(), other.key("daemon").unwrap());
    b.events = vec![event.clone(), event.clone(), other];
    b.cursor = Some(DockerCursor {
        time_ns: event.time_ns,
        event_key: event.key("daemon").unwrap(),
    });
    let ack = w.write_docker(b).unwrap();
    w.flush().unwrap();
    assert_eq!(ack.recv().unwrap().outcome, DockerOutcome::Persisted);
    let mut reader = store.reader().unwrap();
    let snap = reader.docker_snapshot(host(1).id, "daemon").unwrap();
    assert_eq!(
        snap.events(&DockerEventQuery {
            entity: None,
            start_ms: T0,
            end_ms: T0 + 1,
            after: None,
            limit: 200
        })
        .unwrap()
        .len(),
        2
    );
    assert_eq!(snap.cursor().unwrap().unwrap().time_ns, event.time_ns);
    drop(snap);
    // A missing population dependency fails the operation savepoint including its cursor.
    let mut b = batch(2);
    b.buckets[0].entity = "".into();
    b.buckets[0].populations.push(PopulationSegment {
        revision: 99,
        start_ms: T0,
        end_ms: T0 + 10_000,
    });
    b.cursor = Some(DockerCursor {
        time_ns: event.time_ns + 999,
        event_key: "advanced".into(),
    });
    let ack = w.write_docker(b).unwrap();
    assert_eq!(ack.recv().unwrap().outcome, DockerOutcome::Failed);
    w.flush().unwrap();
    let snap = reader.docker_snapshot(host(1).id, "daemon").unwrap();
    assert_eq!(snap.cursor().unwrap().unwrap().time_ns, event.time_ns);
    assert!(
        !snap
            .history(&query(Tier::S10))
            .unwrap()
            .coverage
            .iter()
            .any(|g| g.reason == "write_failed")
    );
}
#[test]
fn discard_reset_retention_and_stale_generation() {
    let dir = temp_dir("docker-lifecycle");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let mut b = batch(1);
    b.coverage.push(DockerCoverage {
        entity: None,
        metric: None,
        start_ms: T0 - 100,
        end_ms: Some(T0 + 100_000),
        reason: "sleep".into(),
        revision: 1,
        sampled_ms: 0,
        expected_ms: 100100,
    });
    let ack = w.write_docker(b).unwrap();
    w.flush().unwrap();
    ack.recv().unwrap();
    w.discard_from(host(1).id, T0 + 5000).unwrap();
    let conn = raw(&store);
    assert_eq!(
        conn.query_row("SELECT count(*) FROM docker_10s", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT end_ms FROM docker_coverage", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        T0 + 5000
    );
    w.write_docker(batch(2)).unwrap();
    w.flush().unwrap();
    w.prune(T0 + 31 * DAY, Retention::default()).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM docker_15m", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    w.reset_docker(host(1).id, 5).unwrap();
    let ack = w.write_docker(batch(3)).unwrap();
    assert_eq!(ack.recv().unwrap().outcome, DockerOutcome::Reset);
    w.flush().unwrap();
    for t in [
        "docker_entities",
        "docker_metadata",
        "docker_populations",
        "docker_events",
        "docker_event_cursors",
        "docker_coverage",
    ] {
        assert_eq!(
            conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0,
            "{t}"
        );
    }
}
#[test]
fn ownership_foreign_keys_and_index_plans() {
    let dir = temp_dir("docker-integrity");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let mut b = batch(1);
    b.entities = (0..128)
        .map(|i| DockerEntity {
            external_id: format!("c{i}"),
            name: format!("service{i}"),
            effective_ms: T0,
            revision: 1,
            ..Default::default()
        })
        .collect();
    w.write_docker(b).unwrap();
    let mut second = batch(2);
    second.daemon_id = "other".into();
    w.write_docker(second).unwrap();
    w.flush().unwrap();
    let path = store.path().to_owned();
    store.close().unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    let other: i64 = conn
        .query_row(
            "SELECT id FROM docker_engines WHERE daemon_id='other'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let c: i64 = conn
        .query_row(
            "SELECT id FROM docker_entities WHERE external_id='c0'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(conn.execute("INSERT INTO docker_metadata(entity_id,engine_id,host_id,effective_ms,revision,name,image,lifecycle,last_seen_ms,tombstone,extras) VALUES(?1,?2,1,0,1,'','','',0,0,x'')",rusqlite::params![c,other]).is_err());
    for sql in [
        "SELECT payload FROM docker_10s WHERE entity_id=1 AND bucket_ms>=0 AND bucket_ms<100 ORDER BY bucket_ms,layout_id",
        "SELECT id FROM docker_events WHERE engine_id=1 AND time_ms>=0 AND time_ms<100 ORDER BY time_ms,id",
        "SELECT revision FROM docker_metadata WHERE entity_id=1 AND effective_ms<100 ORDER BY effective_ms DESC,revision DESC LIMIT 1",
        "SELECT id FROM docker_coverage WHERE engine_id=1 AND start_ms<100 AND (end_ms>0 OR end_ms IS NULL)",
    ] {
        let plan: Vec<String> = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map([], |r| r.get(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(plan.iter().any(|p| p.contains("SEARCH")), "{plan:?}");
        assert!(!plan.iter().any(|p| p.contains("SCAN docker_")), "{plan:?}");
    }
    assert_eq!(
        conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}
#[test]
fn logical_budget_rejects_every_class_without_advancing_cursor() {
    let dir = temp_dir("docker-cap");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    assert_eq!(allowance(150_000_000), 37_500_000);
    w.set_docker_history_budget(400);
    let ack = w.write_docker(batch(1)).unwrap();
    assert_eq!(ack.recv().unwrap().outcome, DockerOutcome::Rejected);
    assert_eq!(w.docker_storage_status().pending_bytes, 0);
    assert!(w.docker_storage_status().paused);
    w.set_docker_history_budget(150_000_000);
    let ack = w.write_docker(batch(2)).unwrap();
    w.flush().unwrap();
    assert_eq!(ack.recv().unwrap().outcome, DockerOutcome::Persisted);
    assert!(!w.docker_storage_status().paused);
}

#[cfg(feature = "write-stats")]
#[test]
fn shared_commit_failure_reports_host_and_docker_and_releases_reservations() {
    let dir = temp_dir("docker-failed-commit");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let ack = w.write_docker(batch(1)).unwrap();
    w.flush().unwrap();
    ack.recv().unwrap();
    w.set_fail_commits(true);
    let mut b = batch(2);
    b.buckets[0].bucket_ms = T0 + 10_000;
    b.start_ms = T0 + 10_000;
    b.end_ms = T0 + 20_000;
    let failed = w.write_docker(b).unwrap();
    w.write_bucket(bucket(
        host(1).id,
        Tier::M1,
        T0,
        &layout(&["cpu.total"]),
        2.,
    ))
    .unwrap();
    assert!(w.flush().is_err());
    assert_eq!(failed.recv().unwrap().outcome, DockerOutcome::Failed);
    assert_eq!(w.docker_storage_status().pending_writes, 0);
    w.set_fail_commits(false);
    w.flush().unwrap();
    let conn = raw(&store);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM gaps WHERE reason='write_failed'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM docker_coverage WHERE reason='write_failed'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    w.flush().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM gaps WHERE reason='write_failed'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}
#[test]
fn aggregate_population_segments_survive_rollup_and_cannot_cross_engines_or_mutate() {
    let dir = temp_dir("docker-populations");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let mut b = batch(1);
    b.buckets[0].entity = "".into();
    b.buckets[0].populations = vec![PopulationSegment {
        revision: 1,
        start_ms: T0,
        end_ms: T0 + 10_000,
    }];
    b.populations = vec![DockerPopulation {
        revision: 1,
        effective_ms: T0,
        members: vec!["c".into()],
    }];
    let ack = w.write_docker(b.clone()).unwrap();
    w.flush().unwrap();
    assert_eq!(ack.recv().unwrap().outcome, DockerOutcome::Persisted);
    b.revision = 2;
    b.populations[0].members.push("new".into());
    let bad = w.write_docker(b).unwrap();
    assert_eq!(bad.recv().unwrap().outcome, DockerOutcome::Failed);
    w.flush().unwrap();
    let mut reader = store.reader().unwrap();
    let snap = reader.docker_snapshot(host(1).id, "daemon").unwrap();
    let mut q = query(Tier::M15);
    q.entity = String::new();
    assert_eq!(
        snap.history(&q).unwrap().buckets[0].populations,
        vec![PopulationSegment {
            revision: 1,
            start_ms: T0,
            end_ms: T0 + 10_000
        }]
    );
}

/// Real SQLite admission churn at the unchanged production allowance. Run in release:
/// cargo test -p kelvo-store --release --test docker default_allowance_churn -- --ignored --nocapture
#[test]
#[ignore = "128-container churn across the 37.5 MB allowance; run release for evidence"]
fn default_allowance_churn() {
    let dir = temp_dir("docker-churn");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let started = std::time::Instant::now();
    let mut rejected = 0;
    let mut accepted = 0;
    let mut peak = 0;
    let mut offered = 0usize;
    for cycle in 0..300 {
        let t = T0 + cycle * 10_000;
        let mut b = batch(cycle + 1);
        b.start_ms = t;
        b.end_ms = t + 10_000;
        b.entities.clear();
        b.buckets.clear();
        for i in 0..128 {
            let id = format!("container-{i}");
            b.entities.push(DockerEntity {
                external_id: id.clone(),
                effective_ms: t,
                revision: cycle + 1,
                name: format!("service-{i}-revision-{cycle}"),
                image: "image:latest".into(),
                last_seen_ms: t,
                extras: vec![0; 1024],
                ..Default::default()
            });
            b.buckets.push(DockerBucket {
                entity: id.clone(),
                tier: Tier::S10,
                bucket_ms: t,
                revision: cycle + 1,
                metrics: vec![WeightedMetric {
                    key: "cpu_percent".into(),
                    min: 1.,
                    max: 2.,
                    sum: 15000.,
                    weight_ms: 10000,
                    expected_ms: 10000,
                }],
                populations: Vec::new(),
            });
            b.events.push(DockerEvent {
                entity: Some(id),
                time_ns: t * 1_000_000 + i,
                action: "start".into(),
                attributes: BTreeMap::new(),
            });
        }
        offered += b.logical_bytes().unwrap();
        let rx = w.write_docker(b).unwrap();
        w.flush().unwrap();
        match rx.recv().unwrap().outcome {
            DockerOutcome::Persisted => accepted += 1,
            DockerOutcome::Rejected => rejected += 1,
            other => panic!("unexpected {other:?}"),
        };
        let status = w.docker_storage_status();
        peak = peak.max(status.logical_bytes);
        assert!(status.logical_bytes <= 37_500_000);
        assert_eq!(status.pending_bytes, 0);
    }
    let before = store.size_on_disk().unwrap();
    w.prune(T0 + 300 * 10000, Retention::default()).unwrap();
    let after = store.size_on_disk().unwrap();
    let conn = raw(&store);
    let retained: Option<i64> = conn
        .query_row("SELECT min(bucket_ms) FROM docker_10s", [], |r| r.get(0))
        .unwrap();
    let foreign_keys: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(foreign_keys, 0);
    assert!(offered > 2 * 37_500_000);
    assert!(accepted > 0);
    assert!(retained.is_some_and(|t| t > T0));
    assert!(after <= before);
    eprintln!(
        "Docker churn: offered={offered} accepted={accepted} rejected={rejected} peak_logical={peak} allowance=37500000 file_before={before} file_after={after} retained_from={retained:?} elapsed={:?}",
        started.elapsed()
    );
}

#[test]
fn applied_batches_release_admission_before_the_300_second_commit() {
    let dir = temp_dir("docker-admission");
    let mut config = kelvo_store::StoreConfig::new(dir.path().join("h.sqlite"));
    config.commit_interval = std::time::Duration::from_secs(300);
    let store = kelvo_store::Store::open(config).unwrap();
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let mut receivers = Vec::new();
    for revision in 1..=400 {
        let mut b = batch(revision);
        // Advance the sampled clock through 299 seconds; the queue barrier below
        // waits for apply, never COMMIT, and avoids wall-clock sleeps.
        b.start_ms = T0 + revision * 299_000 / 400;
        b.end_ms = b.start_ms + 1;
        receivers.push(w.write_docker(b).unwrap());
        w.begin_session(host(1).id, T0).unwrap();
        assert_eq!(w.docker_storage_status().pending_writes, 0);
        assert_eq!(w.docker_storage_status().pending_bytes, 0);
        assert!(matches!(
            receivers.last().unwrap().try_recv(),
            Err(TryRecvError::Empty)
        ));
    }
    w.flush().unwrap();
    for rx in receivers {
        assert_eq!(rx.recv().unwrap().outcome, DockerOutcome::Persisted);
    }
}

#[test]
fn metadata_only_pressure_reclaims_and_resumes_without_waiting_for_prune() {
    let dir = temp_dir("docker-metadata-pressure");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    w.set_docker_history_budget(400_000);
    let mut admitted = 0;
    for i in 1..200 {
        let mut b = batch(i);
        b.buckets.clear();
        b.entities[0].external_id = format!("removed-{i}");
        b.entities[0].effective_ms = T0 + i;
        b.entities[0].tombstone = true;
        b.entities[0].extras = vec![0; 1024];
        let rx = w.write_docker(b).unwrap();
        w.flush().unwrap();
        if rx.recv().unwrap().outcome == DockerOutcome::Persisted {
            admitted += 1;
        }
        assert!(w.docker_storage_status().logical_bytes <= 100_000);
    }
    assert!(admitted > 150);
    let conn = raw(&store);
    let remaining: i64 = conn
        .query_row("SELECT count(*) FROM docker_metadata", [], |r| r.get(0))
        .unwrap();
    assert!(remaining < 100);
    assert!(!w.docker_storage_status().paused);
}

#[test]
fn daemon_identity_churn_is_accounted_bounded_and_preserves_generation() {
    let dir = temp_dir("docker-engine-churn");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    for n in 0..257 {
        let mut b = batch(n + 1);
        b.daemon_id = format!("daemon-{n}");
        b.entities.clear();
        b.buckets.clear();
        let rx = w.write_docker(b).unwrap();
        w.flush().unwrap();
        assert_eq!(
            rx.recv().unwrap().outcome,
            if n < 256 {
                DockerOutcome::Persisted
            } else {
                DockerOutcome::Rejected
            }
        );
    }
    let conn = raw(&store);
    let identities: i64 = conn
        .query_row("SELECT count(*) FROM docker_engines", [], |r| r.get(0))
        .unwrap();
    assert_eq!(identities, 256);
    let accounted: i64 = conn
        .query_row(
            "SELECT coalesce(sum(logical_bytes),0) FROM docker_accounting",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(w.docker_storage_status().logical_bytes > accounted as u64 + 256 * 256);
    w.reset_docker(host(1).id, 5).unwrap();
    let mut b = batch(999);
    b.daemon_id = "daemon-0".into();
    let rx = w.write_docker(b.clone()).unwrap();
    w.flush().unwrap();
    assert_eq!(rx.recv().unwrap().outcome, DockerOutcome::Reset);
    b.generation = 5;
    let rx = w.write_docker(b).unwrap();
    w.flush().unwrap();
    assert_eq!(rx.recv().unwrap().outcome, DockerOutcome::Persisted);
}

#[test]
fn retained_catalog_reopens_latest_metadata_and_obeys_reset_and_retention() {
    let dir = temp_dir("docker-catalog");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    for (engine, id, name, revision) in [
        ("a", "same", "old", 1),
        ("a", "same", "renamed", 2),
        ("b", "same", "other", 3),
    ] {
        let mut b = batch(revision);
        b.daemon_id = engine.into();
        b.buckets.clear();
        b.entities[0].external_id = id.into();
        b.entities[0].name = name.into();
        b.entities[0].effective_ms = T0 + revision;
        let ack = w.write_docker(b).unwrap();
        w.flush().unwrap();
        assert_eq!(ack.recv().unwrap().outcome, DockerOutcome::Persisted);
    }
    drop(w);
    store.close().unwrap();
    let store = open(&dir, "h.sqlite");
    let mut reader = store.reader().unwrap();
    let page = reader.docker_catalog(host(1).id, None, "", 0, 100).unwrap();
    assert_eq!(page.engines.len(), 2);
    assert_eq!(page.total, 2);
    assert_eq!(
        page.entities
            .iter()
            .map(|e| e.metadata.name.as_str())
            .collect::<Vec<_>>(),
        ["other", "renamed"]
    );
    assert_eq!(
        reader
            .docker_catalog(host(1).id, Some("a"), "old", 0, 100)
            .unwrap()
            .total,
        0
    );
    let page = reader
        .docker_catalog(host(1).id, Some("a"), "NAME", 0, 100)
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.entities[0].engine_id, "a");
    assert_eq!(
        reader
            .docker_catalog(host(2).id, None, "", 0, 100)
            .unwrap()
            .total,
        0
    );
    assert!(reader.docker_catalog(host(1).id, None, "", 0, 101).is_err());
    // A concurrent snapshot stays coherent; a fresh request reflects the reset.
    let snapshot = reader.docker_snapshot(host(1).id, "").unwrap();
    store.writer().reset_docker(host(1).id, 10).unwrap();
    assert_eq!(snapshot.catalog(None, "", 0, 100).unwrap().total, 2);
    drop(snapshot);
    let empty = reader.docker_catalog(host(1).id, None, "", 0, 100).unwrap();
    assert_eq!(empty.total, 0);
    assert!(empty.engines.is_empty());
    let mut b = batch(4);
    b.generation = 10;
    b.daemon_id = "a".into();
    b.buckets.clear();
    let ack = store.writer().write_docker(b).unwrap();
    store.writer().flush().unwrap();
    assert_eq!(ack.recv().unwrap().outcome, DockerOutcome::Persisted);
    store
        .writer()
        .prune(T0 + 31 * DAY, Retention::default())
        .unwrap();
    let empty = reader.docker_catalog(host(1).id, None, "", 0, 100).unwrap();
    assert_eq!(empty.total, 0);
    assert!(empty.engines.is_empty());
}

#[test]
fn retained_catalog_overlay_filters_counts_and_pages_without_duplicate_identities() {
    let dir = temp_dir("docker-catalog-overlay");
    let store = open(&dir, "h.sqlite");
    let w = store.writer();
    w.upsert_host(host(1)).unwrap();
    let mut b = batch(1);
    b.buckets.clear();
    b.entities[0].name = "before".into();
    let mut middle = b.entities[0].clone();
    middle.external_id = "middle".into();
    middle.name = "Middle".into();
    b.entities.push(middle);
    let ack = w.write_docker(b).unwrap();
    w.flush().unwrap();
    assert_eq!(ack.recv().unwrap().outcome, DockerOutcome::Persisted);
    let overlay = vec![
        DockerCatalogEntity {
            engine_id: "daemon".into(),
            metadata: DockerEntity {
                external_id: "c".into(),
                name: "Zulu".into(),
                ..Default::default()
            },
        },
        DockerCatalogEntity {
            engine_id: "daemon".into(),
            metadata: DockerEntity {
                external_id: "new".into(),
                name: "Alpha".into(),
                ..Default::default()
            },
        },
    ];
    let engines = vec![
        DockerCatalogEngine {
            id: "daemon".into(),
            runtime: "OrbStack".into(),
        },
        DockerCatalogEngine {
            id: "ring-only-engine".into(),
            runtime: "Docker".into(),
        },
    ];
    let mut reader = store.reader().unwrap();
    let snap = reader.docker_snapshot(host(1).id, "daemon").unwrap();
    let page = snap
        .catalog_with_overlay(None, "", 2, 1, &overlay, &engines)
        .unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.entities.len(), 1);
    assert_eq!(page.entities[0].metadata.name, "Zulu");
    assert_eq!(page.engines.len(), 2);
    let middle = snap
        .catalog_with_overlay(None, "", 1, 1, &overlay, &engines)
        .unwrap();
    assert_eq!(middle.entities[0].metadata.name, "Middle");
    assert_eq!(
        snap.catalog_with_overlay(None, "BEFORE", 0, 100, &overlay, &engines)
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        snap.catalog_with_overlay(None, "ALPHA", 0, 100, &overlay, &engines)
            .unwrap()
            .total,
        1
    );
    let oversized = vec![DockerCatalogEntity {
        engine_id: "daemon".into(),
        metadata: DockerEntity {
            external_id: "huge".into(),
            extras: vec![0; MAX_QUERY_BYTES + 1],
            ..Default::default()
        },
    }];
    assert!(
        snap.catalog_with_overlay(None, "", 0, 100, &oversized, &[])
            .is_err()
    );
}

fn assert_open_interruption_recovers(reason: &str) {
    let dir = temp_dir("docker-switch-recovery");
    let store = open(&dir, "h.sqlite");
    let writer = store.writer();
    writer.upsert_host(host(1)).unwrap();
    let mut b = batch(1);
    b.buckets[0].entity.clear();
    b.buckets[0].populations.push(PopulationSegment {
        revision: 1,
        start_ms: T0 + 2000,
        end_ms: T0 + 10000,
    });
    b.populations.push(DockerPopulation {
        revision: 1,
        effective_ms: T0 + 2000,
        members: vec!["c".into()],
    });
    writer.write_docker(b).unwrap();
    writer.flush().unwrap();
    let conn = rusqlite::Connection::open(store.path()).unwrap();
    conn.execute("INSERT INTO docker_coverage(host_id,engine_id,start_ms,end_ms,reason,revision,sampled_ms,expected_ms) SELECT host_id,id,?1,NULL,?2,1,0,0 FROM docker_engines", rusqlite::params![T0+1000, reason]).unwrap();
    conn.execute("INSERT INTO docker_coverage(host_id,engine_id,start_ms,end_ms,reason,revision,sampled_ms,expected_ms) SELECT host_id,id,?1,NULL,?2,1,0,0 FROM docker_engines", rusqlite::params![T0+15000, reason]).unwrap();
    let mut reader = store.reader().unwrap();
    let snapshot = reader.docker_snapshot(host(1).id, "daemon").unwrap();
    let coverage = snapshot.event_coverage(None, T0, T0 + 20000).unwrap();
    assert_eq!(coverage[0].end_ms, Some(T0 + 2000));
    assert_eq!(
        coverage[1].end_ms, None,
        "no later observation leaves the interruption open"
    );
    drop(snapshot);
    conn.execute_batch("PRAGMA foreign_keys=ON; DELETE FROM docker_10s; DELETE FROM docker_1m;")
        .unwrap();
    let snapshot = reader.docker_snapshot(host(1).id, "daemon").unwrap();
    assert_eq!(
        snapshot.event_coverage(None, T0, T0 + 20000).unwrap()[0].end_ms,
        Some(T0 + 2000),
        "coarse tiers preserve the exact observation boundary"
    );
    drop(snapshot);
    writer.write_docker(batch(2)).unwrap();
    writer.flush().unwrap();
    let end: Option<i64> = conn
        .query_row(
            "SELECT end_ms FROM docker_coverage WHERE reason=?2 AND start_ms=?1",
            rusqlite::params![T0 + 1000, reason],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(end, Some(T0 + 2000));
    let mut stale = batch(3);
    stale.buckets.clear();
    stale.coverage.push(DockerCoverage {
        entity: None,
        metric: None,
        start_ms: T0 + 1000,
        end_ms: None,
        reason: reason.into(),
        revision: 3,
        sampled_ms: 0,
        expected_ms: 0,
    });
    writer.write_docker(stale).unwrap();
    writer.flush().unwrap();
    let end: Option<i64> = conn
        .query_row(
            "SELECT end_ms FROM docker_coverage WHERE reason=?2 AND start_ms=?1",
            rusqlite::params![T0 + 1000, reason],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        end,
        Some(T0 + 2000),
        "late retries must not reopen a closed interval"
    );
}

#[test]
fn open_engine_switch_recovers_from_observed_population_without_migration() {
    assert_open_interruption_recovers("engine_switch");
}

#[test]
fn open_shutdown_recovers_from_observed_population_without_migration() {
    assert_open_interruption_recovers("shutdown");
}

#[test]
fn v7_removes_only_legacy_engine_wide_save_loss_records() {
    let dir = temp_dir("docker-v7");
    let path = dir.path().join("h.sqlite");
    {
        let store = open(&dir, "h.sqlite");
        let w = store.writer();
        w.upsert_host(host(1)).unwrap();
        let mut b = batch(1);
        for entity in [None, Some("c".into())] {
            for reason in ["write_failed", "loss_scope_overflow", "sleep"] {
                b.coverage.push(DockerCoverage {
                    entity: entity.clone(),
                    metric: None,
                    start_ms: T0,
                    end_ms: Some(T0 + 10_000),
                    reason: reason.into(),
                    revision: 1,
                    sampled_ms: 0,
                    expected_ms: 10_000,
                });
            }
        }
        let rx = w.write_docker(b).unwrap();
        w.flush().unwrap();
        assert_eq!(rx.recv().unwrap().outcome, DockerOutcome::Persisted);
    }
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA user_version=6; UPDATE meta SET value=6 WHERE key='schema_version';",
    )
    .unwrap();
    drop(conn);
    let store = open(&dir, "h.sqlite");
    let conn = raw(&store);
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM docker_coverage WHERE entity_id IS NULL AND reason IN ('write_failed','loss_scope_overflow')"
        ),
        0
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM docker_coverage WHERE entity_id IS NOT NULL"
        ),
        3
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM docker_coverage WHERE reason='sleep'"
        ),
        2
    );
}

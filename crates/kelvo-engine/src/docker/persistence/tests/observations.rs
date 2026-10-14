use super::*;

#[test]
fn aggregate_running_selection_excludes_selected_stopped_containers() {
    let template = fixture(2000).containers.remove(0);
    let rows: Vec<_> = (0..14)
        .map(|i| {
            let mut row = template.clone();
            row.id = format!("c{i}");
            row.monitored = i < 9;
            row.lifecycle = if i < 9 {
                api::DockerLifecycle::Exited
            } else {
                api::DockerLifecycle::Running
            };
            row
        })
        .collect();
    let total = crate::docker::aggregate(&rows, None, true, 2000, 1000);
    assert_eq!(total.expected, 0);
    assert_eq!(total.running, 5);
    assert_eq!(total.monitored, 9);
    assert_eq!(total.total, 14);
    assert_eq!((total.cpu_measured, total.memory_measured), (0, 0));
    assert!(total.partial);
}

#[test]
fn detail_reconciles_state_metadata_and_metrics_by_evidence_clock() {
    let live = fixture(2000).containers.remove(0);
    let mut inspected = fixture(1000).containers.remove(0);
    inspected.explicit_limit_bytes = Some(4096.);
    inspected.ports = vec!["80/tcp".into()];
    inspected.lifecycle = api::DockerLifecycle::Exited;
    inspected.health = api::DockerHealth::Unhealthy;
    inspected.metrics.cpu_pct = Some(999.);
    assert_eq!(crate::docker::reconcile(live.clone(), &inspected), live);
    // Fresh inspection supplies state and metadata, while live metrics win.
    inspected.metadata_observed_ms = Some(3000);
    let merged = crate::docker::reconcile(live.clone(), &inspected);
    assert_eq!(merged.lifecycle, api::DockerLifecycle::Exited);
    assert_eq!(merged.health, api::DockerHealth::Unhealthy);
    assert_eq!(merged.explicit_limit_bytes, Some(4096.));
    assert_eq!(merged.ports, inspected.ports);
    assert_eq!(merged.metrics, live.metrics);
    // Metadata can be newer than the previous inspect but older than inventory.
    let mut inventory = live.clone();
    inventory.last_seen_ms = 4000;
    let merged = crate::docker::reconcile(inventory.clone(), &inspected);
    assert_eq!(merged.lifecycle, inventory.lifecycle);
    assert_eq!(merged.explicit_limit_bytes, Some(4096.));
    // Legacy rows without an inspect stamp accept newly stamped metadata.
    inventory.metadata_observed_ms = None;
    assert_eq!(
        crate::docker::reconcile(inventory.clone(), &inspected).explicit_limit_bytes,
        Some(4096.)
    );
    inspected.metadata_observed_ms = None;
    assert_eq!(
        crate::docker::reconcile(inventory.clone(), &inspected).explicit_limit_bytes,
        inventory.explicit_limit_bytes
    );
    // An uninspected inventory accepts available metadata even without a stamp.
    inventory.metadata_available = false;
    assert_eq!(
        crate::docker::reconcile(inventory, &inspected).explicit_limit_bytes,
        Some(4096.)
    );
    // A raced 404 is authoritative removal evidence, including cleared metrics.
    inspected.metadata_observed_ms = Some(3000);
    inspected.lifecycle = api::DockerLifecycle::Removed;
    inspected.observed_ms = None;
    inspected.interval_ms = None;
    inspected.metrics = api::DockerMetrics::default();
    assert_eq!(crate::docker::reconcile(live, &inspected), inspected);
}

#[test]
fn project_aggregates_exclude_stale_unmonitored_and_stopped_metrics() {
    let mut rows = fixture(30_000).containers;
    rows[0].project = Some("app".into());
    rows[0].metrics.cpu_pct = Some(200.);
    rows[0].metrics.working_set_bytes = Some(100.);
    for (id, monitored, lifecycle, observed) in [
        ("stale", true, api::DockerLifecycle::Running, 1000),
        ("unmonitored", false, api::DockerLifecycle::Running, 30_000),
        ("stopped", true, api::DockerLifecycle::Exited, 30_000),
    ] {
        let mut row = rows[0].clone();
        row.id = id.into();
        row.monitored = monitored;
        row.lifecycle = lifecycle;
        row.observed_ms = Some(observed);
        row.metrics.cpu_pct = Some(900.);
        row.metrics.working_set_bytes = Some(900.);
        rows.push(row);
    }
    let mut standalone = rows[0].clone();
    standalone.id = "standalone".into();
    standalone.project = None;
    rows.push(standalone);
    let projects = crate::docker::project_aggregates(&rows, Some(1024.), true, 30_000, 1000);
    assert_eq!(projects.len(), 2);
    let project = &projects
        .iter()
        .find(|p| p.project.as_deref() == Some("app"))
        .unwrap()
        .aggregate;
    assert_eq!(project.cpu_cores, Some(2.));
    assert_eq!(project.working_set_bytes, Some(100.));
    assert_eq!(
        (
            project.total,
            project.running,
            project.monitored,
            project.expected
        ),
        (4, 3, 3, 2)
    );
    assert_eq!((project.cpu_measured, project.memory_measured), (1, 1));
    assert!(project.partial);
    let standalone = &projects
        .iter()
        .find(|p| p.project.is_none())
        .unwrap()
        .aggregate;
    assert!(!standalone.partial);
    assert_eq!(standalone.capacity_bytes, Some(1024.));
    assert!(
        crate::docker::project_aggregates(&rows, None, false, 30_000, 1000)
            .iter()
            .all(|p| p.aggregate.partial)
    );
}

#[test]
fn population_changes_keep_prior_segments_and_exact_revisions() {
    let (_dir, store, r) = setup();
    let t = wall_ms().div_euclid(900_000) * 900_000;
    let mut s = fixture(t + 1000);
    r.record(&s);
    at(&mut s, t + 6000, 5000);
    s.containers[0].id = "other".into();
    r.record(&s);
    at(&mut s, t + 11000, 5000);
    r.record(&s);
    store.writer().flush().unwrap();
    r.status();
    let mut reader = store.reader().unwrap();
    let snap = reader.docker_snapshot(host().id, "daemon").unwrap();
    let page = snap
        .history(&db::DockerHistoryQuery {
            entity: String::new(),
            start_ms: t,
            end_ms: t + 60000,
            tier: Tier::S10,
            max_points: 100,
        })
        .unwrap();
    let populations: &Vec<db::PopulationSegment> = &page.buckets[0].populations;
    assert_eq!(populations.len(), 2);
    assert_ne!(populations[0].revision, populations[1].revision);
    assert!(populations[0].revision > u32::MAX as i64);
}

#[test]
fn review_clock_rollback_reseeds_unchanged_metadata_and_population() {
    let (_dir, store, r) = setup();
    let t = wall_ms().div_euclid(900000) * 900000;
    let mut s = fixture(t + 1000);
    r.record(&s);
    at(&mut s, t + 61000, 1000);
    s.containers[0].name = "future-name".into();
    s.containers[0].explicit_limit_bytes = Some(4096.);
    r.record(&s);
    store.writer().flush().unwrap();
    r.clock_step(t + 10000);
    at(&mut s, t + 11000, 1000);
    r.record(&s);
    at(&mut s, t + 21000, 10000);
    r.record(&s);
    r.shutdown_flush().unwrap();
    let mut reader = store.reader().unwrap();
    let snap = reader.docker_snapshot(host().id, "daemon").unwrap();
    let page = snap
        .history(&db::DockerHistoryQuery {
            entity: "c".into(),
            start_ms: t,
            end_ms: t + 120000,
            tier: Tier::S10,
            max_points: 100,
        })
        .unwrap();
    assert!(
        page.metadata
            .iter()
            .any(|m| m.name == "before" && m.effective_ms == t + 1000)
    );
    assert!(page.metadata.iter().any(|m| m.name == "future-name"
        && m.effective_ms == t + 11000
        && m.explicit_limit == Some(4096)));
    assert!(page.metadata.iter().all(|m| m.effective_ms < t + 61000));
    drop(snap);
    let fresh = DockerRecorder::new(host().id);
    fresh.attach(store.writer(), store.path().to_path_buf());
    let page = fresh.history(&query(t + 20000, t + 30000, 100)).unwrap();
    assert_eq!(page.points[0].explicit_limit_bytes, Some(4096.));
}

#[test]
fn metadata_receipt_refresh_does_not_write_duplicate_semantic_facts() {
    let recorder = DockerRecorder::new(host().id);
    let mut snapshot = fixture(wall_ms());
    recorder.record(&snapshot);
    let count = recorder.state.lock_ok().metadata_history.len();
    snapshot.containers[0].metadata_observed_ms = Some(wall_ms() + 1000);
    recorder.record(&snapshot);
    assert_eq!(recorder.state.lock_ok().metadata_history.len(), count);
    assert_eq!(
        recorder.detail("daemon", "c").unwrap().metadata_observed_ms,
        snapshot.containers[0].metadata_observed_ms
    );
}

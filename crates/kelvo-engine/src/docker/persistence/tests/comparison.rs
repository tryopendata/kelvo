use super::*;

#[test]
fn comparison_keeps_removed_identity_ranks_ties_nulls_and_bounds_series() {
    let (_dir, store, r) = setup();
    let mut snapshot = fixture(10_000);
    let template = snapshot.containers[0].clone();
    snapshot.containers = (0..8)
        .map(|n| {
            let mut container = template.clone();
            container.id = format!("c{n}");
            container.name = if n < 2 {
                "recreated-name".into()
            } else {
                format!("name-{n}")
            };
            container.project = (n == 7).then(|| "other".into());
            container.metrics.cpu_pct = (n < 7).then_some(if n < 2 { 100. } else { n as f64 });
            container.metrics.working_set_bytes = Some(1000.);
            container
        })
        .collect();
    r.record(&snapshot);
    store.writer().flush().unwrap();
    r.status();
    let mut removed = snapshot.containers[0].clone();
    removed.lifecycle = api::DockerLifecycle::Removed;
    removed.last_seen_ms = 20_000;
    removed.metadata_observed_ms = Some(20_000);
    r.removed(&removed, 1);
    store.writer().flush().unwrap();
    // A fresh recorder proves candidates and removed metadata came from SQLite.
    let r = DockerRecorder::new(host().id);
    r.attach(store.writer(), store.path().to_path_buf());
    let mut q = api::DockerComparisonRequest {
        engine_id: "daemon".into(),
        from_ms: 0,
        to_ms: 30_000,
        metric: api::DockerComparisonMetric::Cpu,
        project: None,
        selected: None,
        max_points: 360,
    };
    let page = r.comparison(&q).unwrap();
    assert_eq!(page.ranking.len(), 8);
    assert_eq!(page.series.len(), 5);
    assert_eq!(page.ranking[0].container.id, "c0");
    assert_eq!(page.ranking[1].container.id, "c1");
    assert_eq!(
        page.ranking[0].container.lifecycle,
        api::DockerLifecycle::Removed
    );
    assert_eq!(page.ranking[7].peak_cpu_pct, None);
    assert_eq!(page.ranking[0].avg_cpu_pct, Some(100.));
    assert_eq!(page.ranking[0].cpu_measured_ms, 1000);
    q.to_ms = 15_000;
    let historical = r.comparison(&q).unwrap();
    assert_eq!(
        historical.ranking[0].container.lifecycle,
        api::DockerLifecycle::Removed
    );
    assert_eq!(
        historical.ranking[0].last_lifecycle,
        Some(api::DockerLifecycle::Running)
    );
    q.to_ms = 30_000;
    q.selected = Some((0..6).map(|n| format!("c{n}")).collect());
    q.max_points = 1;
    let page = r.comparison(&q).unwrap();
    assert_eq!(page.series.len(), 6);
    assert!(page.series.iter().all(|s| s.points.len() <= 1));
    q.selected.as_mut().unwrap().push("c7".into());
    assert!(r.comparison(&q).is_err());
    q.selected = None;
    q.project = Some(String::new());
    assert_eq!(r.comparison(&q).unwrap().ranking.len(), 7);
    q.project = Some("other".into());
    assert_eq!(r.comparison(&q).unwrap().ranking.len(), 1);
    assert!(r.comparison(&q).unwrap().series.is_empty());
    q.metric = api::DockerComparisonMetric::Memory;
    assert_eq!(r.comparison(&q).unwrap().ranking[0].container.id, "c7");
    q.max_points = 361;
    assert!(r.comparison(&q).is_err());
}

#[test]
fn comparison_ranks_before_truncating_to_500_stored_identities() {
    let (_dir, store, r) = setup();
    let mut snapshot = fixture(10_000);
    let template = snapshot.containers[0].clone();
    snapshot.containers = (0..510)
        .map(|n| {
            let mut c = template.clone();
            c.id = format!("c{n:03}");
            c.metrics.cpu_pct = Some(n as f64);
            c.metrics.working_set_bytes = Some(n as f64 + 1.);
            c
        })
        .collect();
    for chunk in snapshot.containers.chunks(128) {
        let mut cycle = snapshot.clone();
        cycle.containers = chunk.to_vec();
        r.record(&cycle);
        store.writer().flush().unwrap();
        assert_eq!(r.status().status, "recording");
    }
    let r = DockerRecorder::new(host().id);
    r.attach(store.writer(), store.path().to_path_buf());
    let page = r
        .comparison(&api::DockerComparisonRequest {
            engine_id: "daemon".into(),
            from_ms: 0,
            to_ms: HOUR,
            metric: api::DockerComparisonMetric::Cpu,
            project: None,
            selected: None,
            max_points: 360,
        })
        .unwrap();
    assert_eq!(page.ranking.len(), 500);
    assert_eq!(page.ranking[0].container.id, "c509");
    assert_eq!(page.ranking[499].container.id, "c010");
    assert_eq!(page.series.len(), 5);
    for metric in [
        api::DockerComparisonMetric::Cpu,
        api::DockerComparisonMetric::Memory,
    ] {
        for to_ms in [20_000, HOUR] {
            let custom = r
                .comparison(&api::DockerComparisonRequest {
                    engine_id: "daemon".into(),
                    from_ms: 0,
                    to_ms,
                    metric,
                    project: None,
                    selected: Some(vec!["c000".into()]),
                    max_points: 360,
                })
                .unwrap();
            assert_eq!(custom.ranking.len(), 500);
            assert!(custom.ranking.iter().all(|r| r.container.id != "c000"));
            assert_eq!(custom.series.len(), 1);
            assert!(!custom.series[0].points.is_empty());
            let series = serde_json::to_value(&custom.series[0]).unwrap();
            assert_eq!(
                series["container"]["id"], "c000",
                "selected chart metadata must survive ranking truncation"
            );
            assert_eq!(series["container"]["name"], template.name);
        }
    }
}

#[test]
fn full_hour_comparison_shares_payloads_for_128_containers() {
    let r = DockerRecorder::new(host().id);
    let mut snapshot = fixture(10_000);
    let template = snapshot.containers[0].clone();
    snapshot.containers = (0..128)
        .map(|n| {
            let mut c = template.clone();
            c.id = format!("c{n:03}");
            c
        })
        .collect();
    r.record(&snapshot);
    {
        let mut s = r.state.lock_ok();
        let seeds: Vec<_> = s
            .entries
            .iter()
            .filter(|((_, id, _), _)| !id.is_empty())
            .map(|(key, e)| (key.clone(), (*e.bucket).clone()))
            .collect();
        for (key, seed) in seeds {
            for n in 1..360 {
                let mut bucket = seed.clone();
                bucket.bucket_ms = n * 10_000;
                let bytes = bucket_bytes(&bucket);
                s.bytes += bytes;
                s.entries.insert(
                    (key.0.clone(), key.1.clone(), bucket.bucket_ms),
                    Entry {
                        bucket: Arc::new(bucket),
                        bytes,
                        pins: 0,
                        durable: 0,
                    },
                );
            }
        }
        recount_aux(&mut s);
        assert!(s.bytes + s.aux_bytes < RING_BYTES);
    }
    let started = Instant::now();
    let page = r
        .comparison(&api::DockerComparisonRequest {
            engine_id: "daemon".into(),
            from_ms: 0,
            to_ms: HOUR,
            metric: api::DockerComparisonMetric::Cpu,
            project: None,
            selected: None,
            max_points: 360,
        })
        .unwrap();
    assert_eq!(page.ranking.len(), 128);
    assert_eq!(page.series.len(), 5);
    assert!(page.series.iter().all(|series| series.points.len() == 360));
    let s = r.state.lock_ok();
    assert_eq!(s.reader_bytes, 0);
    assert!(s.shared_read_buckets.is_empty());
    assert!(s.bytes + s.aux_bytes < RING_BYTES);
    eprintln!(
        "128-container hour comparison: {:?}, ring={} bytes",
        started.elapsed(),
        s.bytes
    );
}

#[test]
fn shared_reader_payload_charge_survives_ring_eviction() {
    let r = DockerRecorder::new(host().id);
    r.record(&fixture(10_000));
    let mut s = r.state.lock_ok();
    let key = ("daemon".into(), "c".into(), 0);
    let bucket = Arc::clone(&s.entries.get(&key).unwrap().bucket);
    let bytes = bucket_bytes(&bucket);
    s.shared_read_buckets.insert(
        Arc::as_ptr(&bucket) as usize,
        SharedReadBucket {
            key: key.clone(),
            bucket: Arc::downgrade(&bucket),
            bytes,
        },
    );
    recount_aux(&mut s);
    let before = s.bytes + s.aux_bytes;
    // This bucket is already durable, so eviction creates no retry metadata.
    s.entries.get_mut(&key).unwrap().durable = i64::MAX;
    evict_entry(&mut s, &key);
    recount_aux(&mut s);
    assert_eq!(s.bytes + s.aux_bytes, before);
    drop(bucket);
    recount_aux(&mut s);
    assert!(s.bytes + s.aux_bytes <= before - bytes);
    assert!(s.shared_read_buckets.is_empty());
}

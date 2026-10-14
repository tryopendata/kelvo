use super::*;

#[test]
fn history_points_publish_coverage_without_client_inference() {
    let mut bucket = db::DockerBucket {
        entity: "c".into(),
        tier: Tier::S10,
        bucket_ms: 0,
        revision: 1,
        metrics: vec![db::WeightedMetric {
            key: "cpu_percent".into(),
            min: 10.,
            max: 10.,
            sum: 100_000.,
            weight_ms: 10_000,
            expected_ms: 10_000,
        }],
        populations: vec![],
    };
    for weight in [10_000, 9999, 9800] {
        bucket.metrics[0].weight_ms = weight;
        assert!(point(&bucket).cpu_covered);
        assert!(!point(&bucket).memory_covered);
        assert_eq!(point(&bucket).peak.cpu_pct, Some(10.));
    }
    for weight in [9799, 0] {
        bucket.metrics[0].weight_ms = weight;
        assert!(!point(&bucket).cpu_covered);
    }
    assert_eq!(point(&bucket).peak.cpu_pct, None);
    bucket.metrics.clear();
    assert!(!point(&bucket).cpu_covered);
}

#[test]
fn nonaligned_hour_is_bounded_and_left_overlap_is_retained() {
    let r = DockerRecorder::new(host().id);
    let t = wall_ms().div_euclid(900_000) * 900_000;
    let mut s = fixture(t + 1000);
    for i in 0..=360 {
        at(&mut s, t + i * 10_000 + 1000, 10000);
        r.record(&s);
    }
    let page = r.history(&query(t + 1000, t + HOUR + 1000, 360)).unwrap();
    assert!(page.points.len() <= 360);
    assert!(page.points.first().unwrap().time_ms <= t + 1000);
    assert_eq!(
        page.points.get(1).unwrap().explicit_limit_bytes,
        Some(1024.)
    );
}

#[test]
fn coarse_database_span_is_not_overwritten_by_partial_ring() {
    let (_dir, store, r) = setup();
    let t = wall_ms().div_euclid(900_000) * 900_000;
    let mut s = fixture(t + 1000);
    r.record(&s);
    for i in 1..=6 {
        at(&mut s, t + i * 10000, 10000);
        r.record(&s);
    }
    store.writer().flush().unwrap();
    r.status();
    {
        let mut state = r.state.lock_ok();
        let removed: Vec<_> = state
            .entries
            .keys()
            .filter(|(_, _, time)| *time < t + 50000)
            .cloned()
            .collect();
        for key in removed {
            let e = state.entries.remove(&key).unwrap();
            state.bytes -= e.bytes;
        }
    }
    at(&mut s, t + 61000, 1000);
    s.containers[0].metrics.cpu_pct = Some(300.);
    r.record(&s);
    let page = r.history(&query(t, t + 3_600_000, 60)).unwrap();
    let first = page.points.first().unwrap();
    assert!(first.cpu_measured_ms >= 59000, "{first:?}");
    assert!(first.metrics.cpu_pct.unwrap() < 110.);
}

#[test]
fn same_nanosecond_ring_database_pagination_does_not_skip_or_repeat() {
    let (_dir, store, r) = setup();
    let t = wall_ms();
    let make = |id: &str| api::DockerEvent {
        id: id.into(),
        engine_id: "daemon".into(),
        container_id: Some(id.into()),
        time_ns: (t * 1_000_000 + 1).to_string(),
        time_ms: t,
        action: "oom".into(),
        attributes: BTreeMap::new(),
        name: None,
        exit_code: None,
    };
    for id in ["a", "b", "c"] {
        r.event(&make(id), 1);
    }
    store.writer().flush().unwrap();
    r.status();
    r.event(&make("d"), 1);
    let mut q = api::DockerEventsRequest {
        engine_id: "daemon".into(),
        container_id: None,
        from_ms: t,
        to_ms: t + 1,
        limit: 2,
        before_id: None,
    };
    let first = r.events(&q).unwrap();
    assert_eq!(first.events.len(), 2);
    q.before_id = first.next_id;
    let second = r.events(&q).unwrap();
    assert_eq!(second.events.len(), 2);
    let ids: BTreeSet<_> = first
        .events
        .iter()
        .chain(&second.events)
        .map(|e| &e.id)
        .collect();
    assert_eq!(ids.len(), 4);
}

#[test]
fn reset_invalidates_reader_pins_and_discards_late_generation() {
    let (_dir, store, r) = setup();
    let t = wall_ms();
    r.record(&fixture(t));
    let pin = {
        let mut s = r.state.lock_ok();
        s.reader_bytes += 100;
        ReadPin {
            state: &r.state,
            bytes: 100,
            generation: s.generation,
            reset_epoch: s.reset_epoch,
            started: Instant::now(),
        }
    };
    r.reset(2).unwrap();
    assert!(!pin.valid());
    assert_eq!(r.state.lock_ok().reader_bytes, 100);
    drop(pin);
    assert_eq!(r.state.lock_ok().reader_bytes, 0);
    r.record(&fixture(t + 1000));
    assert!(r.state.lock_ok().entries.is_empty());
    store.writer().flush().unwrap();
}

#[test]
fn review_short_history_falls_back_after_age_or_partial_fine_reclamation() {
    for age in [true, false] {
        let (_dir, store, r) = setup();
        let now = wall_ms().div_euclid(900000) * 900000;
        let t = if age { now - 2 * 86400000 } else { now };
        let mut s = fixture(t + 10000);
        for i in 1..=6 {
            at(&mut s, t + i * 10000, 10000);
            r.record(&s);
        }
        r.shutdown_flush().unwrap();
        if age {
            store
                .writer()
                .prune(now, kelvo_store::Retention::default())
                .unwrap();
        } else {
            let conn = rusqlite::Connection::open(store.path()).unwrap();
            conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
            conn.execute("DELETE FROM docker_10s WHERE bucket_ms=?1", [t + 20000])
                .unwrap();
        }
        let fresh = DockerRecorder::new(host().id);
        fresh.attach(store.writer(), store.path().to_path_buf());
        let page = fresh.history(&query(t, t + 3600000, 360)).unwrap();
        assert_eq!(page.points.len(), 1, "age={age}");
        assert_eq!(page.points[0].duration_ms, 60000);
        assert_eq!(page.points[0].cpu_measured_ms, 60000);
        assert_eq!(page.points[0].metrics.cpu_pct, Some(100.));
    }
}

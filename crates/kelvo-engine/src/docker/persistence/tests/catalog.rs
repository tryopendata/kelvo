use super::*;

#[test]
fn retained_catalog_merges_uncommitted_metadata_and_survives_restart_then_reset() {
    let (_dir, store, recorder) = setup();
    let mut snapshot = fixture(wall_ms());
    recorder.record(&snapshot);
    store.writer().flush().unwrap();
    snapshot.containers[0].name = "after".into();
    snapshot.containers[0].last_seen_ms += 1000;
    let mut second = snapshot.containers[0].clone();
    second.id = "second".into();
    second.name = "second".into();
    snapshot.containers.push(second);
    recorder.record(&snapshot);
    let request = api::DockerRetainedRequest {
        engine_id: Some("daemon".into()),
        search: String::new(),
        offset: 0,
        limit: 1,
    };
    let first = recorder.retained(&request).unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.containers[0].name, "after");
    assert_eq!(first.next_offset, Some(1));
    assert!(
        first.containers[0].metrics.cpu_pct.is_none(),
        "retained metadata must not masquerade as current metrics"
    );
    assert_eq!(first.engines[0].runtime, "OrbStack");
    let page2 = recorder
        .retained(&api::DockerRetainedRequest {
            offset: 1,
            ..request.clone()
        })
        .unwrap();
    assert_eq!(page2.containers[0].id, "second");
    assert_eq!(page2.next_offset, None);
    assert_eq!(
        recorder
            .retained(&api::DockerRetainedRequest {
                search: "before".into(),
                ..request.clone()
            })
            .unwrap()
            .total,
        0,
        "renamed ring metadata suppresses stale durable search results"
    );
    recorder.shutdown_flush().unwrap();
    let restarted = DockerRecorder::new(host().id);
    restarted.attach(store.writer(), store.path().to_path_buf());
    let durable = restarted.retained(&request).unwrap();
    assert_eq!(durable.total, 2);
    assert_eq!(durable.containers[0].name, "after");
    restarted.reset(2).unwrap();
    let reset = restarted.retained(&request).unwrap();
    assert!(reset.containers.is_empty());
    assert!(reset.engines.is_empty());
}

#[test]
fn live_only_retained_catalog_caps_all_engine_descriptors_even_when_filtered() {
    let recorder = DockerRecorder::new(host().id);
    for index in 0..256 {
        let mut snapshot = fixture(wall_ms());
        let id = format!("daemon-{index:03}");
        let engine = snapshot.engine.as_mut().unwrap();
        engine.id = id.clone();
        engine.daemon_id = Some(id.clone());
        snapshot.containers[0].engine_id = id;
        recorder.record(&snapshot);
    }
    let request = api::DockerRetainedRequest {
        engine_id: Some("daemon-000".into()),
        search: String::new(),
        offset: 0,
        limit: 1,
    };
    assert_eq!(recorder.retained(&request).unwrap().engines.len(), 256);
    let mut extra = fixture(wall_ms());
    extra.engine.as_mut().unwrap().id = "daemon-extra".into();
    extra.engine.as_mut().unwrap().daemon_id = Some("daemon-extra".into());
    extra.containers[0].engine_id = "daemon-extra".into();
    recorder.record(&extra);
    assert!(matches!(
        recorder.retained(&request),
        Err(DockerServiceError::ResourceLimit(_))
    ));
    assert_eq!(recorder.state.lock_ok().reader_bytes, 0);
}

#[test]
fn invalid_catalog_bounds_are_distinct_from_resource_exhaustion() {
    let (_dir, _store, recorder) = setup();
    let request = api::DockerRetainedRequest {
        engine_id: None,
        search: String::new(),
        offset: 0,
        limit: 0,
    };
    assert!(matches!(
        recorder.retained(&request),
        Err(DockerServiceError::InvalidRequest(_))
    ));
}

#[test]
fn malformed_retained_metadata_keeps_its_source_and_releases_reader_bytes() {
    use std::error::Error;
    let (_dir, _store, recorder) = setup();
    recorder.record(&fixture(wall_ms()));
    recorder.detach();
    for metadata in recorder.state.lock_ok().facts.values_mut() {
        metadata.extras = b"{broken metadata".to_vec();
    }
    let request = api::DockerRetainedRequest {
        engine_id: Some("daemon".into()),
        search: String::new(),
        offset: 0,
        limit: 1,
    };
    let error = recorder.retained(&request).unwrap_err();
    assert!(matches!(error, DockerServiceError::Metadata { .. }));
    assert!(
        error
            .source()
            .unwrap()
            .downcast_ref::<serde_json::Error>()
            .is_some()
    );
    assert_eq!(
        error.to_string(),
        "Retained container metadata is unavailable"
    );
    assert_eq!(recorder.state.lock_ok().reader_bytes, 0);
}

#[test]
fn failed_catalog_reader_retains_store_error() {
    let (dir, _store, recorder) = setup();
    recorder.attached.lock_ok().as_mut().unwrap().path =
        dir.path().join("missing").join("history.sqlite");
    let request = api::DockerRetainedRequest {
        engine_id: None,
        search: String::new(),
        offset: 0,
        limit: 1,
    };
    assert!(matches!(
        recorder.retained(&request),
        Err(DockerServiceError::Store(_))
    ));
    assert_eq!(recorder.state.lock_ok().reader_bytes, 0);
}

//! Mirror integration uses real SQLite and needs neither a webview nor SSH.
use super::*;
use kelvo_store::CursorRead;
use std::path::Path;

const TS: i64 = 1_788_220_800_000;

fn mirror(dir: &Path, id: u128) -> Mirror {
    let mut record = local_record();
    record.id = HostId(Uuid::from_u128(id));
    record.is_local = false;
    let history = Arc::new(kelvo_engine::HistoryStore::open(dir));
    assert!(history.register_host(&record).is_some());
    Mirror::new(history, record.id, 100)
}
fn keys() -> Vec<kelvo_schema::SeriesKey> {
    vec![kelvo_schema::SeriesKey::parse("cpu.total").unwrap()]
}
fn layout() -> WireLayout {
    WireLayout {
        layout_no: 0,
        series: keys(),
    }
}
fn row() -> WireBucket {
    WireBucket {
        seq: 1,
        bucket_ts: TS,
        layout_no: 0,
        stats: vec![1., 3., 2.],
    }
}
fn page(epoch: Uuid) -> SyncPage {
    SyncPage {
        tier: Tier::S10,
        epoch,
        layouts: vec![layout()],
        rows: vec![row()],
        gaps: vec![],
        events: vec![],
        last_seq: 1,
        more: false,
    }
}

#[test]
fn mirrors_have_independent_databases_and_closing_one_preserves_another() {
    let dir = tempfile::tempdir().unwrap();
    let a_dir = dir.path().join("a");
    let b_dir = dir.path().join("b");
    std::fs::create_dir_all(&a_dir).unwrap();
    std::fs::create_dir_all(&b_dir).unwrap();
    let a = mirror(&a_dir, 1);
    let b = mirror(&b_dir, 2);
    let epoch = Uuid::new_v4();
    a.cursor(Tier::S10, epoch, SyncKinds::ALL).unwrap();
    b.cursor(Tier::S10, epoch, SyncKinds::ALL).unwrap();
    a.ingest(page(epoch), SyncKinds::ALL).unwrap();
    assert!(
        b.cursor(Tier::S10, epoch, SyncKinds::ALL)
            .unwrap()
            .is_none()
    );
    a.history.close();
    b.ingest(page(epoch), SyncKinds::ALL).unwrap();
    assert_eq!(
        b.cursor(Tier::S10, epoch, SyncKinds::ALL)
            .unwrap()
            .unwrap()
            .seq,
        1
    );
    // Explicitly closing the old store permits reopening its exact path.
    let reopened = mirror(&a_dir, 1);
    assert_eq!(
        reopened
            .cursor(Tier::S10, epoch, SyncKinds::ALL)
            .unwrap()
            .unwrap()
            .seq,
        1
    );
}

#[test]
fn epoch_change_discards_recent_tail_and_rejects_obsolete_pages() {
    let dir = tempfile::tempdir().unwrap();
    let m = mirror(dir.path(), 2);
    let first = Uuid::new_v4();
    let next = Uuid::new_v4();
    m.cursor(Tier::S10, first, SyncKinds::ALL).unwrap();
    m.recent(first, Tier::S10, TS + 10_000, vec![layout()], vec![row()])
        .unwrap();
    assert_eq!(m.recent.lock_ok().len(), 1);
    m.cursor(Tier::S10, next, SyncKinds::ALL).unwrap();
    assert!(m.recent.lock_ok().is_empty());
    assert!(m.ingest(page(first), SyncKinds::ALL).is_err());
    assert!(
        m.recent(first, Tier::S10, TS + 10_000, vec![layout()], vec![row()])
            .is_err()
    );
    assert!(m.cursor(Tier::S10, next, SyncKinds::ALL).unwrap().is_none());
}

#[test]
fn durable_page_replaces_transient_tail_and_malformed_tail_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let m = mirror(dir.path(), 2);
    let epoch = Uuid::new_v4();
    m.cursor(Tier::S10, epoch, SyncKinds::ALL).unwrap();
    m.recent(epoch, Tier::S10, TS + 10_000, vec![layout()], vec![row()])
        .unwrap();
    let mut invalid = row();
    invalid.stats.pop();
    assert!(
        m.recent(epoch, Tier::S10, TS + 10_000, vec![layout()], vec![invalid])
            .is_err()
    );
    assert_eq!(m.recent.lock_ok().len(), 1);
    m.ingest(page(epoch), SyncKinds::ALL).unwrap();
    assert!(m.recent.lock_ok().is_empty());
    let read = m
        .history
        .read(|r| r.read_after(m.host, Tier::S10, None, 2000, SyncKinds::ALL))
        .unwrap();
    let CursorRead::Page(p) = read else {
        panic!("expected durable page")
    };
    assert_eq!(p.rows.len(), 1);
}

#[test]
fn durable_coverage_matches_series_and_late_recent_cannot_replace_it() {
    let dir = tempfile::tempdir().unwrap();
    let m = mirror(dir.path(), 2);
    let epoch = Uuid::new_v4();
    m.cursor(Tier::S10, epoch, SyncKinds::ALL).unwrap();
    let second = WireLayout {
        layout_no: 1,
        series: vec![kelvo_schema::SeriesKey::parse("mem.used").unwrap()],
    };
    let mut other = row();
    other.layout_no = 1;
    m.recent(
        epoch,
        Tier::S10,
        TS + 10_000,
        vec![layout(), second.clone()],
        vec![row(), other.clone()],
    )
    .unwrap();
    m.ingest(page(epoch), SyncKinds::ALL).unwrap();
    assert_eq!(
        m.recent.lock_ok().len(),
        1,
        "other layout is not yet durable"
    );
    assert_eq!(
        m.recent.lock_ok()[0].series.as_ref(),
        second.series.as_slice()
    );
    m.recent(
        epoch,
        Tier::S10,
        TS + 10_000,
        vec![layout(), second],
        vec![row(), other],
    )
    .unwrap();
    assert_eq!(
        m.recent.lock_ok().len(),
        1,
        "late transient CPU cannot override committed CPU"
    );
}

#[test]
fn truncation_resets_cursor_and_persists_only_uncovered_gap() {
    let dir = tempfile::tempdir().unwrap();
    let m = mirror(dir.path(), 2);
    let epoch = Uuid::new_v4();
    m.cursor(Tier::M1, epoch, SyncKinds::ALL).unwrap();
    let mut p = page(epoch);
    p.tier = Tier::M1;
    m.ingest(p, SyncKinds::ALL).unwrap();
    m.truncated(Tier::M1, epoch, TS + 3_600_000).unwrap();
    assert!(m.cursor(Tier::M1, epoch, SyncKinds::ALL).unwrap().is_none());
    let gaps = m
        .history
        .read(|r| r.gaps(m.host, TS, TS + 3_600_000))
        .unwrap();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].reason, kelvo_schema::GapReason::Truncated);
    assert_eq!(gaps[0].start_ms, TS + 60_000);
    assert_eq!(gaps[0].end_ms, Some(TS + 3_600_000));
    assert!(
        m.truncated(Tier::M1, Uuid::new_v4(), TS + 4_000_000)
            .is_err()
    );
}

#[test]
fn unmet_fixed_overhead_budget_is_visible_in_history_health() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = mirror(dir.path(), 2);
    // Exercise the defensive path even though UI validation rejects zero budgets.
    m.budget = 0;
    assert!(m.prune(TS).is_err());
    assert!(!m.history.health().cap_met);
}

#[test]
fn per_page_budget_check_reports_an_unmet_cap_before_catchup_completes() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = mirror(dir.path(), 2);
    assert!(m.enforce_budget(TS).is_ok());
    m.budget = 0;
    assert!(m.enforce_budget(TS).is_err());
    assert!(!m.history.health().cap_met);
}

fn local_record() -> kelvo_schema::HostRecord {
    kelvo_schema::HostRecord {
        id: kelvo_schema::HostId(uuid::Uuid::from_u128(7)),
        is_local: true,
        display_name: "Mac".into(),
        info: kelvo_schema::HostInfo {
            os: kelvo_schema::OsKind::MacOs,
            os_version: "27.0".into(),
            model: None,
            chip: None,
            chip_known: true,
            cpu_topology: Vec::new(),
            mem_total_bytes: 0,
            boot_time_ms: 0,
            gpu_dvfs_mhz: Vec::new(),
            kernel: None,
            boot_mounts: Vec::new(),
        },
    }
}

#[test]
fn mirror_failures_keep_actionable_categories() {
    let dir = tempfile::tempdir().unwrap();
    let mut mirror = mirror(dir.path(), 2);
    let epoch = Uuid::new_v4();
    mirror.cursor(Tier::S10, epoch, SyncKinds::ALL).unwrap();
    assert!(matches!(
        mirror.ingest(page(Uuid::new_v4()), SyncKinds::ALL),
        Err(MirrorError::EpochChanged { .. })
    ));
    let mut malformed = row();
    malformed.stats.clear();
    assert!(matches!(
        mirror.recent(
            epoch,
            Tier::S10,
            TS + 10_000,
            vec![layout()],
            vec![malformed]
        ),
        Err(MirrorError::InvalidInput(_))
    ));
    assert!(matches!(
        mirror.recent(epoch, Tier::S10, TS, vec![], vec![row(); 2001]),
        Err(MirrorError::ResourceLimit(_))
    ));
    let mut oversized = page(epoch);
    oversized.rows = vec![row(); 2001];
    assert!(matches!(
        mirror.ingest(oversized, SyncKinds::ALL),
        Err(MirrorError::ResourceLimit(_))
    ));
    mirror.budget = 0;
    assert!(matches!(
        mirror.enforce_budget(TS),
        Err(MirrorError::ResourceLimit(_))
    ));
    mirror.history.close();
    assert!(matches!(
        mirror.cursor(Tier::S10, epoch, SyncKinds::ALL),
        Err(MirrorError::Unavailable(_))
    ));
}

#[test]
fn durable_write_failure_retains_sqlite_source_and_does_not_acknowledge_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let mirror = mirror(dir.path(), 2);
    let epoch = Uuid::new_v4();
    mirror.cursor(Tier::S10, epoch, SyncKinds::ALL).unwrap();
    let connection =
        kelvo_store::rusqlite::Connection::open(dir.path().join(kelvo_engine::HISTORY_DB_FILE))
            .unwrap();
    connection.execute_batch("CREATE TRIGGER reject_durable BEFORE INSERT ON tier_10s BEGIN SELECT RAISE(ABORT, 'simulated disk failure'); END;").unwrap();
    let error = mirror.ingest(page(epoch), SyncKinds::ALL).unwrap_err();
    assert!(std::error::Error::source(&error).is_some());
    assert!(matches!(
        error,
        MirrorError::Store(kelvo_engine::HistoryStoreError::Store(
            kelvo_store::StoreError::Sqlite(_)
        ))
    ));
    assert!(
        mirror
            .cursor(Tier::S10, epoch, SyncKinds::ALL)
            .unwrap()
            .is_none()
    );
    mirror
        .recent(epoch, Tier::S10, TS + 10_000, vec![layout()], vec![row()])
        .unwrap();
    assert_eq!(
        mirror.recent_rows(TS, TS + 10_000).len(),
        1,
        "failed durable writes cannot displace recent data"
    );
}

/// A remote layout with 40 `cgroup.cpu` series (an older or uncapped remote), plus
/// `cpu.total`, each series' stats being its position.
fn over_cap_layout() -> (WireLayout, WireBucket) {
    let mut series = vec![kelvo_schema::SeriesKey::parse("cpu.total").unwrap()];
    series.extend((0..40).map(|i| {
        kelvo_schema::SeriesKey::parse(&format!("cgroup.cpu{{kind=docker,name=c{i:02}}}")).unwrap()
    }));
    let stats = (0..series.len()).flat_map(|i| [i as f32; 3]).collect();
    let layout = WireLayout {
        layout_no: 0,
        series,
    };
    let row = WireBucket { stats, ..row() };
    (layout, row)
}

/// The mirror keeps the first 32 series of a capped metric in layout order, with their
/// own stats, as a local engine would (D-116).
#[test]
fn durable_ingest_caps_series_per_metric() {
    let dir = tempfile::tempdir().unwrap();
    let m = mirror(dir.path(), 2);
    let epoch = Uuid::new_v4();
    m.cursor(Tier::S10, epoch, SyncKinds::ALL).unwrap();
    let (layout, row) = over_cap_layout();
    let mut p = page(epoch);
    p.layouts = vec![layout.clone()];
    p.rows = vec![row];
    m.ingest(p, SyncKinds::ALL).unwrap();
    let read = m
        .history
        .read(|r| r.read_after(m.host, Tier::S10, None, 2000, SyncKinds::ALL))
        .unwrap();
    let CursorRead::Page(p) = read else {
        panic!("expected durable page")
    };
    let stored = &p.layouts[0].series;
    assert_eq!(stored.len(), 33);
    assert_eq!(stored.as_slice(), &layout.series[..33]);
    // Each kept series still has its own stats: series i carries i.
    let row = &p.rows[0];
    for (i, k) in stored.iter().enumerate() {
        let at = layout.series.iter().position(|s| s == k).unwrap();
        assert_eq!(row.stats[i * 3..i * 3 + 3], [at as f32; 3], "{k}");
    }
}

#[test]
fn recent_snapshot_caps_series_per_metric() {
    let dir = tempfile::tempdir().unwrap();
    let m = mirror(dir.path(), 2);
    let epoch = Uuid::new_v4();
    m.cursor(Tier::S10, epoch, SyncKinds::ALL).unwrap();
    let (layout, row) = over_cap_layout();
    m.recent(
        epoch,
        Tier::S10,
        TS + 10_000,
        vec![layout.clone()],
        vec![row],
    )
    .unwrap();
    let recent = m.recent.lock_ok();
    assert_eq!(recent[0].series.as_ref(), &layout.series[..33]);
    assert_eq!(recent[0].stats.len(), 33 * 3);
    assert_eq!(recent[0].stats[32 * 3], 32.0);
}

/// A row that does not match its layout is not trimmed into a valid-looking one.
#[test]
fn capping_leaves_a_malformed_row_for_validation_to_reject() {
    let dir = tempfile::tempdir().unwrap();
    let m = mirror(dir.path(), 2);
    let epoch = Uuid::new_v4();
    m.cursor(Tier::S10, epoch, SyncKinds::ALL).unwrap();
    let (layout, mut row) = over_cap_layout();
    row.stats.truncate(33 * 3);
    assert!(
        m.recent(epoch, Tier::S10, TS + 10_000, vec![layout], vec![row])
            .is_err()
    );
}

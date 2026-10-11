//! Shell mirror integration independent of a running webview or SSH server.
use super::*;
use kelvo_store::CursorRead;
use std::path::Path;

const TS: i64 = 1_788_220_800_000;

fn mirror(dir: &Path, id: u128) -> Mirror {
    let mut record = crate::platform::local_host_record(HostId(Uuid::from_u128(id)));
    record.is_local = false;
    let history = Arc::new(History::open(dir));
    assert!(history.register_host(&record).is_some());
    Mirror {
        history,
        host: record.id,
        budget: 100,
        recent: Mutex::default(),
        epoch: Mutex::default(),
    }
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
fn invalid_host_configuration_is_rejected_before_ssh_or_storage() {
    let valid = RemoteHostConfig {
        id: String::new(),
        name: "Other Mac".into(),
        lan: Some("build-mac".into()),
        tailscale: None,
        enabled: true,
        mirror_budget_mb: 100,
    };
    assert!(validate(&valid).is_ok());
    for alias in ["-oProxyCommand=bad", "mac --verbose", "mac\nwhoami"] {
        let mut c = valid.clone();
        c.lan = Some(alias.into());
        assert!(validate(&c).is_err());
    }
    let mut c = valid.clone();
    c.mirror_budget_mb = 0;
    assert!(validate(&c).is_err());
    c = valid;
    c.lan = None;
    assert!(validate(&c).is_err());
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

#[tokio::test]
async fn cancellation_drops_inflight_setup_before_it_can_register_a_host() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Cleanup(Arc<AtomicBool>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let (tx, mut rx) = tokio::sync::watch::channel(false);
    let (started, ready) = tokio::sync::oneshot::channel();
    let observed = dropped.clone();
    let task = tokio::spawn(async move {
        setup_step(&mut rx, async move {
            let _cleanup = Cleanup(observed);
            let _ = started.send(());
            std::future::pending::<()>().await;
            Ok::<(), CommandError>(())
        })
        .await
    });
    ready.await.unwrap();
    tx.send(true).unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .expect("setup cancellation must settle promptly")
            .unwrap()
            .is_err()
    );
    assert!(dropped.load(Ordering::SeqCst));
}

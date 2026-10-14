//! Recorder scenarios grouped by invariant; the SQLite fixture permit is shared.
mod catalog;
mod comparison;
mod history;
mod observations;
mod pressure;
mod recovery;

use super::*;
use kelvo_schema::{HostInfo, HostRecord, OsKind};
use kelvo_store::{Store, StoreConfig};
fn host() -> HostRecord {
    HostRecord {
        id: HostId(uuid::Uuid::from_u128(1)),
        is_local: true,
        display_name: "test".into(),
        info: HostInfo {
            os: OsKind::MacOs,
            os_version: "test".into(),
            model: None,
            chip: None,
            chip_known: false,
            cpu_topology: Vec::new(),
            mem_total_bytes: 1,
            boot_time_ms: 0,
            gpu_dvfs_mhz: Vec::new(),
            boot_mounts: Vec::new(),
        },
    }
}
// The product admits four history readers per process, including readers of
// different files. Keep independent database fixtures from consuming one
// another's permits when the Rust test harness runs them concurrently.
static DATABASE_TESTS: Mutex<()> = Mutex::new(());
struct DatabaseFixture {
    dir: tempfile::TempDir,
    _readers: std::sync::MutexGuard<'static, ()>,
}
impl DatabaseFixture {
    fn path(&self) -> &std::path::Path {
        self.dir.path()
    }
}
fn setup() -> (DatabaseFixture, Store, DockerRecorder) {
    let dir = DatabaseFixture {
        _readers: DATABASE_TESTS.lock_ok(),
        dir: tempfile::tempdir().unwrap(),
    };
    let mut cfg = StoreConfig::new(dir.path().join("h.sqlite"));
    cfg.commit_interval = Duration::from_secs(300);
    let store = Store::open(cfg).unwrap();
    store.writer().upsert_host(host()).unwrap();
    let recorder = DockerRecorder::new(host().id);
    recorder.attach(store.writer(), store.path().to_path_buf());
    (dir, store, recorder)
}
fn fixture(time: i64) -> api::DockerSnapshot {
    let handle =
        super::super::DockerHandle::new(host().id, api::DockerSettings::default(), PathBuf::new());
    let mut s = handle.snapshot();
    s.generation = 1;
    s.status = api::DockerStatus::Connected;
    s.observed_ms = time;
    s.interval_ms = 1000;
    s.engine = Some(api::DockerEngine {
        id: "daemon".into(),
        daemon_id: Some("daemon".into()),
        runtime: "OrbStack".into(),
        context: None,
        socket: "/tmp/docker.sock".into(),
        reachable: true,
        preferred: true,
        error: None,
        api_version: None,
        capacity_bytes: Some(1024.),
        capacity_observed_ms: Some(time),
    });
    s.containers = vec![api::DockerContainer {
        auto_remove: Some(false),
        engine_id: "daemon".into(),
        metadata_available: true,
        metadata_observed_ms: Some(time),
        id: "c".into(),
        name: "before".into(),
        image: "image".into(),
        project: None,
        service: None,
        lifecycle: api::DockerLifecycle::Running,
        health: api::DockerHealth::NoCheck,
        restart_count: Some(0),
        observed_restarts: 0,
        repeated_restarts: false,
        observed_restart_window_ms: 300000,
        oom_killed: false,
        explicit_limit_bytes: Some(1024.),
        cpu_quota: None,
        restart_policy: None,
        ports: Vec::new(),
        mounts: Vec::new(),
        created: None,
        last_seen_ms: time,
        observed_ms: Some(time),
        interval_ms: Some(1000),
        monitored: true,
        metrics: api::DockerMetrics {
            cpu_pct: Some(100.),
            working_set_bytes: Some(128.),
            ..Default::default()
        },
    }];
    s.aggregate = api::DockerAggregate {
        cpu_cores: Some(1.),
        working_set_bytes: Some(128.),
        expected: 1,
        cpu_measured: 1,
        memory_measured: 1,
        running: 1,
        total: 1,
        monitored: 1,
        inventory_complete: true,
        ..Default::default()
    };
    s
}

fn at(s: &mut api::DockerSnapshot, time: i64, interval: u32) {
    s.observed_ms = time;
    s.interval_ms = interval;
    for c in &mut s.containers {
        c.observed_ms = Some(time);
        c.last_seen_ms = time;
        c.interval_ms = Some(interval);
    }
}
fn query(start: i64, end: i64, max_points: u32) -> api::DockerHistoryRequest {
    api::DockerHistoryRequest {
        engine_id: "daemon".into(),
        container_id: Some("c".into()),
        from_ms: start,
        to_ms: end,
        max_points,
    }
}

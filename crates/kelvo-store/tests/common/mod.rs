//! Helpers shared by the store's integration tests.

#![allow(dead_code, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use kelvo_schema::{ClusterInfo, CoreKind, HostId, HostInfo, HostRecord, OsKind, SeriesKey, Tier};
use kelvo_store::{BucketRow, Store, StoreConfig};
use uuid::Uuid;

/// 2026-09-01T00:00:00Z: a minute- and 10 s-aligned base for test timestamps.
pub const T0: i64 = 1_788_220_800_000;
pub const MIN: i64 = 60_000;
pub const HOUR: i64 = 60 * MIN;
pub const DAY: i64 = 24 * HOUR;

/// A fresh, empty directory under the system temp dir, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "kelvo-store-it-{name}-{}-{}",
            std::process::id(),
            Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A store whose writer only commits on flush, so tests decide when rows land.
pub fn open(dir: &TempDir, name: &str) -> Store {
    let mut cfg = StoreConfig::new(dir.file(name));
    cfg.commit_interval = Duration::from_secs(3600);
    Store::open(cfg).unwrap()
}

pub fn host(n: u128) -> HostRecord {
    HostRecord {
        id: HostId(Uuid::from_u128(n)),
        is_local: n == 1,
        display_name: format!("host {n}"),
        info: HostInfo {
            os: OsKind::MacOs,
            os_version: "27.0".into(),
            model: Some("Mac16,6".into()),
            chip: Some("Apple M4 Pro".into()),
            chip_known: true,
            cpu_topology: vec![ClusterInfo {
                name: "P0".into(),
                kind: CoreKind::Performance,
                cores: vec!["P0".into()],
                dvfs_mhz: vec![1260, 4512],
            }],
            mem_total_bytes: 24 << 30,
            boot_time_ms: T0 - DAY,
            gpu_dvfs_mhz: Vec::new(),
            boot_mounts: Vec::new(),
        },
    }
}

pub fn key(s: &str) -> SeriesKey {
    SeriesKey::parse(s).unwrap()
}

pub fn layout(keys: &[&str]) -> Arc<[SeriesKey]> {
    keys.iter().map(|k| key(k)).collect::<Vec<_>>().into()
}

/// A bucket whose every series has `(v - 1, v + 1, v)`.
pub fn bucket(host: HostId, tier: Tier, ts: i64, series: &Arc<[SeriesKey]>, v: f32) -> BucketRow {
    BucketRow {
        host,
        tier,
        bucket_ts: ts,
        series: Arc::clone(series),
        stats: series.iter().flat_map(|_| [v - 1.0, v + 1.0, v]).collect(),
    }
}

/// Raw read-only access for asserting on table contents.
pub fn raw(store: &Store) -> rusqlite::Connection {
    rusqlite::Connection::open_with_flags(store.path(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap()
}

/// Every row of `table` as debug strings, ordered by rowid: a cheap "identical rows"
/// comparison.
pub fn dump(conn: &rusqlite::Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
        .unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| {
        let mut s = String::new();
        for i in 0..n {
            s.push_str(&format!("{:?}|", r.get_ref(i)?));
        }
        Ok(s)
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

pub fn count(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

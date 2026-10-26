//! Storage setup and persisted-write evidence for the dump benchmark.
use anyhow::{Context, Result, bail};
use kelvo_schema::HostRecord;
use kelvo_store::{Store, StoreConfig};
use std::time::Duration;

pub(super) fn open(path: Option<&str>, commit_ms: Option<u64>) -> Result<Option<Store>> {
    path.map(|path| {
        let mut cfg = StoreConfig::new(path);
        if let Some(ms) = commit_ms {
            cfg.commit_interval = Duration::from_millis(ms);
        }
        Store::open(cfg).context("opening the store")
    })
    .transpose()
}

pub(super) fn register(
    store: Option<&Store>,
    record: &HostRecord,
    measure: bool,
) -> Result<Option<i64>> {
    let Some(store) = store else {
        return Ok(None);
    };
    store
        .writer()
        .upsert_host(record.clone())
        .context("registering dump host")?;
    if measure {
        Ok(Some(persisted_history_rows(store.path())?))
    } else {
        Ok(None)
    }
}

pub(super) fn verify(store: &Store, before: i64) -> Result<()> {
    store
        .writer()
        .flush()
        .context("flushing measured history")?;
    require_persisted_history(store.path(), before)
}

// Checked outside the CPU interval, after a writer flush. A successful collector-only
// run cannot stand in for a benchmark that promises to include the history writer.
fn persisted_history_rows(path: impl AsRef<std::path::Path>) -> Result<i64> {
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .context("opening measured history")?;
    Ok(db.query_row(
        "SELECT (SELECT count(*) FROM tier_10s) + (SELECT count(*) FROM tier_1m)",
        [],
        |row| row.get(0),
    )?)
}
fn require_persisted_history(path: impl AsRef<std::path::Path>, before: i64) -> Result<()> {
    let after = persisted_history_rows(path)?;
    if after <= before {
        bail!(
            "--perf --db persisted no new history rows; writer evidence is missing (run long enough to close a ten-second bucket)"
        );
    }
    eprintln!("[perf] persisted {} new history rows", after - before);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelvo_schema::HostId;

    #[test]
    fn perf_db_requires_new_committed_history_not_an_empty_or_reused_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("perf.sqlite");
        let path = path.to_str().unwrap();
        let store = open(Some(path), None).unwrap().unwrap();
        let record = kelvo_engine::host_record(HostId(uuid::Uuid::nil()));
        let writer = store.writer();
        assert_eq!(register(Some(&store), &record, true).unwrap(), Some(0));
        assert!(require_persisted_history(path, 0).is_err());
        writer
            .write_bucket(kelvo_store::BucketRow {
                host: record.id,
                tier: kelvo_schema::Tier::S10,
                bucket_ts: 10_000,
                series: vec![kelvo_schema::SeriesKey::parse("cpu.total").unwrap()].into(),
                stats: vec![1.0, 3.0, 2.0],
            })
            .unwrap();
        verify(&store, 0).unwrap();
        let before = persisted_history_rows(path).unwrap();
        assert_eq!(before, 1);
        assert!(require_persisted_history(path, before).is_err());
        store.close().unwrap();
    }
}

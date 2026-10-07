//! Opening the database: pragmas, the DDL from architecture.md, migrations through
//! `PRAGMA user_version`, and the `meta` rows.

use std::path::Path;
use std::time::Duration;

use kelvo_schema::Tier;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use uuid::Uuid;

use crate::error::{Result, StoreError};

/// The schema version this build writes. Bump it with a new step in [`MIGRATIONS`].
pub const SCHEMA_VERSION: i64 = 4;

/// Page size of new databases. 16 KiB packs wide M1 blobs (250 series = 3,000 B) five to
/// a page instead of one; measured in D-057.
pub(crate) const PAGE_SIZE: i64 = 16_384;
/// WAL is truncated back to this size after checkpoints.
const JOURNAL_SIZE_LIMIT: i64 = 8 * 1024 * 1024;
/// Auto-checkpoint at about 4 MiB of WAL, what SQLite's 1,000-page default means at
/// 4 KiB pages, so the larger page size does not let the WAL grow four times as big.
const WAL_AUTOCHECKPOINT_PAGES: i64 = 4 * 1024 * 1024 / PAGE_SIZE;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// `pruned.tier` key of the roll-down mark (D-076): the highest `seq` of the M1 rows rolled
/// into M15 and the cutoff they were rolled at. History before the cutoff is in
/// `tier_15m`. A receiver that did not negotiate M15 treats it as pruning of its M1
/// cursor; for one that did, nothing was lost.
pub(crate) const M1_ROLLED: &str = "m1_rolled";

/// `meta` key: S10 has holes before this time (ms epoch) because low-disk mode stopped
/// writing it; `i64::MAX` while paused. Absent when S10 was never paused (D-057).
pub(crate) const S10_HOLE_UNTIL: &str = "s10_hole_until";

/// Version 1: architecture.md's DDL plus what the store needs beyond it (D-041):
/// `seq` indexes on gaps and events for cursor reads, the controller's `cursors` table,
/// and `pruned` marks that tell a cursor read it fell behind pruning.
const V1: &str = "
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value BLOB NOT NULL
);

CREATE TABLE hosts (
  id         INTEGER PRIMARY KEY,
  uuid       TEXT NOT NULL UNIQUE,
  is_local   INTEGER NOT NULL,
  name       TEXT NOT NULL,
  info       BLOB NOT NULL,
  created_ms INTEGER NOT NULL
);

CREATE TABLE series (
  id        INTEGER PRIMARY KEY,
  host_id   INTEGER NOT NULL REFERENCES hosts(id),
  metric_id TEXT NOT NULL,
  labels    TEXT NOT NULL,
  UNIQUE (host_id, metric_id, labels)
);

CREATE TABLE layouts (
  id         INTEGER PRIMARY KEY,
  host_id    INTEGER NOT NULL REFERENCES hosts(id),
  series_ids BLOB NOT NULL,
  hash       BLOB NOT NULL,
  created_ms INTEGER NOT NULL,
  UNIQUE (host_id, hash)
);

CREATE TABLE tier_10s (
  host_id   INTEGER NOT NULL REFERENCES hosts(id),
  bucket_ts INTEGER NOT NULL,
  layout_id INTEGER NOT NULL REFERENCES layouts(id),
  seq       INTEGER NOT NULL,
  blob      BLOB NOT NULL
);
CREATE UNIQUE INDEX tier_10s_key ON tier_10s (host_id, bucket_ts, layout_id);
CREATE INDEX tier_10s_seq ON tier_10s (host_id, seq);

CREATE TABLE tier_1m (
  host_id   INTEGER NOT NULL REFERENCES hosts(id),
  bucket_ts INTEGER NOT NULL,
  layout_id INTEGER NOT NULL REFERENCES layouts(id),
  seq       INTEGER NOT NULL,
  blob      BLOB NOT NULL
);
CREATE UNIQUE INDEX tier_1m_key ON tier_1m (host_id, bucket_ts, layout_id);
CREATE INDEX tier_1m_seq ON tier_1m (host_id, seq);

CREATE TABLE gaps (
  id        INTEGER PRIMARY KEY,
  host_id   INTEGER NOT NULL REFERENCES hosts(id),
  start_ts  INTEGER NOT NULL,
  end_ts    INTEGER,
  module    TEXT,
  reason    TEXT NOT NULL,
  seq       INTEGER NOT NULL
);
CREATE INDEX gaps_range ON gaps (host_id, start_ts);
CREATE INDEX gaps_seq ON gaps (host_id, seq);

CREATE TABLE proc_names (
  id      INTEGER PRIMARY KEY,
  host_id INTEGER NOT NULL REFERENCES hosts(id),
  name    TEXT NOT NULL,
  UNIQUE (host_id, name)
);

CREATE TABLE proc_snap (
  host_id INTEGER NOT NULL REFERENCES hosts(id),
  ts      INTEGER NOT NULL,
  seq     INTEGER NOT NULL,
  blob    BLOB NOT NULL
);
CREATE UNIQUE INDEX proc_snap_key ON proc_snap (host_id, ts);

CREATE TABLE proc_top_1m (
  host_id   INTEGER NOT NULL REFERENCES hosts(id),
  bucket_ts INTEGER NOT NULL,
  seq       INTEGER NOT NULL,
  blob      BLOB NOT NULL
);
CREATE UNIQUE INDEX proc_top_1m_key ON proc_top_1m (host_id, bucket_ts);

CREATE TABLE events (
  id      INTEGER PRIMARY KEY,
  host_id INTEGER NOT NULL REFERENCES hosts(id),
  ts      INTEGER NOT NULL,
  kind    TEXT NOT NULL,
  payload BLOB NOT NULL,
  seq     INTEGER NOT NULL
);
CREATE INDEX events_range ON events (host_id, ts);
CREATE INDEX events_seq ON events (host_id, seq);

-- Controller side: where sync of (host, tier) left off in the remote database `epoch`.
CREATE TABLE cursors (
  host_id INTEGER NOT NULL REFERENCES hosts(id),
  tier    TEXT NOT NULL,
  epoch   TEXT NOT NULL,
  seq     INTEGER NOT NULL,
  PRIMARY KEY (host_id, tier)
);

-- Exporter side: the highest seq pruning removed per (host, tier), and the cutoff it
-- used. A cursor below `seq` gets `Truncated { earliest_ts_ms: ts }`.
CREATE TABLE pruned (
  host_id INTEGER NOT NULL REFERENCES hosts(id),
  tier    TEXT NOT NULL,
  seq     INTEGER NOT NULL,
  ts      INTEGER NOT NULL,
  PRIMARY KEY (host_id, tier)
);
";

/// Version 2 (D-064):
/// - At most one local host. `is_local` is the controller's own fact about a host and
///   never comes from a peer; identity recovery reads the one row this index allows.
///   A database that somehow has several keeps the newest.
/// - Cursors remember the sync row kinds they covered (`SyncKinds::to_stored`). A cursor
///   from before this version covered nothing, so its tier resyncs once.
const V2: &str = "
UPDATE hosts SET is_local = 0
 WHERE is_local = 1 AND id <> (SELECT max(id) FROM hosts WHERE is_local = 1);
CREATE UNIQUE INDEX hosts_one_local ON hosts (is_local) WHERE is_local = 1;
ALTER TABLE cursors ADD COLUMN kinds TEXT NOT NULL DEFAULT '';
";

/// Version 3: the 15-minute tier (D-076). M1 keeps 7 days; pruning rolls older minutes
/// into `tier_15m` (min of mins, max of maxes, mean of the minutes' averages, one row per
/// 15-minute bucket and layout) and older `proc_top_1m` rows into `proc_top_15m`. Same
/// shapes, keys and `seq` semantics as their minute tables. Nothing is copied here: the
/// first prune after the upgrade rolls existing minutes past the window down.
const V3: &str = "
CREATE TABLE tier_15m (
  host_id   INTEGER NOT NULL REFERENCES hosts(id),
  bucket_ts INTEGER NOT NULL,
  layout_id INTEGER NOT NULL REFERENCES layouts(id),
  seq       INTEGER NOT NULL,
  blob      BLOB NOT NULL
);
CREATE UNIQUE INDEX tier_15m_key ON tier_15m (host_id, bucket_ts, layout_id);
CREATE INDEX tier_15m_seq ON tier_15m (host_id, seq);

CREATE TABLE proc_top_15m (
  host_id   INTEGER NOT NULL REFERENCES hosts(id),
  bucket_ts INTEGER NOT NULL,
  seq       INTEGER NOT NULL,
  blob      BLOB NOT NULL
);
CREATE UNIQUE INDEX proc_top_15m_key ON proc_top_15m (host_id, bucket_ts);
";

/// Version 4: per-app network bytes (D-089). One row per bucket: a header with the
/// bucket's measured span and interface totals, then up to 20 apps plus "other apps"
/// (`blob::pack_net`). `proc_net_10s` keeps 72 hours; `proc_net_1m` is written live beside
/// it, recomputed from the minute's 10 s rows, and keeps the M1 window; pruning rolls
/// older minutes into `proc_net_15m`. Same key and `seq` semantics as the process tables;
/// not synced.
const V4: &str = "
CREATE TABLE proc_net_10s (
  host_id   INTEGER NOT NULL REFERENCES hosts(id),
  bucket_ts INTEGER NOT NULL,
  seq       INTEGER NOT NULL,
  blob      BLOB NOT NULL
);
CREATE UNIQUE INDEX proc_net_10s_key ON proc_net_10s (host_id, bucket_ts);

CREATE TABLE proc_net_1m (
  host_id   INTEGER NOT NULL REFERENCES hosts(id),
  bucket_ts INTEGER NOT NULL,
  seq       INTEGER NOT NULL,
  blob      BLOB NOT NULL
);
CREATE UNIQUE INDEX proc_net_1m_key ON proc_net_1m (host_id, bucket_ts);

CREATE TABLE proc_net_15m (
  host_id   INTEGER NOT NULL REFERENCES hosts(id),
  bucket_ts INTEGER NOT NULL,
  seq       INTEGER NOT NULL,
  blob      BLOB NOT NULL
);
CREATE UNIQUE INDEX proc_net_15m_key ON proc_net_15m (host_id, bucket_ts);
";

/// `MIGRATIONS[i]` takes the schema from version `i` to `i + 1`.
const MIGRATIONS: &[&str] = &[V1, V2, V3, V4];

/// Opens or creates the database for writing and brings its schema up to date. Returns
/// the connection and the `db_instance_uuid`.
pub(crate) fn open_writer(path: &Path, now_ms: i64) -> Result<(Connection, Uuid)> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::TooNew {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    if version == 0 {
        // Must precede the first table: SQLite only honours these on an empty database.
        conn.execute_batch(&format!(
            "PRAGMA page_size = {PAGE_SIZE}; PRAGMA auto_vacuum = INCREMENTAL"
        ))?;
    }
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(StoreError::Corrupt(format!(
            "journal_mode is {mode}, expected wal"
        )));
    }
    conn.query_row(
        &format!("PRAGMA journal_size_limit = {JOURNAL_SIZE_LIMIT}"),
        [],
        |_| Ok(()),
    )?;
    conn.query_row(
        &format!("PRAGMA wal_autocheckpoint = {WAL_AUTOCHECKPOINT_PAGES}"),
        [],
        |_| Ok(()),
    )?;
    conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")?;

    migrate(&conn, version, now_ms)?;
    // A run that ended while S10 was paused never resumed it: the hole ends now, and
    // this run starts unpaused.
    conn.execute(
        "UPDATE meta SET value = ?1 WHERE key = ?2 AND value = ?3",
        params![now_ms, S10_HOLE_UNTIL, i64::MAX],
    )?;
    let epoch = read_epoch(&conn)?;
    Ok((conn, epoch))
}

/// Opens a read-only connection. WAL lets it read while the writer writes.
pub(crate) fn open_reader(path: &Path) -> Result<(Connection, Uuid)> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    let epoch = read_epoch(&conn)?;
    Ok((conn, epoch))
}

fn migrate(conn: &Connection, from: i64, now_ms: i64) -> Result<()> {
    for (i, step) in MIGRATIONS.iter().enumerate().skip(from as usize) {
        let to = i as i64 + 1;
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(step)?;
        if to == 1 {
            let epoch = Uuid::new_v4().hyphenated().to_string();
            tx.execute(
                "INSERT INTO meta (key, value) VALUES
                   ('db_instance_uuid', ?1), ('next_seq', 1), ('created_at_ms', ?2),
                   ('schema_version', ?3)",
                params![epoch, now_ms, to],
            )?;
        } else {
            tx.execute(
                "UPDATE meta SET value = ?1 WHERE key = 'schema_version'",
                [to],
            )?;
        }
        tx.execute_batch(&format!("PRAGMA user_version = {to}"))?;
        tx.commit()?;
    }
    Ok(())
}

fn read_epoch(conn: &Connection) -> Result<Uuid> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'db_instance_uuid'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let text = text.ok_or_else(|| StoreError::Corrupt("meta has no db_instance_uuid".into()))?;
    Uuid::parse_str(&text).map_err(|e| StoreError::Corrupt(format!("db_instance_uuid: {e}")))
}

/// End of the span in which S10 has holes from low-disk mode, if any.
pub(crate) fn read_s10_hole_until(conn: &Connection) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [S10_HOLE_UNTIL],
            |r| r.get(0),
        )
        .optional()?)
}

pub(crate) fn read_next_seq(conn: &Connection) -> Result<i64> {
    let seq: Option<i64> = conn
        .query_row("SELECT value FROM meta WHERE key = 'next_seq'", [], |r| {
            r.get(0)
        })
        .optional()?;
    seq.ok_or_else(|| StoreError::Corrupt("meta has no next_seq".into()))
}

/// The tier table for a persisted tier.
pub(crate) fn tier_table(tier: Tier) -> Result<&'static str> {
    match tier {
        Tier::S10 => Ok("tier_10s"),
        Tier::M1 => Ok("tier_1m"),
        Tier::M15 => Ok("tier_15m"),
        Tier::Live1s | Tier::Unknown => Err(StoreError::NotPersisted(tier)),
    }
}

/// The per-app network table of a persisted tier.
pub(crate) fn net_table(tier: Tier) -> Result<&'static str> {
    match tier {
        Tier::S10 => Ok("proc_net_10s"),
        Tier::M1 => Ok("proc_net_1m"),
        Tier::M15 => Ok("proc_net_15m"),
        Tier::Live1s | Tier::Unknown => Err(StoreError::NotPersisted(tier)),
    }
}

/// Bucket width of a persisted tier.
pub(crate) fn tier_width(tier: Tier) -> Result<i64> {
    tier_table(tier)?;
    tier.bucket_ms().ok_or(StoreError::NotPersisted(tier))
}

/// The tables a read of `tier` takes buckets from, each with the weight of its rows. An
/// M15 read also takes the minutes still in `tier_1m`: a range that starts before the M1
/// window usually ends inside it, and those minutes fall into the same 15-minute slots
/// that the roll-down will fold them into (D-076). Each row weighs its width in minutes,
/// so a slot holding both kinds is not tilted toward whichever has more rows.
pub(crate) fn bucket_sources(tier: Tier) -> Result<&'static [(&'static str, u32)]> {
    match tier {
        Tier::S10 => Ok(&[("tier_10s", 1)]),
        Tier::M1 => Ok(&[("tier_1m", 1)]),
        Tier::M15 => Ok(&[("tier_15m", 15), ("tier_1m", 1)]),
        Tier::Live1s | Tier::Unknown => Err(StoreError::NotPersisted(tier)),
    }
}

/// `SELECT bucket_ts, layout_id, blob, weight` over every [`bucket_sources`] table of
/// `tier`, for `host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3`, as one `UNION ALL`.
pub(crate) fn bucket_rows_sql(tier: Tier) -> Result<String> {
    Ok(bucket_sources(tier)?
        .iter()
        .map(|(table, weight)| {
            format!(
                "SELECT bucket_ts, layout_id, blob, {weight} FROM {table}
                   WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3"
            )
        })
        .collect::<Vec<_>>()
        .join("\n UNION ALL\n "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_gets_pragmas_schema_and_meta() {
        let dir = crate::test_dir("db-fresh");
        let path = dir.join("h.sqlite");
        let (conn, epoch) = open_writer(&path, 42).unwrap();
        let get = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(get("PRAGMA auto_vacuum"), 2, "INCREMENTAL");
        assert_eq!(get("PRAGMA page_size"), PAGE_SIZE);
        assert_eq!(get("PRAGMA wal_autocheckpoint"), WAL_AUTOCHECKPOINT_PAGES);
        assert_eq!(get("PRAGMA synchronous"), 1, "NORMAL");
        assert_eq!(get("PRAGMA journal_size_limit"), JOURNAL_SIZE_LIMIT);
        assert_eq!(get("PRAGMA user_version"), SCHEMA_VERSION);
        assert_eq!(read_next_seq(&conn).unwrap(), 1);
        assert_eq!(
            get("SELECT value FROM meta WHERE key = 'created_at_ms'"),
            42
        );
        assert_eq!(
            get("SELECT value FROM meta WHERE key = 'schema_version'"),
            SCHEMA_VERSION
        );
        drop(conn);

        // Reopening keeps the epoch: it is created once, with the database.
        let (_, again) = open_writer(&path, 99).unwrap();
        assert_eq!(again, epoch);
        let (_, read) = open_reader(&path).unwrap();
        assert_eq!(read, epoch);
    }

    #[test]
    fn newer_schema_is_refused() {
        let dir = crate::test_dir("db-newer");
        let path = dir.join("h.sqlite");
        drop(open_writer(&path, 0).unwrap());
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA user_version = 99").unwrap();
        drop(conn);
        assert!(matches!(
            open_writer(&path, 0),
            Err(StoreError::TooNew { found: 99, .. })
        ));
    }

    #[test]
    fn v1_databases_keep_their_newest_local_host() {
        let dir = crate::test_dir("db-v1-locals");
        let path = dir.join("h.sqlite");
        drop(open_writer(&path, 0).unwrap());
        // Turn it back into a v1 file that two install identities both marked local.
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "DROP TABLE tier_15m;
             DROP TABLE proc_top_15m;
             DROP TABLE proc_net_10s;
             DROP TABLE proc_net_1m;
             DROP TABLE proc_net_15m;
             DROP INDEX hosts_one_local;
             ALTER TABLE cursors DROP COLUMN kinds;
             PRAGMA user_version = 1;
             INSERT INTO hosts (uuid, is_local, name, info, created_ms) VALUES
               ('old', 1, 'h', x'', 0), ('remote', 0, 'h', x'', 0), ('new', 1, 'h', x'', 0);",
        )
        .unwrap();
        drop(conn);
        let (conn, _) = open_writer(&path, 0).unwrap();
        let local: Vec<String> = conn
            .prepare("SELECT uuid FROM hosts WHERE is_local = 1")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(local, ["new"]);
        let kinds: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('cursors') WHERE name = 'kinds'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kinds, 1);
    }

    /// D-076: a version 2 file gains the 15-minute tables and keeps its rows; the minutes
    /// are rolled down by the next prune, not by the migration.
    #[test]
    fn v2_databases_gain_the_15_minute_tables() {
        let dir = crate::test_dir("db-v2-m15");
        let path = dir.join("h.sqlite");
        drop(open_writer(&path, 0).unwrap());
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "DROP TABLE tier_15m;
             DROP TABLE proc_top_15m;
             DROP TABLE proc_net_10s;
             DROP TABLE proc_net_1m;
             DROP TABLE proc_net_15m;
             PRAGMA user_version = 2;
             UPDATE meta SET value = 2 WHERE key = 'schema_version';
             INSERT INTO hosts (uuid, is_local, name, info, created_ms) VALUES ('h', 1, 'h', x'', 0);
             INSERT INTO layouts (host_id, series_ids, hash, created_ms) VALUES (1, x'', x'00', 0);
             INSERT INTO tier_1m (host_id, bucket_ts, layout_id, seq, blob) VALUES (1, 60000, 1, 1, x'');",
        )
        .unwrap();
        drop(conn);

        let (conn, _) = open_writer(&path, 0).unwrap();
        let get = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(get("PRAGMA user_version"), SCHEMA_VERSION);
        assert_eq!(
            get("SELECT value FROM meta WHERE key = 'schema_version'"),
            SCHEMA_VERSION
        );
        assert_eq!(get("SELECT count(*) FROM tier_1m"), 1, "rows are untouched");
        for index in ["tier_15m_key", "tier_15m_seq", "proc_top_15m_key"] {
            assert_eq!(
                get(&format!(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = '{index}'"
                )),
                1,
                "{index}"
            );
        }
        // Same shape and key as the minute table.
        conn.execute(
            "INSERT INTO tier_15m (host_id, bucket_ts, layout_id, seq, blob) VALUES (1, 0, 1, 2, x'')",
            [],
        )
        .unwrap();
        let dup = conn.execute(
            "INSERT INTO tier_15m (host_id, bucket_ts, layout_id, seq, blob) VALUES (1, 0, 1, 3, x'')",
            [],
        );
        assert!(dup.is_err(), "unique on (host, bucket_ts, layout)");
    }

    /// D-089: a version 3 file gains the per-app network tables and keeps its process rows
    /// and names.
    #[test]
    fn v3_databases_gain_the_network_tables() {
        let dir = crate::test_dir("db-v3-net");
        let path = dir.join("h.sqlite");
        drop(open_writer(&path, 0).unwrap());
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "DROP TABLE proc_net_10s;
             DROP TABLE proc_net_1m;
             DROP TABLE proc_net_15m;
             PRAGMA user_version = 3;
             UPDATE meta SET value = 3 WHERE key = 'schema_version';
             INSERT INTO hosts (uuid, is_local, name, info, created_ms) VALUES ('h', 1, 'h', x'', 0);
             INSERT INTO proc_names (host_id, name) VALUES (1, 'Safari');
             INSERT INTO proc_snap (host_id, ts, seq, blob) VALUES (1, 10000, 1, x'01');
             INSERT INTO proc_top_1m (host_id, bucket_ts, seq, blob) VALUES (1, 60000, 2, x'02');
             INSERT INTO proc_top_15m (host_id, bucket_ts, seq, blob) VALUES (1, 0, 3, x'03');",
        )
        .unwrap();
        drop(conn);

        let (conn, _) = open_writer(&path, 0).unwrap();
        let get = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(get("PRAGMA user_version"), 4);
        assert_eq!(
            get("SELECT value FROM meta WHERE key = 'schema_version'"),
            4
        );
        for table in ["proc_snap", "proc_top_1m", "proc_top_15m", "proc_names"] {
            assert_eq!(
                get(&format!("SELECT count(*) FROM {table}")),
                1,
                "{table} untouched"
            );
        }
        let name: String = conn
            .query_row("SELECT name FROM proc_names WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "Safari");
        for tier in Tier::PERSISTED {
            let table = net_table(tier).unwrap();
            conn.execute(
                &format!(
                    "INSERT INTO {table} (host_id, bucket_ts, seq, blob) VALUES (1, 0, 9, x'')"
                ),
                [],
            )
            .unwrap();
            let dup = conn.execute(
                &format!(
                    "INSERT INTO {table} (host_id, bucket_ts, seq, blob) VALUES (1, 0, 10, x'')"
                ),
                [],
            );
            assert!(dup.is_err(), "{table}: unique on (host, bucket_ts)");
        }
    }

    #[test]
    fn at_most_one_local_host() {
        let dir = crate::test_dir("db-one-local");
        let (conn, _) = open_writer(&dir.join("h.sqlite"), 0).unwrap();
        let insert = |uuid: &str, local: bool| {
            conn.execute(
                "INSERT INTO hosts (uuid, is_local, name, info, created_ms) VALUES (?1, ?2, 'h', x'', 0)",
                params![uuid, local],
            )
        };
        insert("a", true).unwrap();
        insert("b", false).unwrap();
        insert("c", false).unwrap();
        let err = insert("d", true).unwrap_err();
        assert!(err.to_string().contains("UNIQUE"), "{err}");
    }

    #[test]
    fn gaps_module_is_nullable_and_reason_free_text() {
        let dir = crate::test_dir("db-gaps");
        let (conn, _) = open_writer(&dir.join("h.sqlite"), 0).unwrap();
        conn.execute(
            "INSERT INTO hosts (uuid, is_local, name, info, created_ms) VALUES ('h', 1, 'h', x'', 0)",
            [],
        )
        .unwrap();
        for (module, reason) in [
            (None, "paused"),
            (Some("cpu"), "module_disabled"),
            (None, "app_not_running"),
        ] {
            conn.execute(
                "INSERT INTO gaps (host_id, start_ts, end_ts, module, reason, seq)
                 VALUES (1, 0, NULL, ?1, ?2, 1)",
                params![module, reason],
            )
            .unwrap();
        }
        let n: i64 = conn
            .query_row("SELECT count(*) FROM gaps WHERE module IS NULL", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 2);
    }
}

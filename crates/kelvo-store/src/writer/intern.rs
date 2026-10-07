//! Interning: host rows, series, layouts and process names, each cached per host.

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use kelvo_schema::{HostId, HostRecord, SeriesKey};
use rusqlite::{OptionalExtension, Params, params};

use super::{State, now_ms, to_u32};
use crate::blob::{self, OTHER_APPS};
use crate::error::{Result, StoreError};

/// New `proc_names` rows network buckets may create per host and hour of bucket time.
/// Past it, an app whose name is not interned yet is counted in "other apps", so a
/// process that rewrites its argv on every launch cannot grow the table without bound.
/// Names already interned (by a process snapshot or an earlier bucket) are not limited.
/// The count lives in the writer, so it starts over when the app restarts.
pub const NET_NEW_NAMES_PER_HOUR: u32 = 64;
const HOUR_MS: i64 = 3_600_000;

/// An intern table: the queries that find and add the row of one host's key.
struct Intern {
    /// What a row names, for errors.
    what: &'static str,
    select: &'static str,
    insert: &'static str,
    /// Id 0 is [`OTHER_APPS`] in network rows, so a row with it is corrupt.
    reserves_zero: bool,
}

const SERIES: Intern = Intern {
    what: "series",
    select: "SELECT id FROM series WHERE host_id = ?1 AND metric_id = ?2 AND labels = ?3",
    insert: "INSERT INTO series (host_id, metric_id, labels) VALUES (?1, ?2, ?3)",
    reserves_zero: false,
};

const PROC_NAMES: Intern = Intern {
    what: "process name",
    select: "SELECT id FROM proc_names WHERE host_id = ?1 AND name = ?2",
    insert: "INSERT INTO proc_names (host_id, name) VALUES (?1, ?2)",
    reserves_zero: false,
};

/// `proc_names` as network buckets intern into it.
const NET_NAMES: Intern = Intern {
    reserves_zero: true,
    ..PROC_NAMES
};

impl State {
    // --- interning --------------------------------------------------------------------

    pub(super) fn upsert_host(&mut self, record: &HostRecord) -> Result<()> {
        let mut info = Vec::new();
        ciborium::into_writer(&record.info, &mut info)
            .map_err(|e| StoreError::Cbor(e.to_string()))?;
        let uuid = record.id.to_string();
        if record.is_local {
            // `hosts_one_local`: the newest local registration wins (a restored or
            // replaced `host-id` file); the old host keeps its rows as a non-local host.
            let demoted = self.conn.execute(
                "UPDATE hosts SET is_local = 0 WHERE is_local = 1 AND uuid <> ?1",
                [&uuid],
            )?;
            if demoted > 0 {
                tracing::warn!(host = %record.id, "another stored host was local; it no longer is");
            }
        } else {
            // A remote host never takes the local host's row (D-071): the upsert below
            // would demote this Mac and merge a cloned Mac's history into it.
            let is_local_id: bool = self.conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM hosts WHERE uuid = ?1 AND is_local = 1)",
                [&uuid],
                |r| r.get(0),
            )?;
            if is_local_id {
                return Err(StoreError::HostConflict(record.id));
            }
        }
        self.conn.execute(
            "INSERT INTO hosts (uuid, is_local, name, info, created_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (uuid) DO UPDATE SET
               is_local = excluded.is_local, name = excluded.name, info = excluded.info",
            params![uuid, record.is_local, record.display_name, info, now_ms()],
        )?;
        Ok(())
    }

    pub(super) fn host_ref(&mut self, host: HostId) -> Result<i64> {
        if let Some(&r) = self.hosts.get(&host) {
            return Ok(r);
        }
        let r: i64 = self
            .conn
            .prepare_cached("SELECT id FROM hosts WHERE uuid = ?1")?
            .query_row([host.to_string()], |r| r.get(0))
            .optional()?
            .ok_or(StoreError::UnknownHost(host))?;
        self.hosts.insert(host, r);
        Ok(r)
    }

    pub(super) fn series_id(&mut self, host_ref: i64, key: &SeriesKey) -> Result<u32> {
        let metric = key.metric.as_str();
        let labels = key.labels.canonical();
        let id = self.intern_id(
            |s| &mut s.series,
            &SERIES,
            host_ref,
            key,
            params![host_ref, metric, labels],
            |_| true,
        )?;
        Ok(id.expect("admit always allows"))
    }

    /// The layout ID for this exact series list, minting one on first sight. Layouts are
    /// immutable: a different list, even a reordering, is a different layout.
    pub(super) fn layout_id(&mut self, host_ref: i64, series: &Arc<[SeriesKey]>) -> Result<u32> {
        if let Some(&id) = self.layouts.get(&host_ref).and_then(|m| m.get(&series[..])) {
            return Ok(id);
        }
        let mut seen = HashSet::with_capacity(series.len());
        let mut ids = Vec::with_capacity(series.len());
        for key in series.iter() {
            if !seen.insert(key) {
                return Err(StoreError::DuplicateSeries(key.to_string()));
            }
            ids.push(self.series_id(host_ref, key)?);
        }
        let packed = blob::pack_u32s(&ids);
        let hash = blob::layout_hash(&packed);
        let found: Option<(i64, Vec<u8>)> = self
            .conn
            .prepare_cached("SELECT id, series_ids FROM layouts WHERE host_id = ?1 AND hash = ?2")?
            .query_row(params![host_ref, &hash[..]], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let id = match found {
            Some((id, existing)) if existing == packed => id,
            Some((id, _)) => {
                return Err(StoreError::Corrupt(format!(
                    "layout hash collision with layout {id}"
                )));
            }
            None => {
                self.conn
                    .prepare_cached(
                        "INSERT INTO layouts (host_id, series_ids, hash, created_ms)
                         VALUES (?1, ?2, ?3, ?4)",
                    )?
                    .execute(params![host_ref, packed, &hash[..], now_ms()])?;
                self.conn.last_insert_rowid()
            }
        };
        let id = to_u32(id, "layout")?;
        self.layouts
            .entry(host_ref)
            .or_default()
            .insert(Arc::clone(series), id);
        Ok(id)
    }

    pub(super) fn proc_name_id(&mut self, host_ref: i64, name: &str) -> Result<u32> {
        let id = self.intern_id(
            |s| &mut s.proc_names,
            &PROC_NAMES,
            host_ref,
            name,
            params![host_ref, name],
            |_| true,
        )?;
        Ok(id.expect("admit always allows"))
    }

    /// The `proc_names` id of a network app, or [`OTHER_APPS`] for one with no name or
    /// a new name past this hour's [`NET_NEW_NAMES_PER_HOUR`].
    pub(super) fn net_name_id(
        &mut self,
        host_ref: i64,
        name: Option<&str>,
        bucket_ts: i64,
    ) -> Result<u32> {
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            return Ok(OTHER_APPS);
        };
        let admit = |s: &mut State| {
            let hour = bucket_ts.div_euclid(HOUR_MS);
            let used = s.net_new_names.entry(host_ref).or_insert((hour, 0));
            if used.0 != hour {
                *used = (hour, 0);
            }
            if used.1 >= NET_NEW_NAMES_PER_HOUR {
                return false;
            }
            used.1 += 1;
            true
        };
        let id = self.intern_id(
            |s| &mut s.proc_names,
            &NET_NAMES,
            host_ref,
            name,
            params![host_ref, name],
            admit,
        )?;
        Ok(id.unwrap_or(OTHER_APPS))
    }

    /// The id of `key` in `host_ref`'s `intern` table: from `cache`, else the row
    /// `intern.select` finds, else the row `intern.insert` adds once `admit` allows it
    /// (both queries bound to `params`). `None` when `admit` refused.
    fn intern_id<K, Q>(
        &mut self,
        cache: fn(&mut State) -> &mut HashMap<i64, HashMap<K, u32>>,
        intern: &Intern,
        host_ref: i64,
        key: &Q,
        params: impl Params + Copy,
        admit: impl FnOnce(&mut State) -> bool,
    ) -> Result<Option<u32>>
    where
        K: Borrow<Q> + Eq + Hash,
        Q: ToOwned<Owned = K> + Eq + Hash + Debug + ?Sized,
    {
        if let Some(&id) = cache(self).get(&host_ref).and_then(|m| m.get(key)) {
            return Ok(Some(id));
        }
        let found: Option<i64> = self
            .conn
            .prepare_cached(intern.select)?
            .query_row(params, |r| r.get(0))
            .optional()?;
        let id = match found {
            Some(id) => id,
            None => {
                if !admit(self) {
                    return Ok(None);
                }
                self.conn.prepare_cached(intern.insert)?.execute(params)?;
                self.conn.last_insert_rowid()
            }
        };
        let id = to_u32(id, intern.what)?;
        if intern.reserves_zero && id == OTHER_APPS {
            // Rowids start at 1; a 0 would read back as "other apps".
            return Err(StoreError::Corrupt(format!(
                "{} {key:?} has the reserved id 0",
                intern.what
            )));
        }
        cache(self)
            .entry(host_ref)
            .or_default()
            .insert(key.to_owned(), id);
        Ok(Some(id))
    }
}

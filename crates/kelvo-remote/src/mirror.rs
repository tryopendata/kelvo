//! Durable mirror ingestion and transient history reconciliation, independent of Tauri.
//! Epoch is the outer lock: ingestion records coverage only after the durable writer
//! acknowledges, then removes matching recent rows. Recent snapshots take epoch before
//! recent as well, so delayed snapshots cannot overwrite acknowledged durable data.
use crate::{MirrorError, MirrorSink};
use kelvo_proto::{SyncPage, WireBucket, WireLayout};
use kelvo_schema::{Catalog, HostId, SyncKinds, Tier, lock::LockExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct Mirror {
    history: Arc<kelvo_engine::HistoryStore>,
    host: HostId,
    budget: u32,
    recent: Mutex<Vec<kelvo_store::BucketRow>>,
    epoch: Mutex<MirrorEpoch>,
    /// For the cardinality caps on ingest (D-115).
    catalog: Catalog,
    /// A page had series over a cap; warned once per mirror.
    cap_warned: AtomicBool,
}

#[derive(Default)]
struct MirrorEpoch {
    id: Option<Uuid>,
    /// Only current-epoch keys in the recent-rollup window are needed to reject a
    /// delayed transient snapshot after a durable page has superseded it.
    committed: Vec<(Tier, i64, Vec<kelvo_schema::SeriesKey>)>,
    newest_ms: Option<i64>,
}
impl MirrorEpoch {
    fn trim(&mut self, now: i64) {
        self.newest_ms = Some(self.newest_ms.map_or(now, |old| old.max(now)));
        let floor = self
            .newest_ms
            .unwrap_or(now)
            .saturating_sub(kelvo_engine::RECENT_ROWS_MS);
        self.committed.retain(|(_, ts, _)| *ts >= floor);
    }
}

impl MirrorSink for Mirror {
    fn cursor(
        &self,
        tier: Tier,
        epoch: Uuid,
        kinds: SyncKinds,
    ) -> Result<Option<kelvo_schema::Cursor>, MirrorError> {
        let mut current = self.epoch.lock_ok();
        if current.id != Some(epoch) {
            self.recent.lock_ok().clear();
            *current = MirrorEpoch {
                id: Some(epoch),
                ..Default::default()
            };
        }
        self.history
            .read(|r| r.resume_cursor(self.host, tier, epoch, kinds))
            .map_err(MirrorError::from)
    }

    fn ingest(&self, mut page: SyncPage, kinds: SyncKinds) -> Result<(), MirrorError> {
        let mut current = self.epoch.lock_ok();
        if current.id != Some(page.epoch) {
            return Err(MirrorError::EpochChanged {
                operation: "history",
            });
        }
        self.cap(&mut page.layouts, &mut page.rows);
        let page = crate::page_from_wire(page, kinds)?;
        let written: Vec<_> = page
            .rows
            .iter()
            .map(|row| {
                let layout = page
                    .layouts
                    .iter()
                    .find(|layout| layout.layout_no == row.layout_no)
                    .ok_or(MirrorError::InvalidInput("Unknown durable history layout"))?;
                Ok((page.tier, row.bucket_ts, layout.series.clone()))
            })
            .collect::<Result<_, MirrorError>>()?;
        self.history
            .writer()
            .map_err(MirrorError::from)?
            .ingest_page_durable(self.host, page)
            .map_err(MirrorError::from)?;
        // Record coverage only AFTER its rows and cursor are durably committed.
        for key in &written {
            if !current.committed.contains(key) {
                current.committed.push(key.clone());
            }
        }
        if let Some(newest) = written.iter().map(|(_, ts, _)| *ts).max() {
            current.trim(newest);
        }
        self.recent.lock_ok().retain(|r| {
            !written.iter().any(|(tier, ts, series)| {
                *tier == r.tier && *ts == r.bucket_ts && series.as_slice() == r.series.as_ref()
            })
        });
        Ok(())
    }

    fn recent(
        &self,
        epoch: Uuid,
        tier: Tier,
        complete_to_ms: i64,
        mut layouts: Vec<WireLayout>,
        mut rows: Vec<WireBucket>,
    ) -> Result<(), MirrorError> {
        let mut current = self.epoch.lock_ok();
        if current.id != Some(epoch) {
            return Err(MirrorError::EpochChanged {
                operation: "recent-history",
            });
        }
        if tier != Tier::S10 {
            return Err(MirrorError::InvalidInput(
                "Unsupported or oversized recent-history snapshot",
            ));
        }
        if rows.len() > 2000 || layouts.len() > 2000 {
            return Err(MirrorError::ResourceLimit(
                "Unsupported or oversized recent-history snapshot",
            ));
        }
        let width = tier
            .bucket_ms()
            .ok_or(MirrorError::InvalidInput("Invalid recent tier"))?;
        self.cap(&mut layouts, &mut rows);
        let floor = complete_to_ms.saturating_sub(kelvo_engine::RECENT_ROWS_MS);
        let mut out = Vec::new();
        for row in rows {
            let layout = layouts
                .iter()
                .find(|l| l.layout_no == row.layout_no)
                .ok_or(MirrorError::InvalidInput("Unknown recent layout"))?;
            if row.stats.len() != layout.series.len().saturating_mul(3)
                || row.bucket_ts < floor
                || row.bucket_ts > complete_to_ms
                || row.bucket_ts.rem_euclid(width) != 0
            {
                return Err(MirrorError::InvalidInput("Invalid recent row"));
            }
            if current.committed.iter().any(|(t, ts, series)| {
                *t == tier && *ts == row.bucket_ts && *series == layout.series
            }) {
                continue;
            }
            out.push(kelvo_store::BucketRow {
                host: self.host,
                tier,
                bucket_ts: row.bucket_ts,
                series: layout.series.clone().into(),
                stats: row.stats,
            });
        }
        // The snapshot may include the bucket beginning at complete_to_ms: it is the
        // open bucket. Exact committed keys win over that transient value.
        current.trim(complete_to_ms);
        *self.recent.lock_ok() = out;
        Ok(())
    }

    fn truncated(&self, tier: Tier, epoch: Uuid, earliest_ms: i64) -> Result<(), MirrorError> {
        let current = self.epoch.lock_ok();
        if current.id != Some(epoch) {
            return Err(MirrorError::EpochChanged {
                operation: "truncation",
            });
        }
        let writer = self.history.writer().map_err(MirrorError::from)?;
        writer
            .record_truncation(self.host, tier, earliest_ms)
            .map_err(MirrorError::from)?;
        writer.flush().map_err(MirrorError::from)
    }

    fn enforce_budget(&self, now_ms: i64) -> Result<(), MirrorError> {
        if self.history.size_on_disk().map_err(MirrorError::from)?
            > u64::from(self.budget) * 1_000_000
        {
            self.prune(now_ms)?;
        }
        Ok(())
    }
    fn prune(&self, now_ms: i64) -> Result<(), MirrorError> {
        let _current = self.epoch.lock_ok();
        let retention = kelvo_store::Retention {
            max_bytes: u64::from(self.budget) * 1_000_000,
            ..kelvo_store::Retention::mirror_default()
        };
        let report = self
            .history
            .writer()
            .map_err(MirrorError::from)?
            .prune_mirror(now_ms, retention)
            .map_err(MirrorError::from)?;
        self.history
            .record_mirror_prune(&report, now_ms, &retention);
        if report.cap_trim.is_some_and(|trim| !trim.cap_met) {
            return Err(MirrorError::ResourceLimit(
                "The mirror's fixed database overhead exceeds its storage budget",
            ));
        }
        Ok(())
    }
}
impl Mirror {
    pub fn new(history: Arc<kelvo_engine::HistoryStore>, host: HostId, budget: u32) -> Self {
        Self {
            history,
            host,
            budget,
            recent: Mutex::default(),
            epoch: Mutex::default(),
            catalog: Catalog::builtin(),
            cap_warned: AtomicBool::new(false),
        }
    }

    /// Applies the cardinality caps to a remote page or snapshot before anything is
    /// stored (D-115).
    fn cap(&self, layouts: &mut [WireLayout], rows: &mut [WireBucket]) {
        let dropped = crate::pages::cap_layouts(&self.catalog, layouts, rows);
        if dropped > 0 && !self.cap_warned.swap(true, Ordering::Relaxed) {
            tracing::warn!(host = ?self.host, dropped, "remote series over their cap; not mirrored");
        }
    }
    pub fn store(&self) -> Arc<kelvo_engine::HistoryStore> {
        self.history.clone()
    }
    pub fn recent_rows(&self, from: i64, to: i64) -> Vec<kelvo_store::BucketRow> {
        self.recent
            .lock_ok()
            .iter()
            .filter(|r| r.bucket_ts >= from - 60_000 && r.bucket_ts < to)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests;

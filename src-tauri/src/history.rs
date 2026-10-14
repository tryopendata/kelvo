//! Shell history adapter. Store lifecycle/read pooling lives in kelvo-engine;
//! query adapters and IPC conversion remain beside their command-facing owner.
use crate::error::{CommandError, HistoryUnavailableReason};
use crate::ipc::{
    BatteryHour, ExportRequest, HeatmapDay, HeatmapMetric, HeatmapRequest, HistoryHealth,
    HistoryPage, HistoryPoint, HistoryRequest, HistorySeries, NetworkByApp, NetworkSpan,
    ProcessResolution, ProcessesAt, StoredProcess, TierRequest,
};
use kelvo_engine::{HistoryStore, HousekeepingHandle};
use kelvo_schema::{HostRecord, Labels, MetricId, SeriesKey, Tier};
use kelvo_store::{
    BATTERY_CHARGE, BATTERY_CHARGING, BatteryCell, BucketRow, ExportQuery, HistoryQuery,
    HistoryResult, ProcResolution, RangeStats, Reader, Retention, TierChoice, Writer,
    fill_battery_hours,
};
use std::path::Path;
use std::sync::Arc;
mod ipc;
mod queries;
use ipc::MAX_BATTERY_HOURS;
#[cfg(test)]
use ipc::MAX_HEATMAP_DAYS;
use ipc::history_page;
#[cfg(test)]
mod tests;
pub use kelvo_engine::HISTORY_DB_FILE as DB_FILE;
#[cfg(test)]
use kelvo_store::{Store, StoreConfig};

pub(crate) const MAX_NET_SPAN_MS: i64 = {
    let days = kelvo_schema::settings::HistorySettings::RETENTION_DAYS;
    days[days.len() - 1] as i64 * Retention::DAY_MS
};

pub struct History {
    storage: Arc<HistoryStore>,
}
impl History {
    pub fn open(dir: &Path) -> Self {
        Self {
            storage: Arc::new(HistoryStore::open(dir)),
        }
    }
    pub fn unavailable() -> Self {
        Self {
            storage: Arc::new(HistoryStore::unavailable()),
        }
    }
    pub(super) fn store(&self) -> Arc<HistoryStore> {
        self.storage.clone()
    }
    pub fn register_host(&self, record: &HostRecord) -> Option<Writer> {
        self.storage.register_host(record)
    }
    pub fn health(&self) -> HistoryHealth {
        self.storage.health().into()
    }
    pub fn clear_trim(&self) -> Option<HistoryHealth> {
        self.storage.clear_trim().map(Into::into)
    }
    pub fn is_available(&self) -> bool {
        self.storage.is_available()
    }
    pub fn unavailable_reason(&self) -> Option<HistoryUnavailableReason> {
        self.storage.unavailable_reason()
    }
    pub fn writer(&self) -> Result<Writer, CommandError> {
        Ok(self.storage.writer()?)
    }
    pub fn read<T>(
        &self,
        f: impl FnOnce(&mut Reader) -> kelvo_store::Result<T>,
    ) -> Result<T, CommandError> {
        Ok(self.storage.read(f)?)
    }
    pub fn size_on_disk(&self) -> Result<u64, CommandError> {
        Ok(self.storage.size_on_disk()?)
    }
    pub fn close(&self) {
        self.storage.close();
    }
    pub fn reset(&self, now_ms: i64) -> Result<Writer, CommandError> {
        Ok(self.storage.reset(now_ms)?)
    }
    pub fn spawn_pruner(
        &self,
        retention: impl Fn() -> Retention + Send + 'static,
        on_change: impl Fn(HistoryHealth) + Send + 'static,
    ) -> std::io::Result<HousekeepingHandle> {
        self.storage
            .spawn_pruner(retention, move |health| on_change(health.into()))
    }
}

impl History {
    /// [`History::reset`], then `attach` (reattach every source, registering its host),
    /// then confirm the store is still there. Returns the health to announce; an error
    /// means history is unavailable and nothing should be announced, so a window's cached
    /// `history_unavailable` (and its banner) stays.
    pub fn reset_and_attach(
        &self,
        now_ms: i64,
        attach: impl FnOnce(&History),
    ) -> Result<HistoryHealth, CommandError> {
        self.reset(now_ms)?;
        attach(self);
        if !self.is_available() {
            return Err(self
                .writer()
                .err()
                .unwrap_or_else(CommandError::history_unavailable));
        }
        Ok(self.health())
    }
}

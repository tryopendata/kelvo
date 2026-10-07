//! The hosts the app knows, each with its [`Source`] and [`LiveHub`] (architecture.md
//! infra 5). v1 registers one, the local machine; v4 adds remote hosts here without
//! changing a command signature. The hub, not the source, holds the ring buffer and the
//! latest frame, so every source's output reaches windows the same way (D-066).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use kelvo_engine::{
    Bus, LiveFrame, LiveHub, RecentNet, Source, SourceControl, SourceError, SourceHandle,
    SourceSink,
};
use kelvo_schema::lock::{LockExt, RwLockExt};
use kelvo_schema::{Capabilities, HostId, HostRecord, SeriesKey, Settings};
use kelvo_store::{BucketRow, Writer};

use crate::error::CommandError;
use crate::live::LiveFeed;

/// One host: its record, the source producing for it, and the hub the source publishes
/// through.
pub struct HostEntry {
    record: RwLock<HostRecord>,
    source: Arc<dyn Source>,
    handle: Mutex<Option<SourceHandle>>,
    live: LiveHub,
}

impl HostEntry {
    fn handle(&self) -> MutexGuard<'_, Option<SourceHandle>> {
        self.handle.lock_ok()
    }

    fn with_control<T>(&self, f: impl FnOnce(&dyn SourceControl) -> T) -> Option<T> {
        self.handle().as_ref().map(|h| f(h.control()))
    }

    pub fn record(&self) -> HostRecord {
        self.record.read_ok().clone()
    }

    /// Replaces the record (the local host learns `chip_known` from its first
    /// capabilities). Returns whether it changed.
    pub fn update_record(&self, f: impl FnOnce(&mut HostRecord)) -> bool {
        let mut r = self.record.write_ok();
        let before = r.clone();
        f(&mut r);
        *r != before
    }

    pub fn bus(&self) -> &Bus {
        self.live.bus()
    }

    /// Fills the hub's ring from stored history before the source's first frame
    /// ([`LiveHub::warm`]). Returns the rows added.
    pub fn warm_ring(&self, history: &kelvo_store::HistoryResult) -> usize {
        self.live.warm(history)
    }

    /// The most recent frame, for readers that need current values without a stream (the
    /// sensor dump; the tray subscribes to the bus instead).
    pub fn latest_frame(&self) -> Option<Arc<LiveFrame>> {
        self.live.latest_frame()
    }

    /// Starts the source into this host's hub and, when history is available, the store.
    pub fn start(&self, store: Option<Writer>) -> Result<(), SourceError> {
        let handle = Arc::clone(&self.source).start(SourceSink {
            live: self.live.clone(),
            store,
        })?;
        *self.handle() = Some(handle);
        Ok(())
    }

    /// Stops the source and waits for it (the engine flushes buckets and closes gaps).
    pub fn stop(&self) {
        let handle = self.handle().take();
        if let Some(mut h) = handle {
            h.stop();
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.with_control(|c| c.set_paused(paused));
    }

    /// The "Network history" setting (D-089).
    pub fn set_network_history(&self, on: bool) {
        self.with_control(|c| c.set_network_history(on));
    }

    /// Per-app network buckets of the last hour from this host's hub, and where its open
    /// ones start, for `History::network_by_app`.
    pub fn recent_net(&self, from_ms: i64, to_ms: i64) -> RecentNet {
        self.live.recent_net(from_ms, to_ms)
    }

    /// Per-app use over a range from this host's hub, the largest `limit` by `by`
    /// (D-093, D-099).
    pub fn usage_by_app(
        &self,
        from_ms: i64,
        to_ms: i64,
        by: kelvo_engine::UsageKey,
        limit: usize,
    ) -> kelvo_engine::UsageByApp {
        self.live.usage_by_app(from_ms, to_ms, by, limit)
    }

    /// The interface carrying the default route, as the engine last reported it.
    pub fn primary_iface(&self) -> Option<String> {
        self.live
            .status()
            .primary_iface
            .as_deref()
            .map(str::to_owned)
    }

    /// History bucket rows of the last 15 minutes and the open buckets from this host's
    /// hub, for `History::history` (D-092).
    pub fn recent_rows(&self, host: HostId, from_ms: i64, to_ms: i64) -> Vec<BucketRow> {
        self.live.recent_rows(host, from_ms, to_ms)
    }

    /// Forgets the hub's recent rows and open buckets (`clear_history`).
    pub fn forget_recent_rows(&self) {
        self.live.forget_recent_rows();
    }

    /// `HistorySeries.hold_ms` of `key` in buckets of `bucket_ms`, in a range starting at
    /// `from_ms` (D-092).
    pub fn history_hold_ms(&self, key: &SeriesKey, bucket_ms: i64, from_ms: i64) -> i64 {
        self.live.history_hold_ms(key, bucket_ms, from_ms)
    }

    pub fn apply_settings(&self, settings: &Settings) {
        self.with_control(|c| c.apply_settings(settings));
    }

    /// Detaches the running source from the store (`None`) or attaches it to a new one.
    /// Returns once the source has applied it.
    pub fn set_store(&self, store: Option<Writer>) {
        self.with_control(|c| c.set_store(store));
    }
}

impl LiveFeed for HostEntry {
    fn host(&self) -> HostId {
        self.record.read_ok().id
    }

    fn hub(&self) -> &LiveHub {
        &self.live
    }

    fn capabilities(&self) -> Capabilities {
        self.source.capabilities()
    }

    fn set_process_interest(&self, period_ms: Option<u32>) {
        self.with_control(|c| c.set_process_interest(period_ms));
    }

    fn set_network_process_interest(&self, interested: bool) {
        self.with_control(|c| c.set_network_process_interest(interested));
    }

    fn set_gpu_process_interest(&self, interested: bool) {
        self.with_control(|c| c.set_gpu_process_interest(interested));
    }

    fn set_port_process_interest(&self, interested: bool) {
        self.with_control(|c| c.set_port_process_interest(interested));
    }

    fn set_detail_interest(&self, interested: bool) {
        self.with_control(|c| c.set_detail_interest(interested));
    }
}

#[derive(Default)]
pub struct HostRegistry {
    hosts: RwLock<HashMap<HostId, Arc<HostEntry>>>,
}

impl HostRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a host with its source, not yet started.
    pub fn insert(&self, record: HostRecord, source: Arc<dyn Source>) -> Arc<HostEntry> {
        let id = record.id;
        let entry = Arc::new(HostEntry {
            record: RwLock::new(record),
            source,
            handle: Mutex::new(None),
            live: LiveHub::default(),
        });
        self.hosts.write_ok().insert(id, Arc::clone(&entry));
        entry
    }

    pub fn get(&self, host: HostId) -> Result<Arc<HostEntry>, CommandError> {
        self.hosts
            .read_ok()
            .get(&host)
            .cloned()
            .ok_or(CommandError::UnknownHost { host })
    }

    /// Every host, local first.
    pub fn all(&self) -> Vec<Arc<HostEntry>> {
        let mut all: Vec<_> = self.hosts.read_ok().values().cloned().collect();
        all.sort_by_key(|h| {
            let r = h.record();
            (!r.is_local, r.display_name)
        });
        all
    }

    pub fn stop_all(&self) {
        for h in self.all() {
            h.stop();
        }
    }
}
